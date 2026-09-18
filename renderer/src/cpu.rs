use crate::{Command, EmbeddedImage, FillRule, FilterOp, GradientSpread, LineCap, LineJoin, Paint, PreparedScene, RenderDiagnostic, Rgba, VectorRecord};
use std::{io::Cursor, sync::Arc};
use vello_cpu::{
    color::{AlphaColor, Srgb},
    kurbo::{Affine, BezPath, Cap, Join, Stroke as KurboStroke},
    peniko::{ColorStop, ColorStops, Extend, Fill, Gradient, ImageSampler},
    Image, ImageSource, Pixmap, RenderContext, Resources,
};

#[derive(Debug)]
pub struct CpuOutput { pub width:u16, pub height:u16, pub pixels:Vec<u8>, pub diagnostics:Vec<RenderDiagnostic> }

pub fn render(scene:&PreparedScene)->CpuOutput{
    let mut context=RenderContext::new(scene.width,scene.height);let mut diagnostics=scene.diagnostics.clone();
    if let Err(message)=render_commands(&mut context,&scene.commands,scene.width,scene.height){diagnostics.push(RenderDiagnostic{severity:"error".into(),code:"G500".into(),message,declaration:None});}
    context.flush();let mut resources=Resources::new();let mut pixmap=Pixmap::new(scene.width,scene.height);context.render(&mut pixmap,&mut resources);
    CpuOutput{width:scene.width,height:scene.height,pixels:pixmap.data_as_u8_slice().to_vec(),diagnostics}
}

fn render_commands(context:&mut RenderContext,commands:&[Command],width:u16,height:u16)->Result<(),String>{
    let mut index=0;
    while index<commands.len(){
        match &commands[index]{
            Command::PushMask{records}=>{let end=matching_end(commands,index,|c|matches!(c,Command::PushMask{..}),|c|matches!(c,Command::PopMask),"mask")?;let mut layer=render_layer(&commands[index+1..end],width,height)?;let mask=rasterize_records(records,width,height)?;apply_alpha_mask(&mut layer,&mask)?;composite_pixmap(context,layer,width,height)?;index=end+1;}
            Command::PushFilter{filters}=>{let end=matching_end(commands,index,|c|matches!(c,Command::PushFilter{..}),|c|matches!(c,Command::PopFilter),"filter")?;let mut layer=render_layer(&commands[index+1..end],width,height)?;apply_filters(&mut layer,filters,width,height)?;composite_pixmap(context,layer,width,height)?;index=end+1;}
            Command::PopMask=>return Err("encountered unmatched mask terminator".into()),
            Command::PopFilter=>return Err("encountered unmatched filter terminator".into()),
            command=>{apply_simple(context,command)?;index+=1;}
        }
    }
    Ok(())
}

fn render_layer(commands:&[Command],width:u16,height:u16)->Result<Pixmap,String>{let mut context=RenderContext::new(width,height);render_commands(&mut context,commands,width,height)?;context.flush();let mut resources=Resources::new();let mut pixmap=Pixmap::new(width,height);context.render(&mut pixmap,&mut resources);Ok(pixmap)}
fn matching_end<F,G>(commands:&[Command],start:usize,is_push:F,is_pop:G,name:&str)->Result<usize,String>where F:Fn(&Command)->bool,G:Fn(&Command)->bool{let mut depth=0usize;for(index,command)in commands.iter().enumerate().skip(start){if is_push(command){depth+=1;}else if is_pop(command){if depth==0{return Err(format!("encountered unmatched {name} terminator"));}depth-=1;if depth==0{return Ok(index);}}}Err(format!("{name} block is missing its terminator"))}

fn apply_simple(context:&mut RenderContext,command:&Command)->Result<(),String>{match command{
    Command::PushClip{path,rule}=>{context.set_fill_rule(to_fill(*rule));context.push_clip_path(&parse_path(&path.svg)?);}
    Command::PopClip=>context.pop_clip_path(),
    Command::PushMask{..}|Command::PopMask|Command::PushFilter{..}|Command::PopFilter=>return Err("layer commands must be handled as blocks".into()),
    Command::DrawImage{image}=>draw_image(context,image)?,
    Command::Fill{path,paint,rule}=>{context.set_fill_rule(to_fill(*rule));set_paint(context,paint)?;context.fill_path(&parse_path(&path.svg)?);}
    Command::Stroke{path,paint,style}=>{set_paint(context,paint)?;let stroke=KurboStroke::new(style.width).with_miter_limit(style.miter_limit).with_caps(cap(style.line_cap)).with_join(join(style.line_join)).with_dashes(style.dash_offset,style.dash.iter());context.set_stroke(stroke);context.stroke_path(&parse_path(&path.svg)?);}
}Ok(())}

