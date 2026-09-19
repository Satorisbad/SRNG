use crate::{Command, EmbeddedImage, FillRule, FilterOp, GradientSpread, LineCap, LineJoin, Paint, PreparedScene, RenderDiagnostic, Rgba, VectorRecord};
use std::sync::Arc;
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
    let iw=u16::try_from(image.intrinsic_width).map_err(|_|"embedded image is too wide for CPU pixmap".to_string())?;
    let ih=u16::try_from(image.intrinsic_height).map_err(|_|"embedded image is too tall for CPU pixmap".to_string())?;
    let expected=usize::from(iw).checked_mul(usize::from(ih)).and_then(|n|n.checked_mul(4)).ok_or_else(||"embedded image byte size overflow".to_string())?;
    if image.pixels.len()!=expected{return Err("embedded image RGBA buffer length does not match intrinsic dimensions".into());}
    let mut source=Pixmap::new(iw,ih);source.data_as_u8_slice_mut().copy_from_slice(&image.pixels);source.recompute_may_have_transparency();
    let sw=f64::from(image.intrinsic_width);let sh=f64::from(image.intrinsic_height);
    let (x,y,w,h)=fit_image(image.x,image.y,image.width,image.height,sw,sh,&image.preserve_aspect_ratio);
    let sampler=ImageSampler::default();let paint=Image{image:ImageSource::Pixmap(Arc::new(source)),sampler};context.reset_paint_transform();context.set_paint(paint);context.set_paint_transform(Affine::translate((x,y))*Affine::scale_non_uniform(w/sw,h/sh));context.set_fill_rule(Fill::NonZero);context.fill_path(&parse_path(&format!("M 0 0 H {sw} V {sh} H 0 Z"))?);context.reset_paint_transform();Ok(())
}

fn fit_image(x:f64,y:f64,w:f64,h:f64,sw:f64,sh:f64,preserve:&str)->(f64,f64,f64,f64){if preserve.trim().starts_with("none"){return(x,y,w,h);}let slice=preserve.contains("slice");let scale=if slice{(w/sw).max(h/sh)}else{(w/sw).min(h/sh)};let rw=sw*scale;let rh=sh*scale;let dx=if preserve.contains("xMin"){0.0}else if preserve.contains("xMax"){w-rw}else{(w-rw)/2.0};let dy=if preserve.contains("YMin"){0.0}else if preserve.contains("YMax"){h-rh}else{(h-rh)/2.0};(x+dx,y+dy,rw,rh)}

fn apply_filters(pixmap:&mut Pixmap,filters:&[FilterOp],width:u16,height:u16)->Result<(),String>{for filter in filters{match *filter{FilterOp::GaussianBlur{sigma_x,sigma_y}=>gaussian_blur(pixmap,sigma_x,sigma_y,width,height),FilterOp::Offset{dx,dy}=>offset_pixmap(pixmap,dx.round()as i32,dy.round()as i32,width,height)}}Ok(())}
fn gaussian_blur(pixmap:&mut Pixmap,sigma_x:f64,sigma_y:f64,width:u16,height:u16){let rx=(sigma_x*3.0).ceil().clamp(0.0,64.0)as i32;let ry=(sigma_y*3.0).ceil().clamp(0.0,64.0)as i32;if rx==0&&ry==0{return;}let mut data=pixmap.data_as_u8_slice().to_vec();if rx>0{data=blur_axis(&data,usize::from(width),usize::from(height),rx,true);}if ry>0{data=blur_axis(&data,usize::from(width),usize::from(height),ry,false);}pixmap.data_as_u8_slice_mut().copy_from_slice(&data);pixmap.recompute_may_have_transparency();}
fn blur_axis(src:&[u8],w:usize,h:usize,r:i32,horizontal:bool)->Vec<u8>{let sigma=(r as f64/3.0).max(0.333);let mut weights=Vec::with_capacity((r*2+1)as usize);let mut sum=0.0;for i in -r..=r{let v=(-((i*i)as f64)/(2.0*sigma*sigma)).exp();weights.push(v);sum+=v;}for v in &mut weights{*v/=sum;}let mut out=vec![0;src.len()];for y in 0..h{for x in 0..w{for c in 0..4{let mut acc=0.0;for(i,&weight)in(-r..=r).zip(weights.iter()){let(nx,ny)=if horizontal{((x as i32+i).clamp(0,w as i32-1)as usize,y)}else{(x,(y as i32+i).clamp(0,h as i32-1)as usize)};acc+=f64::from(src[(ny*w+nx)*4+c])*weight;}out[(y*w+x)*4+c]=acc.round().clamp(0.0,255.0)as u8;}}}out}
fn offset_pixmap(pixmap:&mut Pixmap,dx:i32,dy:i32,width:u16,height:u16){let w=usize::from(width);let h=usize::from(height);let src=pixmap.data_as_u8_slice().to_vec();let dst=pixmap.data_as_u8_slice_mut();dst.fill(0);for y in 0..h{for x in 0..w{let nx=x as i32+dx;let ny=y as i32+dy;if nx>=0&&ny>=0&&nx<w as i32&&ny<h as i32{let si=(y*w+x)*4;let di=(ny as usize*w+nx as usize)*4;dst[di..di+4].copy_from_slice(&src[si..si+4]);}}}pixmap.recompute_may_have_transparency();}

