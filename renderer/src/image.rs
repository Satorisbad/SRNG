use std::io::Cursor;

pub const MAX_DATA_URI_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_DECODED_PIXELS: u64 = 16_777_216;
pub const MAX_DECODED_BYTES: usize = (MAX_DECODED_PIXELS as usize) * 4;
pub const MAX_DIMENSION: u32 = 16_384;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageCodec { Png, Jpeg, WebP, Gif, Svg }

impl ImageCodec {
    pub fn mime(self) -> &'static str {
        match self {
            Self::Png => "image/png",
            Self::Jpeg => "image/jpeg",
            Self::WebP => "image/webp",
            Self::Gif => "image/gif",
            Self::Svg => "image/svg+xml",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct DecodedImage {
    pub codec: ImageCodec,
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

pub fn sniff_codec(bytes: &[u8]) -> Option<ImageCodec> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") { return Some(ImageCodec::Png); }
    if bytes.len() >= 3 && bytes[0..3] == [0xff, 0xd8, 0xff] { return Some(ImageCodec::Jpeg); }
    if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" { return Some(ImageCodec::WebP); }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") { return Some(ImageCodec::Gif); }
    let probe = bytes.iter().copied().skip_while(u8::is_ascii_whitespace).take(256).collect::<Vec<_>>();
    if probe.starts_with(b"<?xml") || probe.starts_with(b"<svg") || probe.windows(4).any(|w| w == b"<svg") { return Some(ImageCodec::Svg); }
    None
}

pub fn validate_dimensions(width: u32, height: u32) -> Result<(), String> {
    if width == 0 || height == 0 { return Err("embedded image has empty dimensions".into()); }
    if width > MAX_DIMENSION || height > MAX_DIMENSION { return Err(format!("embedded image dimensions {width}x{height} exceed {MAX_DIMENSION}px per-axis limit")); }
    let pixels = u64::from(width).checked_mul(u64::from(height)).ok_or_else(|| "embedded image dimensions overflow".to_string())?;
    if pixels > MAX_DECODED_PIXELS { return Err(format!("embedded image has {pixels} pixels; limit is {MAX_DECODED_PIXELS}")); }
    Ok(())
}