fn draw_image(context:&mut RenderContext,image:&EmbeddedImage)->Result<(),String>{
    let source=decode_data_image(&image.href)?;let sw=f64::from(source.width());let sh=f64::from(source.height());if sw<=0.0||sh<=0.0{return Err("embedded image has empty dimensions".into());}
    let (x,y,w,h)=fit_image(image.x,image.y,image.width,image.height,sw,sh,&image.preserve_aspect_ratio);
    let sampler=ImageSampler::default();let paint=Image{image:ImageSource::Pixmap(Arc::new(source)),sampler};context.reset_paint_transform();context.set_paint(paint);context.set_paint_transform(Affine::translate((x,y))*Affine::scale_non_uniform(w/sw,h/sh));context.set_fill_rule(Fill::NonZero);context.fill_path(&parse_path(&format!("M 0 0 H {sw} V {sh} H 0 Z"))?);context.reset_paint_transform();Ok(())
}

fn fit_image(x:f64,y:f64,w:f64,h:f64,sw:f64,sh:f64,preserve:&str)->(f64,f64,f64,f64){if preserve.trim().starts_with("none"){return(x,y,w,h);}let slice=preserve.contains("slice");let scale=if slice{(w/sw).max(h/sh)}else{(w/sw).min(h/sh)};let rw=sw*scale;let rh=sh*scale;let dx=if preserve.contains("xMin"){0.0}else if preserve.contains("xMax"){w-rw}else{(w-rw)/2.0};let dy=if preserve.contains("YMin"){0.0}else if preserve.contains("YMax"){h-rh}else{(h-rh)/2.0};(x+dx,y+dy,rw,rh)}

fn decode_data_image(uri:&str)->Result<Pixmap,String>{let(rest,payload)=uri.split_once(',').ok_or_else(||"invalid data image URI".to_string())?;let mime=rest.strip_prefix("data:").unwrap_or(rest).split(';').next().unwrap_or("");let bytes=if rest.contains(";base64"){decode_base64(payload)?}else{percent_decode(payload)?};match mime{"image/png"=>decode_png(&bytes),"image/svg+xml"=>decode_svg(&bytes),other=>Err(format!("unsupported embedded image type `{other}`; native v0.5 supports PNG and SVG data images"))}}
fn decode_png(bytes:&[u8])->Result<Pixmap,String>{let decoder=png::Decoder::new(Cursor::new(bytes));let mut reader=decoder.read_info().map_err(|e|format!("invalid embedded PNG: {e}"))?;let mut buf=vec![0;reader.output_buffer_size()];let info=reader.next_frame(&mut buf).map_err(|e|format!("could not decode embedded PNG: {e}"))?;let width=u16::try_from(info.width).map_err(|_|"embedded PNG is too wide".to_string())?;let height=u16::try_from(info.height).map_err(|_|"embedded PNG is too tall".to_string())?;let mut pixmap=Pixmap::new(width,height);let out=pixmap.data_as_u8_slice_mut();let src=&buf[..info.buffer_size()];match info.color_type{png::ColorType::Rgba=>out.copy_from_slice(src),png::ColorType::Rgb=>{for(dst,s)in out.chunks_exact_mut(4).zip(src.chunks_exact(3)){dst[0]=s[0];dst[1]=s[1];dst[2]=s[2];dst[3]=255;}},png::ColorType::Grayscale=>{for(dst,&v)in out.chunks_exact_mut(4).zip(src){dst[0]=v;dst[1]=v;dst[2]=v;dst[3]=255;}},png::ColorType::GrayscaleAlpha=>{for(dst,s)in out.chunks_exact_mut(4).zip(src.chunks_exact(2)){dst[0]=s[0];dst[1]=s[0];dst[2]=s[0];dst[3]=s[1];}},png::ColorType::Indexed=>return Err("indexed embedded PNG must be expanded by decoder; unsupported decoder output".into())}pixmap.recompute_may_have_transparency();Ok(pixmap)}
fn decode_svg(bytes:&[u8])->Result<Pixmap,String>{use resvg::{tiny_skia,usvg};let text=std::str::from_utf8(bytes).map_err(|_|"embedded SVG is not UTF-8".to_string())?;let options=svg_options(text);let tree=usvg::Tree::from_str(text,&options).map_err(|e|format!("invalid embedded SVG: {e}"))?;let size=tree.size();let width=size.width().ceil().clamp(1.0,4096.0)as u16;let height=size.height().ceil().clamp(1.0,4096.0)as u16;let mut source=tiny_skia::Pixmap::new(u32::from(width),u32::from(height)).ok_or_else(||"could not allocate embedded SVG".to_string())?;resvg::render(&tree,tiny_skia::Transform::from_scale(f32::from(width)/size.width(),f32::from(height)/size.height()),&mut source.as_mut());let mut pixmap=Pixmap::new(width,height);pixmap.data_as_u8_slice_mut().copy_from_slice(source.data());pixmap.recompute_may_have_transparency();Ok(pixmap)}