fn rasterize_records(records:&[VectorRecord],width:u16,height:u16)->Result<Pixmap,String>{let mut context=RenderContext::new(width,height);for record in records{context.set_fill_rule(Fill::NonZero);set_paint(&mut context,&record.paint)?;context.fill_path(&parse_path(&record.path.svg)?);}context.flush();let mut resources=Resources::new();let mut pixmap=Pixmap::new(width,height);context.render(&mut pixmap,&mut resources);Ok(pixmap)}
fn rasterize_pattern_records(records:&[VectorRecord],tile_width:f64,tile_height:f64)->Result<(Pixmap,u16,u16),String>{if !tile_width.is_finite()||!tile_height.is_finite()||tile_width<=0.0||tile_height<=0.0{return Err("native pattern tile dimensions must be positive finite numbers".into());}let width=tile_width.ceil().clamp(1.0,512.0)as u16;let height=tile_height.ceil().clamp(1.0,512.0)as u16;let mut context=RenderContext::new(width,height);for record in records{context.set_fill_rule(Fill::NonZero);set_paint(&mut context,&record.paint)?;context.fill_path(&parse_path(&record.path.svg)?);}context.flush();let mut resources=Resources::new();let mut pixmap=Pixmap::new(width,height);context.render(&mut pixmap,&mut resources);Ok((pixmap,width,height))}
fn apply_alpha_mask(layer:&mut Pixmap,mask:&Pixmap)->Result<(),String>{let mask_bytes=mask.data_as_u8_slice();let layer_bytes=layer.data_as_u8_slice_mut();if mask_bytes.len()!=layer_bytes.len(){return Err("native mask dimensions do not match isolated layer".into());}for(pixel,mask_pixel)in layer_bytes.chunks_exact_mut(4).zip(mask_bytes.chunks_exact(4)){let alpha=u16::from(mask_pixel[3]);for channel in pixel{*channel=((u16::from(*channel)*alpha+127)/255)as u8;}}layer.recompute_may_have_transparency();Ok(())}
fn composite_pixmap(context:&mut RenderContext,pixmap:Pixmap,width:u16,height:u16)->Result<(),String>{let paint=Image{image:ImageSource::Pixmap(Arc::new(pixmap)),sampler:ImageSampler::default()};context.reset_paint_transform();context.set_paint(paint);context.fill_path(&parse_path(&format!("M 0 0 H {width} V {height} H 0 Z"))?);Ok(())}
fn parse_path(value:&str)->Result<BezPath,String>{BezPath::from_svg(value).map_err(|e|format!("invalid path: {e:?}"))}
fn to_fill(rule:FillRule)->Fill{match rule{FillRule::NonZero=>Fill::NonZero,FillRule::EvenOdd=>Fill::EvenOdd}}
fn cap(value:LineCap)->Cap{match value{LineCap::Butt=>Cap::Butt,LineCap::Round=>Cap::Round,LineCap::Square=>Cap::Square}}
fn join(value:LineJoin)->Join{match value{LineJoin::Miter=>Join::Miter,LineJoin::Round=>Join::Round,LineJoin::Bevel=>Join::Bevel}}
fn color(value:Rgba)->AlphaColor<Srgb>{AlphaColor::<Srgb>::from_rgba8(value.r,value.g,value.b,value.a)}
fn stops(values:&[crate::GradientStop])->ColorStops{ColorStops(values.iter().map(|s|ColorStop{offset:s.offset,color:color(s.color).into()}).collect())}
fn extend(spread:GradientSpread)->Extend{match spread{GradientSpread::Pad=>Extend::Pad,GradientSpread::Repeat=>Extend::Repeat,GradientSpread::Reflect=>Extend::Reflect}}
fn set_paint(context:&mut RenderContext,paint:&Paint)->Result<(),String>{match paint{Paint::Solid(v)=>context.set_paint(color(*v)),Paint::LinearGradient{start,end,stops:values,spread}=>context.set_paint(Gradient::new_linear(*start,*end).with_stops(stops(values)).with_extend(extend(*spread))),Paint::RadialGradient{center,focal,focal_radius,radius,stops:values,spread}=>context.set_paint(Gradient::new_two_point_radial(*focal,*focal_radius as f32,*center,*radius as f32).with_stops(stops(values)).with_extend(extend(*spread))),Paint::Pattern{records,tile_width,tile_height}=>{let(tile,w,h)=rasterize_pattern_records(records,*tile_width,*tile_height)?;context.set_paint(Image{image:ImageSource::Pixmap(Arc::new(tile)),sampler:ImageSampler::default()});context.set_paint_transform(Affine::scale_non_uniform(*tile_width/f64::from(w),*tile_height/f64::from(h)));},Paint::SvgPattern{..}=>return Err("legacy SVG patterns are not executable in native CPU paint path".into())}Ok(())}
