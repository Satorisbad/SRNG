use std::io::Cursor;

pub const MAX_DATA_URI_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_DECODED_PIXELS: u64 = 16_777_216;
pub const MAX_DECODED_BYTES: usize = (MAX_DECODED_PIXELS as usize) * 4;
pub const MAX_DIMENSION: u32 = 16_384;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageCodec { Png, Jpeg, WebP, Gif, Svg }
impl ImageCodec { pub fn mime(self)->&'static str { match self { Self::Png=>"image/png",Self::Jpeg=>"image/jpeg",Self::WebP=>"image/webp",Self::Gif=>"image/gif",Self::Svg=>"image/svg+xml" } } }
#[derive(Debug, Clone, PartialEq)]
pub struct DecodedImage { pub codec:ImageCodec,pub width:u32,pub height:u32,pub pixels:Vec<u8> }

pub fn decode_data_uri(uri:&str)->Result<DecodedImage,String>{
    let(meta,payload)=uri.split_once(',').ok_or_else(||"invalid data image URI: missing comma".to_string())?;
    let meta=meta.strip_prefix("data:").ok_or_else(||"invalid data image URI scheme".to_string())?;
    let mut parts=meta.split(';');let declared=parts.next().unwrap_or("").trim().to_ascii_lowercase();let is_base64=parts.any(|p|p.eq_ignore_ascii_case("base64"));
    let bytes=if is_base64{decode_base64(payload)?}else{percent_decode(payload)?};
    if bytes.len()>MAX_DATA_URI_BYTES{return Err(format!("embedded image payload exceeds {MAX_DATA_URI_BYTES} byte limit"));}
    let codec=sniff_codec(&bytes).ok_or_else(||"embedded image payload is not a supported raster/SVG format".to_string())?;
    if !declared.is_empty()&&declared!=codec.mime()&&!(codec==ImageCodec::Jpeg&&declared=="image/jpg"){return Err(format!("embedded image MIME `{declared}` does not match detected `{}` payload",codec.mime()));}
    decode_bytes(codec,&bytes)
}

pub fn decode_bytes(codec:ImageCodec,bytes:&[u8])->Result<DecodedImage,String>{match codec{ImageCodec::Svg=>decode_svg(bytes),ImageCodec::Png|ImageCodec::Jpeg|ImageCodec::WebP|ImageCodec::Gif=>decode_raster(codec,bytes)}}
pub fn sniff_codec(bytes:&[u8])->Option<ImageCodec>{
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n"){return Some(ImageCodec::Png)}
    if bytes.len()>=3&&bytes[0..3]==[0xff,0xd8,0xff]{return Some(ImageCodec::Jpeg)}
    if bytes.len()>=12&&&bytes[0..4]==b"RIFF"&&&bytes[8..12]==b"WEBP"{return Some(ImageCodec::WebP)}
    if bytes.starts_with(b"GIF87a")||bytes.starts_with(b"GIF89a"){return Some(ImageCodec::Gif)}
    let probe=bytes.iter().copied().skip_while(u8::is_ascii_whitespace).take(256).collect::<Vec<_>>();if probe.starts_with(b"<?xml")||probe.starts_with(b"<svg")||probe.windows(4).any(|w|w==b"<svg"){return Some(ImageCodec::Svg)}None
}
fn decode_raster(codec:ImageCodec,bytes:&[u8])->Result<DecodedImage,String>{
    let format=match codec{ImageCodec::Png=>image::ImageFormat::Png,ImageCodec::Jpeg=>image::ImageFormat::Jpeg,ImageCodec::WebP=>image::ImageFormat::WebP,ImageCodec::Gif=>image::ImageFormat::Gif,ImageCodec::Svg=>unreachable!()};
    let reader=image::ImageReader::with_format(Cursor::new(bytes),format);let(width,height)=reader.into_dimensions().map_err(|e|format!("invalid embedded {}: {e}",codec.mime()))?;validate_dimensions(width,height)?;
    let decoded=image::ImageReader::with_format(Cursor::new(bytes),format).decode().map_err(|e|format!("could not decode embedded {}: {e}",codec.mime()))?;let pixels=decoded.to_rgba8().into_raw();if pixels.len()>MAX_DECODED_BYTES{return Err("decoded image exceeds memory limit".into())}Ok(DecodedImage{codec,width,height,pixels})
}
fn decode_svg(bytes:&[u8])->Result<DecodedImage,String>{use resvg::{tiny_skia,usvg};let text=std::str::from_utf8(bytes).map_err(|_|"embedded SVG is not UTF-8".to_string())?;let tree=usvg::Tree::from_str(text,&usvg::Options::default()).map_err(|e|format!("invalid embedded SVG: {e}"))?;let size=tree.size();let width=size.width().ceil()as u32;let height=size.height().ceil()as u32;validate_dimensions(width,height)?;let mut source=tiny_skia::Pixmap::new(width,height).ok_or_else(||"could not allocate embedded SVG".to_string())?;resvg::render(&tree,tiny_skia::Transform::identity(),&mut source.as_mut());let pixels=source.data().to_vec();if pixels.len()>MAX_DECODED_BYTES{return Err("decoded SVG exceeds memory limit".into())}Ok(DecodedImage{codec:ImageCodec::Svg,width,height,pixels})}
pub fn validate_dimensions(width:u32,height:u32)->Result<(),String>{if width==0||height==0{return Err("embedded image has empty dimensions".into())}if width>MAX_DIMENSION||height>MAX_DIMENSION{return Err(format!("embedded image dimensions {width}x{height} exceed {MAX_DIMENSION}px per-axis limit"))}let pixels=u64::from(width).checked_mul(u64::from(height)).ok_or_else(||"embedded image dimensions overflow".to_string())?;if pixels>MAX_DECODED_PIXELS{return Err(format!("embedded image has {pixels} pixels; limit is {MAX_DECODED_PIXELS}"))}Ok(())}
pub fn png_data_uri(decoded:&DecodedImage)->Result<String,String>{use base64::Engine as _;let rgba=image::RgbaImage::from_raw(decoded.width,decoded.height,decoded.pixels.clone()).ok_or_else(||"decoded RGBA buffer length does not match dimensions".to_string())?;let mut cursor=Cursor::new(Vec::new());image::DynamicImage::ImageRgba8(rgba).write_to(&mut cursor,image::ImageFormat::Png).map_err(|e|format!("could not normalize embedded image to PNG: {e}"))?;Ok(format!("data:image/png;base64,{}",base64::engine::general_purpose::STANDARD.encode(cursor.into_inner())))}
fn decode_base64(input:&str)->Result<Vec<u8>,String>{use base64::Engine as _;let compact=input.bytes().filter(|b|!b.is_ascii_whitespace()).collect::<Vec<_>>();if compact.len()>MAX_DATA_URI_BYTES.saturating_mul(2){return Err("base64 image payload is too large".into())}base64::engine::general_purpose::STANDARD.decode(compact).map_err(|_|"invalid base64 in image URI".to_string())}
fn percent_decode(input:&str)->Result<Vec<u8>,String>{let bytes=input.as_bytes();if bytes.len()>MAX_DATA_URI_BYTES.saturating_mul(3){return Err("percent-encoded image payload is too large".into())}let mut out=Vec::with_capacity(bytes.len().min(MAX_DATA_URI_BYTES));let mut i=0;while i<bytes.len(){if out.len()>=MAX_DATA_URI_BYTES{return Err("embedded image payload exceeds byte limit".into())}if bytes[i]==b'%'{if i+2>=bytes.len(){return Err("truncated percent escape in data URI".into())}out.push((hex(bytes[i+1])?<<4)|hex(bytes[i+2])?);i+=3}else{out.push(bytes[i]);i+=1}}Ok(out)}
fn hex(b:u8)->Result<u8,String>{match b{b'0'..=b'9'=>Ok(b-b'0'),b'a'..=b'f'=>Ok(b-b'a'+10),b'A'..=b'F'=>Ok(b-b'A'+10),_=>Err("invalid percent escape in data URI".into())}}
#[cfg(test)]mod tests{use super::*;#[test]fn sniff_gif(){assert_eq!(sniff_codec(b"GIF89a123"),Some(ImageCodec::Gif));}#[test]fn mime_mismatch_is_rejected(){assert!(decode_data_uri("data:image/png;base64,R0lGODlh").unwrap_err().contains("does not match"));}#[test]fn malformed_percent_escape_is_rejected(){assert!(decode_data_uri("data:image/svg+xml,%GG").is_err());}}