fn decode_base64(input:&str)->Result<Vec<u8>,String>{let mut out=Vec::with_capacity(input.len()*3/4);let mut acc=0u32;let mut bits=0u8;for b in input.bytes().filter(|b|!b.is_ascii_whitespace()){if b==b'='{break;}let v=match b{b'A'..=b'Z'=>b-b'A',b'a'..=b'z'=>b-b'a'+26,b'0'..=b'9'=>b-b'0'+52,b'+'=>62,b'/'=>63,_=>return Err("invalid base64 in image URI".into())};acc=(acc<<6)|u32::from(v);bits+=6;if bits>=8{bits-=8;out.push(((acc>>bits)&0xff)as u8);}}Ok(out)}
fn percent_decode(input:&str)->Result<Vec<u8>,String>{let bytes=input.as_bytes();let mut out=Vec::with_capacity(bytes.len());let mut i=0;while i<bytes.len(){if bytes[i]==b'%'{if i+2>=bytes.len(){return Err("truncated percent escape in data URI".into());}let h=hex(bytes[i+1])?;let l=hex(bytes[i+2])?;out.push((h<<4)|l);i+=3;}else{out.push(bytes[i]);i+=1;}}Ok(out)}
fn hex(b:u8)->Result<u8,String>{match b{b'0'..=b'9'=>Ok(b-b'0'),b'a'..=b'f'=>Ok(b-b'a'+10),b'A'..=b'F'=>Ok(b-b'A'+10),_=>Err("invalid percent escape in data URI".into())}}

fn apply_filters(pixmap:&mut Pixmap,filters:&[FilterOp],width:u16,height:u16)->Result<(),String>{for filter in filters{match *filter{FilterOp::GaussianBlur{sigma_x,sigma_y}=>gaussian_blur(pixmap,sigma_x,sigma_y,width,height),FilterOp::Offset{dx,dy}=>offset_pixmap(pixmap,dx.round()as i32,dy.round()as i32,width,height)}}Ok(())}
fn gaussian_blur(pixmap:&mut Pixmap,sigma_x:f64,sigma_y:f64,width:u16,height:u16){let rx=(sigma_x*3.0).ceil().clamp(0.0,64.0)as i32;let ry=(sigma_y*3.0).ceil().clamp(0.0,64.0)as i32;if rx==0&&ry==0{return;}let mut data=pixmap.data_as_u8_slice().to_vec();if rx>0{data=blur_axis(&data,usize::from(width),usize::from(height),rx,true);}if ry>0{data=blur_axis(&data,usize::from(width),usize::from(height),ry,false);}pixmap.data_as_u8_slice_mut().copy_from_slice(&data);pixmap.recompute_may_have_transparency();}
fn blur_axis(src:&[u8],w:usize,h:usize,r:i32,horizontal:bool)->Vec<u8>{let sigma=(r as f64/3.0).max(0.333);let mut weights=Vec::with_capacity((r*2+1)as usize);let mut sum=0.0;for i in -r..=r{let v=(-((i*i)as f64)/(2.0*sigma*sigma)).exp();weights.push(v);sum+=v;}for v in &mut weights{*v/=sum;}let mut out=vec![0;src.len()];for y in 0..h{for x in 0..w{for c in 0..4{let mut acc=0.0;for(i,&weight)in(-r..=r).zip(weights.iter()){let(nx,ny)=if horizontal{((x as i32+i).clamp(0,w as i32-1)as usize,y)}else{(x,(y as i32+i).clamp(0,h as i32-1)as usize)};acc+=f64::from(src[(ny*w+nx)*4+c])*weight;}out[(y*w+x)*4+c]=acc.round().clamp(0.0,255.0)as u8;}}}out}
fn offset_pixmap(pixmap:&mut Pixmap,dx:i32,dy:i32,width:u16,height:u16){let w=usize::from(width);let h=usize::from(height);let src=pixmap.data_as_u8_slice().to_vec();let dst=pixmap.data_as_u8_slice_mut();dst.fill(0);for y in 0..h{for x in 0..w{let nx=x as i32+dx;let ny=y as i32+dy;if nx>=0&&ny>=0&&nx<w as i32&&ny<h as i32{let si=(y*w+x)*4;let di=(ny as usize*w+nx as usize)*4;dst[di..di+4].copy_from_slice(&src[si..si+4]);}}}pixmap.recompute_may_have_transparency();}

fn rasterize_records(records:&[VectorRecord],width:u16,height:u16)->Result<Pixmap,String>{let mut context=RenderContext::new(width,height);for record in records{context.set_fill_rule(Fill::NonZero);set_paint(&mut context,&record.paint)?;context.fill_path(&parse_path(&record.path.svg)?);}context.flush();let mut resources=Resources::new();let mut pixmap=Pixmap::new(width,height);context.render(&mut pixmap,&mut resources);Ok(pixmap)}
fn rasterize_pattern_records(records:&[VectorRecord],tile_width:f64,tile_height:f64)->Result<(Pixmap,u16,u16),String>{if !tile_width.is_finite()||!tile_height.is_finite()||tile_width<=0.0||tile_height<=0.0{return Err("native pattern tile dimensions must be positive finite numbers".into());}let width=tile_width.ceil().clamp(1.0,512.0)as u16;let height=tile_height.ceil().clamp(1.0,512.0)as u16;let mut context=RenderContext::new(width,height);for record in records{context.set_fill_rule(Fill::NonZero);set_paint(&mut context,&record.paint)?;context.fill_path(&parse_path(&record.path.svg)?);}context.flush();let mut resources=Resources::new();let mut pixmap=Pixmap::new(width,height);context.render(&mut pixmap,&mut resources);Ok((pixmap,width,height))}
fn apply_alpha_mask(layer:&mut Pixmap,mask:&Pixmap)->Result<(),String>{let mask_bytes=mask.data_as_u8_slice();let layer_bytes=layer.data_as_u8_slice_mut();if mask_bytes.len()!=layer_bytes.len(){return Err("native mask dimensions do not match isolated layer".into());}for(pixel,mask_pixel)in layer_bytes.chunks_exact_mut(4).zip(mask_bytes.chunks_exact(4)){let alpha=u16::from(mask_pixel[3]);for channel in pixel{*channel=((u16::from(*channel)*alpha+127)/255)as u8;}}layer.recompute_may_have_transparency();Ok(())}
fn composite_pixmap(context:&mut RenderContext,pixmap:Pixmap,width:u16,height:u16)->Result<(),String>{let image=Image{image:ImageSource::Pixmap(Arc::new(pixmap)),sampler:ImageSampler::default()};context.reset_paint_transform();context.set_paint(image);context.set_fill_rule(Fill::NonZero);context.fill_path(&parse_path(&format!("M 0 0 H {width} V {height} H 0 Z"))?);Ok(())}
fn to_fill(rule:FillRule)->Fill{match rule{FillRule::NonZero=>Fill::NonZero,FillRule::EvenOdd=>Fill::EvenOdd}}
fn cap(value:LineCap)->Cap{match value{LineCap::Butt=>Cap::Butt,LineCap::Round=>Cap::Round,LineCap::Square=>Cap::Square}}
fn join(value:LineJoin)->Join{match value{LineJoin::Miter=>Join::Miter,LineJoin::Round=>Join::Round,LineJoin::Bevel=>Join::Bevel}}
fn parse_path(value:&str)->Result<BezPath,String>{BezPath::from_svg(value).map_err(|e|format!("invalid path: {e:?}"))}
fn color(value:Rgba)->AlphaColor<Srgb>{AlphaColor::<Srgb>::from_rgba8(value.r,value.g,value.b,value.a)}
fn extend(spread:GradientSpread)->Extend{match spread{GradientSpread::Pad=>Extend::Pad,GradientSpread::Repeat=>Extend::Repeat,GradientSpread::Reflect=>Extend::Reflect}}

fn set_paint(context:&mut RenderContext,paint:&Paint)->Result<(),String>{context.reset_paint_transform();match paint{
    Paint::Solid(value)=>context.set_paint(color(*value)),
    Paint::LinearGradient{start,end,stops,spread}=>{let stops=ColorStops(stops.iter().map(|s|ColorStop{offset:s.offset,color:color(s.color).into()}).collect());context.set_paint(Gradient::new_linear(*start,*end).with_stops(stops).with_extend(extend(*spread)));}
    Paint::RadialGradient{center,focal,focal_radius,radius,stops,spread}=>{let stops=ColorStops(stops.iter().map(|s|ColorStop{offset:s.offset,color:color(s.color).into()}).collect());context.set_paint(Gradient::new_two_point_radial(*focal,*focal_radius as f32,*center,*radius as f32).with_stops(stops).with_extend(extend(*spread)));}
    Paint::Pattern{records,tile_width,tile_height}=>{let(pixmap,rw,rh)=rasterize_pattern_records(records,*tile_width,*tile_height)?;let image=Image{image:ImageSource::Pixmap(Arc::new(pixmap)),sampler:ImageSampler{x_extend:Extend::Repeat,y_extend:Extend::Repeat,..Default::default()}};context.set_paint(image);context.set_paint_transform(Affine::scale_non_uniform(*tile_width/f64::from(rw),*tile_height/f64::from(rh)));}
    Paint::SvgPattern{svg,tile_width,tile_height}=>{let(pixmap,rw,rh)=rasterize_pattern(svg,*tile_width,*tile_height)?;let image=Image{image:ImageSource::Pixmap(Arc::new(pixmap)),sampler:ImageSampler{x_extend:Extend::Repeat,y_extend:Extend::Repeat,..Default::default()}};context.set_paint(image);context.set_paint_transform(Affine::scale_non_uniform(*tile_width/f64::from(rw),*tile_height/f64::from(rh)));}
}Ok(())}

fn svg_options(svg:&str)->resvg::usvg::Options<'static>{let mut options=resvg::usvg::Options::default();if svg.contains("<text")||svg.contains("<tspan"){options.fontdb_mut().load_system_fonts();}options}
fn rasterize_pattern(svg:&str,tile_width:f64,tile_height:f64)->Result<(Pixmap,u16,u16),String>{use resvg::{tiny_skia,usvg};if !tile_width.is_finite()||!tile_height.is_finite()||tile_width<=0.0||tile_height<=0.0{return Err("SVG pattern tile dimensions must be positive finite numbers".into());}let options=svg_options(svg);let tree=usvg::Tree::from_str(svg,&options).map_err(|e|format!("could not parse legacy SVG pattern: {e}"))?;let size=tree.size();if size.width()<=0.0||size.height()<=0.0{return Err("SVG pattern has an empty tile viewport".into());}let width=(size.width().max(1.0)).ceil().clamp(1.0,512.0)as u32;let height=(size.height().max(1.0)).ceil().clamp(1.0,512.0)as u32;let rw=u16::try_from(width).map_err(|_|"SVG pattern raster width is too large".to_string())?;let rh=u16::try_from(height).map_err(|_|"SVG pattern raster height is too large".to_string())?;let mut source=tiny_skia::Pixmap::new(width,height).ok_or_else(||"could not allocate SVG pattern tile".to_string())?;resvg::render(&tree,tiny_skia::Transform::from_scale(width as f32/size.width(),height as f32/size.height()),&mut source.as_mut());let mut pixmap=Pixmap::new(rw,rh);pixmap.data_as_u8_slice_mut().copy_from_slice(source.data());pixmap.recompute_may_have_transparency();Ok((pixmap,rw,rh))}

#[cfg(test)]mod tests{
use super::*;use crate::{GradientStop,PathData};
#[test]fn base64_decodes_png_signature(){assert_eq!(decode_base64("iVBORw0KGgo=").unwrap(),vec![137,80,78,71,13,10,26,10]);}
#[test]fn image_fit_meet_centers(){assert_eq!(fit_image(0.0,0.0,100.0,100.0,200.0,100.0,"xMidYMid meet"),(0.0,25.0,100.0,50.0));}
#[test]fn blur_changes_impulse(){let mut p=Pixmap::new(5,1);p.data_as_u8_slice_mut()[2*4+3]=255;gaussian_blur(&mut p,1.0,0.0,5,1);assert!(p.data_as_u8_slice()[1*4+3]>0);}
#[test]fn native_radial_gradient_renders(){let scene=PreparedScene{width:16,height:16,revision:1,diagnostics:vec![],commands:vec![Command::Fill{path:PathData{svg:"M 0 0 H 16 V 16 H 0 Z".into()},paint:Paint::RadialGradient{center:(8.0,8.0),focal:(8.0,8.0),focal_radius:0.0,radius:8.0,stops:vec![GradientStop{offset:0.0,color:Rgba{r:255,g:0,b:0,a:255}},GradientStop{offset:1.0,color:Rgba{r:0,g:0,b:255,a:255}}],spread:GradientSpread::Pad},rule:FillRule::NonZero}]};let output=render(&scene);assert!(!output.diagnostics.iter().any(|d|d.severity=="error"));}
}
