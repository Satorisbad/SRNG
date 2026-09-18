use crate::{Command, EmbeddedImage, FillRule, FilterOp, GradientSpread, LineCap, LineJoin, Paint, PreparedScene, RenderDiagnostic, Rgba, VectorRecord};
use kurbo::{Affine, BezPath, Cap, Join, Shape, Stroke as KurboStroke};
use peniko::{color::{AlphaColor, Srgb}, ColorStop, ColorStops, Extend, Fill, Gradient, ImageQuality};
use std::{collections::HashMap, fmt, io::Cursor};
use vello_common::geometry::RectU16;
use vello_hybrid::{RenderSize, RenderTargetConfig, Renderer as HybridRenderer, Resources, SampleRect, Scene as HybridScene, TextureBindings, TextureId};
use wgpu::{CommandEncoder, Device, Extent3d, Origin3d, Queue, TexelCopyBufferLayout, TexelCopyTextureInfo, Texture, TextureAspect, TextureDescriptor, TextureDimension, TextureFormat, TextureUsages, TextureView, TextureViewDescriptor};

#[derive(Debug)]
pub enum GpuRenderError { Scene(Vec<RenderDiagnostic>), Backend(String) }
impl fmt::Display for GpuRenderError { fn fmt(&self, f:&mut fmt::Formatter<'_>)->fmt::Result { match self { Self::Scene(d)=>write!(f,"scene preparation produced {} renderer diagnostic(s)",d.len()), Self::Backend(m)=>write!(f,"GPU renderer error: {m}") } } }
impl std::error::Error for GpuRenderError {}

#[derive(Debug)]
pub struct GpuTexture { _texture: Texture, view: TextureView, width:u32, height:u32 }
impl GpuTexture {
    pub fn new_offscreen(device:&Device,label:&str,width:u32,height:u32)->Result<Self,String>{
        let width=width.max(1); let height=height.max(1);
        let texture=device.create_texture(&TextureDescriptor{label:Some(label),size:Extent3d{width,height,depth_or_array_layers:1},mip_level_count:1,sample_count:1,dimension:TextureDimension::D2,format:TextureFormat::Rgba8Unorm,usage:TextureUsages::TEXTURE_BINDING|TextureUsages::COPY_DST|TextureUsages::RENDER_ATTACHMENT,view_formats:&[]});
        let view=texture.create_view(&TextureViewDescriptor::default()); Ok(Self{_texture:texture,view,width,height})
    }
    fn new_rgba8(device:&Device, queue:&Queue, label:&str, width:u32, height:u32, pixels:&[u8])->Result<Self,String> {
        let width=width.max(1); let height=height.max(1); let expected=(width as usize).checked_mul(height as usize).and_then(|n|n.checked_mul(4)).ok_or_else(||"texture dimensions overflow".to_string())?;
        if pixels.len()!=expected { return Err(format!("texture `{label}` has {} bytes, expected {expected}",pixels.len())); }
        let texture=Self::new_offscreen(device,label,width,height)?;
        queue.write_texture(TexelCopyTextureInfo{texture:&texture._texture,mip_level:0,origin:Origin3d::ZERO,aspect:TextureAspect::All},pixels,TexelCopyBufferLayout{offset:0,bytes_per_row:Some(width*4),rows_per_image:Some(height)},Extent3d{width,height,depth_or_array_layers:1});
        Ok(texture)
    }
    pub fn view(&self)->&TextureView { &self.view }
    pub fn width(&self)->u32 { self.width }
    pub fn height(&self)->u32 { self.height }
    fn binding_view(&self)->TextureView { self.view.clone() }
}

#[derive(Clone,Copy,Debug)]
struct TextureHandle { id:TextureId, width:u16, height:u16 }

#[derive(Debug, Default)]
pub struct GpuResourceStore { next_texture_id:u64, textures:HashMap<TextureId,GpuTexture> }
impl GpuResourceStore {
    fn insert(&mut self, texture:GpuTexture)->Result<TextureHandle,String> { let width=u16::try_from(texture.width).map_err(|_|"GPU texture width exceeds vello_hybrid u16 limits".to_string())?; let height=u16::try_from(texture.height).map_err(|_|"GPU texture height exceeds vello_hybrid u16 limits".to_string())?; let id=TextureId(self.next_texture_id); self.next_texture_id=self.next_texture_id.wrapping_add(1); self.textures.insert(id,texture); Ok(TextureHandle{id,width,height}) }
    fn bindings(&self)->TextureBindings { let mut bindings=TextureBindings::new(); for (id,texture) in &self.textures { bindings.insert(*id,texture.binding_view()); } bindings }
    pub fn clear(&mut self){ self.textures.clear(); }
    pub fn len(&self)->usize{ self.textures.len() }
}

#[derive(Debug, Clone)]
pub struct FilterExecutionRequest { pub operations:Vec<FilterOp>, pub width:u32, pub height:u32 }
pub trait GpuFilterExecutor { fn execute(&mut self, request:&FilterExecutionRequest, source:&GpuTexture, device:&Device, queue:&Queue, encoder:&mut CommandEncoder)->Result<GpuTexture,String>; }

#[derive(Debug)]
pub struct GpuRenderer { renderer:HybridRenderer, resources:Resources, target_width:u32, target_height:u32, target_format:TextureFormat, resource_store:GpuResourceStore }
impl GpuRenderer {
    pub fn new(device:&Device,target_format:TextureFormat,target_width:u32,target_height:u32)->Self { let config=RenderTargetConfig{format:target_format,width:target_width.max(1),height:target_height.max(1)}; let(renderer,resources)=HybridRenderer::new(device,&config); Self{renderer,resources,target_width:config.width,target_height:config.height,target_format,resource_store:GpuResourceStore::default()} }
    pub fn target_format(&self)->TextureFormat{self.target_format}
    pub fn target_size(&self)->(u32,u32){(self.target_width,self.target_height)}
    pub fn resource_count(&self)->usize{self.resource_store.len()}
    pub fn render_to_view(&mut self,scene:&PreparedScene,device:&Device,queue:&Queue,encoder:&mut CommandEncoder,view:&TextureView)->Result<(),GpuRenderError>{
        let width=u32::from(scene.width); let height=u32::from(scene.height); if width>self.target_width||height>self.target_height{return Err(GpuRenderError::Backend(format!("prepared scene {width}x{height} exceeds renderer target {}x{}",self.target_width,self.target_height)));}
        self.resource_store.clear(); let mut diagnostics=scene.diagnostics.clone(); let hybrid_scene=build_scene_with_resources(scene,device,queue,&mut self.resource_store,&mut diagnostics).map_err(|m|{diagnostics.push(diag("G600",m));GpuRenderError::Scene(diagnostics.clone())})?;
        if diagnostics.iter().any(|d|d.severity=="error"){return Err(GpuRenderError::Scene(diagnostics));}
        let render_size=RenderSize{width,height}; let bindings=self.resource_store.bindings(); self.renderer.render(&hybrid_scene,&mut self.resources,device,queue,encoder,&render_size,view,&bindings).map_err(|e|GpuRenderError::Backend(e.to_string()))
    }
}

pub fn build_scene(scene:&PreparedScene)->Result<HybridScene,Vec<RenderDiagnostic>> {
    let mut output=HybridScene::new(scene.width,scene.height); let mut diagnostics=scene.diagnostics.clone();
    for command in &scene.commands { if let Err(message)=apply_vector_only(&mut output,command){diagnostics.push(diag("G600",message));} }
    if diagnostics.iter().any(|d|d.severity=="error"){Err(diagnostics)}else{Ok(output)}
}

fn build_scene_with_resources(scene:&PreparedScene,device:&Device,queue:&Queue,resources:&mut GpuResourceStore,diagnostics:&mut Vec<RenderDiagnostic>)->Result<HybridScene,String>{
    let mut output=HybridScene::new(scene.width,scene.height); render_range(&mut output,&scene.commands,device,queue,resources,diagnostics,scene.width,scene.height)?; Ok(output)
}

fn render_range(context:&mut HybridScene,commands:&[Command],device:&Device,queue:&Queue,resources:&mut GpuResourceStore,diagnostics:&mut Vec<RenderDiagnostic>,width:u16,height:u16)->Result<(),String>{
    let mut i=0usize;
    while i<commands.len(){ match &commands[i] {
        Command::PushMask{records}=>{ let end=matching_end(commands,i,|c|matches!(c,Command::PushMask{..}),|c|matches!(c,Command::PopMask),"mask")?; render_masked_block(context,&commands[i+1..end],records,device,queue,resources,diagnostics,width,height)?; i=end+1; }
        Command::PushFilter{filters}=>{ let end=matching_end(commands,i,|c|matches!(c,Command::PushFilter{..}),|c|matches!(c,Command::PopFilter),"filter")?; diagnostics.push(RenderDiagnostic{severity:"warning".into(),code:"G630".into(),message:format!("GPU filter execution boundary received {} operation(s); native FilterGraph execution is delegated to filter-graph-v0.6, rendering unfiltered content deterministically",filters.len()),declaration:None}); render_range(context,&commands[i+1..end],device,queue,resources,diagnostics,width,height)?; i=end+1; }
        Command::PopMask=>return Err("encountered unmatched mask terminator".into()), Command::PopFilter=>return Err("encountered unmatched filter terminator".into()),
        command=>{apply_gpu(context,command,device,queue,resources,width,height)?; i+=1;}
    }} Ok(())
}

fn render_masked_block(context:&mut HybridScene,commands:&[Command],records:&[VectorRecord],device:&Device,queue:&Queue,resources:&mut GpuResourceStore,diagnostics:&mut Vec<RenderDiagnostic>,width:u16,height:u16)->Result<(),String>{
    let layer=render_cpu_rgba(commands,width,height)?; let mask=render_mask_rgba(records,width,height)?; let mut composited=layer;
    for (dst,m) in composited.chunks_exact_mut(4).zip(mask.chunks_exact(4)){let a=u16::from(m[3]); for c in dst.iter_mut(){*c=((u16::from(*c)*a+127)/255)as u8;}}
    let handle=resources.insert(GpuTexture::new_rgba8(device,queue,"srng-mask-layer",u32::from(width),u32::from(height),&composited)?)?; draw_texture_fullscreen(context,handle); diagnostics.push(RenderDiagnostic{severity:"info".into(),code:"G611".into(),message:"GPU mask compositing used a deterministic isolated-layer upload; no masked command was dropped".into(),declaration:None}); Ok(())
}

fn apply_gpu(context:&mut HybridScene,command:&Command,device:&Device,queue:&Queue,resources:&mut GpuResourceStore,width:u16,height:u16)->Result<(),String>{ match command {
    Command::PushClip{path,rule}=>{context.set_fill_rule(to_fill(*rule));context.push_clip_path(&parse_path(&path.svg)?);}, Command::PopClip=>context.pop_clip_path(),
    Command::PushMask{..}|Command::PopMask|Command::PushFilter{..}|Command::PopFilter=>return Err("layer commands must be handled as blocks".into()),
    Command::DrawImage{image}=>draw_image(context,image,device,queue,resources)?,
    Command::Fill{path,paint,rule}=>{context.set_fill_rule(to_fill(*rule)); if let Some(texture)=texture_paint(paint,device,queue,resources)?{fill_path_with_texture(context,&parse_path(&path.svg)?,texture);}else{set_vector_paint(context,paint)?;context.fill_path(&parse_path(&path.svg)?);}},
    Command::Stroke{path,paint,style}=>{
        if matches!(paint,Paint::Pattern{..}|Paint::SvgPattern{..}) {
            let pixels=render_cpu_rgba(std::slice::from_ref(command),width,height)?;
            let handle=resources.insert(GpuTexture::new_rgba8(device,queue,"srng-pattern-stroke-fallback",u32::from(width),u32::from(height),&pixels)?)?;
            draw_texture_fullscreen(context,handle);
        } else {
            set_vector_paint(context,paint)?; let stroke=KurboStroke::new(style.width).with_miter_limit(style.miter_limit).with_caps(cap(style.line_cap)).with_join(join(style.line_join)).with_dashes(style.dash_offset,style.dash.iter()); context.set_stroke(stroke); context.stroke_path(&parse_path(&path.svg)?);
        }
    }
    } Ok(()) }

fn apply_vector_only(context:&mut HybridScene,command:&Command)->Result<(),String>{match command{
    Command::PushClip{path,rule}=>{context.set_fill_rule(to_fill(*rule));context.push_clip_path(&parse_path(&path.svg)?);},Command::PopClip=>context.pop_clip_path(),
    Command::PushMask{..}|Command::PopMask=>return Err("native masks require GpuRenderer::render_to_view resource execution".into()),
    Command::PushFilter{..}|Command::PopFilter=>return Err("native filters require GpuRenderer::render_to_view offscreen execution boundary".into()),
    Command::DrawImage{..}=>return Err("embedded images require GpuRenderer::render_to_view texture upload execution".into()),
    Command::Fill{path,paint,rule}=>{context.set_fill_rule(to_fill(*rule));set_vector_paint(context,paint)?;context.fill_path(&parse_path(&path.svg)?);},
    Command::Stroke{path,paint,style}=>{set_vector_paint(context,paint)?;let stroke=KurboStroke::new(style.width).with_miter_limit(style.miter_limit).with_caps(cap(style.line_cap)).with_join(join(style.line_join)).with_dashes(style.dash_offset,style.dash.iter());context.set_stroke(stroke);context.stroke_path(&parse_path(&path.svg)?);}}
    Ok(())}

fn draw_image(context:&mut HybridScene,image:&EmbeddedImage,device:&Device,queue:&Queue,resources:&mut GpuResourceStore)->Result<(),String>{let(decoded,sw,sh)=decode_data_image(&image.href)?;let handle=resources.insert(GpuTexture::new_rgba8(device,queue,"srng-embedded-image",sw,sh,&decoded)?)?;let(x,y,w,h)=fit_image(image.x,image.y,image.width,image.height,f64::from(sw),f64::from(sh),&image.preserve_aspect_ratio);let source=RectU16::new(0,0,handle.width,handle.height);context.draw_texture_rects(handle.id,ImageQuality::Medium,[SampleRect{source_region:source,transform:Affine::translate((x,y))*Affine::scale_non_uniform(w/f64::from(sw),h/f64::from(sh))}]);Ok(())}

fn texture_paint(paint:&Paint,device:&Device,queue:&Queue,resources:&mut GpuResourceStore)->Result<Option<TextureHandle>,String>{match paint{
    Paint::Pattern{records,tile_width,tile_height}=>{let(rgba,w,h)=rasterize_pattern_records(records,*tile_width,*tile_height)?;Ok(Some(resources.insert(GpuTexture::new_rgba8(device,queue,"srng-vector-pattern",w,h,&rgba)?)?))},
    Paint::SvgPattern{svg,tile_width,tile_height}=>{let(rgba,w,h)=rasterize_svg(svg,*tile_width,*tile_height)?;Ok(Some(resources.insert(GpuTexture::new_rgba8(device,queue,"srng-raster-pattern",w,h,&rgba)?)?))},
    _=>Ok(None)}}

fn fill_path_with_texture(context:&mut HybridScene,path:&BezPath,texture:TextureHandle){
    let bounds=path.bounding_box(); let tile_w=f64::from(texture.width.max(1)); let tile_h=f64::from(texture.height.max(1)); let start_x=(bounds.x0/tile_w).floor()*tile_w; let start_y=(bounds.y0/tile_h).floor()*tile_h; let end_x=bounds.x1.ceil(); let end_y=bounds.y1.ceil(); let source=RectU16::new(0,0,texture.width,texture.height); let mut rects=Vec::new(); let mut y=start_y;
    while y<end_y { let mut x=start_x; while x<end_x { rects.push(SampleRect{source_region:source,transform:Affine::translate((x,y))}); x+=tile_w; } y+=tile_h; }
    context.push_clip_path(path); context.draw_texture_rects(texture.id,ImageQuality::Medium,rects); context.pop_clip_path();
}
fn draw_texture_fullscreen(context:&mut HybridScene,texture:TextureHandle){let source=RectU16::new(0,0,texture.width,texture.height);context.draw_texture_rects(texture.id,ImageQuality::Medium,[SampleRect{source_region:source,transform:Affine::IDENTITY}]);}

fn set_vector_paint(context:&mut HybridScene,paint:&Paint)->Result<(),String>{match paint{Paint::Solid(v)=>context.set_paint(color(*v)),Paint::LinearGradient{start,end,stops:values,spread}=>context.set_paint(Gradient::new_linear(*start,*end).with_stops(stops(values)).with_extend(extend(*spread))),Paint::RadialGradient{center,focal,focal_radius,radius,stops:values,spread}=>context.set_paint(Gradient::new_two_point_radial(*focal,*focal_radius as f32,*center,*radius as f32).with_stops(stops(values)).with_extend(extend(*spread))),Paint::Pattern{..}|Paint::SvgPattern{..}=>return Err("texture-backed paint requires GPU resource execution".into())}Ok(())}

fn render_cpu_rgba(commands:&[Command],width:u16,height:u16)->Result<Vec<u8>,String>{let scene=PreparedScene{width,height,revision:0,commands:commands.to_vec(),diagnostics:Vec::new()};let out=crate::cpu::render(&scene);if out.diagnostics.iter().any(|d|d.severity=="error"){return Err(out.diagnostics.into_iter().map(|d|d.message).collect::<Vec<_>>().join("; "));}Ok(out.pixels)}
fn render_mask_rgba(records:&[VectorRecord],width:u16,height:u16)->Result<Vec<u8>,String>{let mut commands=Vec::new();for record in records{commands.push(Command::Fill{path:record.path.clone(),paint:record.paint.clone(),rule:FillRule::NonZero});}render_cpu_rgba(&commands,width,height)}

fn rasterize_pattern_records(records:&[VectorRecord],tile_width:f64,tile_height:f64)->Result<(Vec<u8>,u32,u32),String>{if !tile_width.is_finite()||!tile_height.is_finite()||tile_width<=0.0||tile_height<=0.0{return Err("native pattern tile dimensions must be positive finite numbers".into());}let width=tile_width.ceil().clamp(1.0,512.0)as u16;let height=tile_height.ceil().clamp(1.0,512.0)as u16;let mut commands=Vec::new();for record in records{commands.push(Command::Fill{path:record.path.clone(),paint:record.paint.clone(),rule:FillRule::NonZero});}let pixels=render_cpu_rgba(&commands,width,height)?;Ok((pixels,u32::from(width),u32::from(height)))}

fn rasterize_svg(svg:&str,tile_width:f64,tile_height:f64)->Result<(Vec<u8>,u32,u32),String>{use resvg::{tiny_skia,usvg};let options=usvg::Options::default();let tree=usvg::Tree::from_str(svg,&options).map_err(|e|format!("invalid SVG pattern: {e}"))?;let width=tile_width.ceil().clamp(1.0,4096.0)as u32;let height=tile_height.ceil().clamp(1.0,4096.0)as u32;let mut pixmap=tiny_skia::Pixmap::new(width,height).ok_or_else(||"could not allocate SVG pattern pixmap".to_string())?;let size=tree.size();resvg::render(&tree,tiny_skia::Transform::from_scale(width as f32/size.width(),height as f32/size.height()),&mut pixmap.as_mut());Ok((pixmap.data().to_vec(),width,height))}
fn rasterize_svg_image(svg:&str)->Result<(Vec<u8>,u32,u32),String>{use resvg::{tiny_skia,usvg};let options=usvg::Options::default();let tree=usvg::Tree::from_str(svg,&options).map_err(|e|format!("invalid embedded SVG: {e}"))?;let size=tree.size();let width=size.width().ceil().clamp(1.0,4096.0)as u32;let height=size.height().ceil().clamp(1.0,4096.0)as u32;let mut pixmap=tiny_skia::Pixmap::new(width,height).ok_or_else(||"could not allocate embedded SVG pixmap".to_string())?;resvg::render(&tree,tiny_skia::Transform::from_scale(width as f32/size.width(),height as f32/size.height()),&mut pixmap.as_mut());Ok((pixmap.data().to_vec(),width,height))}

fn decode_data_image(uri:&str)->Result<(Vec<u8>,u32,u32),String>{let(rest,payload)=uri.split_once(',').ok_or_else(||"invalid data image URI".to_string())?;let mime=rest.strip_prefix("data:").unwrap_or(rest).split(';').next().unwrap_or("");let bytes=if rest.contains(";base64"){decode_base64(payload)?}else{percent_decode(payload)?};match mime{"image/png"=>decode_png(&bytes),"image/svg+xml"=>{let text=std::str::from_utf8(&bytes).map_err(|_|"embedded SVG is not UTF-8".to_string())?;rasterize_svg_image(text)},other=>Err(format!("unsupported embedded image type `{other}`; GPU backend supports PNG and SVG data images"))}}
fn decode_png(bytes:&[u8])->Result<(Vec<u8>,u32,u32),String>{let decoder=png::Decoder::new(Cursor::new(bytes));let mut reader=decoder.read_info().map_err(|e|format!("invalid embedded PNG: {e}"))?;let mut buf=vec![0;reader.output_buffer_size()];let info=reader.next_frame(&mut buf).map_err(|e|format!("could not decode embedded PNG: {e}"))?;let src=&buf[..info.buffer_size()];let mut out=vec![0;(info.width as usize)*(info.height as usize)*4];match info.color_type{png::ColorType::Rgba=>out.copy_from_slice(src),png::ColorType::Rgb=>for(dst,s)in out.chunks_exact_mut(4).zip(src.chunks_exact(3)){dst[0]=s[0];dst[1]=s[1];dst[2]=s[2];dst[3]=255;},png::ColorType::Grayscale=>for(dst,&v)in out.chunks_exact_mut(4).zip(src){dst[0]=v;dst[1]=v;dst[2]=v;dst[3]=255;},png::ColorType::GrayscaleAlpha=>for(dst,s)in out.chunks_exact_mut(4).zip(src.chunks_exact(2)){dst[0]=s[0];dst[1]=s[0];dst[2]=s[0];dst[3]=s[1];},png::ColorType::Indexed=>return Err("indexed embedded PNG must be expanded by decoder".into())}premultiply_rgba(&mut out);Ok((out,info.width,info.height))}
fn premultiply_rgba(pixels:&mut[u8]){for p in pixels.chunks_exact_mut(4){let a=u16::from(p[3]);p[0]=((u16::from(p[0])*a+127)/255)as u8;p[1]=((u16::from(p[1])*a+127)/255)as u8;p[2]=((u16::from(p[2])*a+127)/255)as u8;}}
fn decode_base64(input:&str)->Result<Vec<u8>,String>{let mut out=Vec::with_capacity(input.len()*3/4);let mut acc=0u32;let mut bits=0u8;for b in input.bytes().filter(|b|!b.is_ascii_whitespace()){if b==b'='{break;}let v=match b{b'A'..=b'Z'=>b-b'A',b'a'..=b'z'=>b-b'a'+26,b'0'..=b'9'=>b-b'0'+52,b'+'=>62,b'/'=>63,_=>return Err("invalid base64 in image URI".into())};acc=(acc<<6)|u32::from(v);bits+=6;if bits>=8{bits-=8;out.push(((acc>>bits)&0xff)as u8);}}Ok(out)}
fn percent_decode(input:&str)->Result<Vec<u8>,String>{let bytes=input.as_bytes();let mut out=Vec::with_capacity(bytes.len());let mut i=0;while i<bytes.len(){if bytes[i]==b'%'{if i+2>=bytes.len(){return Err("truncated percent escape in data URI".into());}out.push((hex(bytes[i+1])?<<4)|hex(bytes[i+2])?);i+=3;}else{out.push(bytes[i]);i+=1;}}Ok(out)}
fn hex(b:u8)->Result<u8,String>{match b{b'0'..=b'9'=>Ok(b-b'0'),b'a'..=b'f'=>Ok(b-b'a'+10),b'A'..=b'F'=>Ok(b-b'A'+10),_=>Err("invalid percent escape in data URI".into())}}
fn fit_image(x:f64,y:f64,w:f64,h:f64,sw:f64,sh:f64,preserve:&str)->(f64,f64,f64,f64){if preserve.trim().starts_with("none"){return(x,y,w,h);}let slice=preserve.contains("slice");let scale=if slice{(w/sw).max(h/sh)}else{(w/sw).min(h/sh)};let rw=sw*scale;let rh=sh*scale;let dx=if preserve.contains("xMin"){0.0}else if preserve.contains("xMax"){w-rw}else{(w-rw)/2.0};let dy=if preserve.contains("YMin"){0.0}else if preserve.contains("YMax"){h-rh}else{(h-rh)/2.0};(x+dx,y+dy,rw,rh)}
fn matching_end<F,G>(commands:&[Command],start:usize,is_push:F,is_pop:G,name:&str)->Result<usize,String>where F:Fn(&Command)->bool,G:Fn(&Command)->bool{let mut depth=0usize;for(index,command)in commands.iter().enumerate().skip(start){if is_push(command){depth+=1;}else if is_pop(command){if depth==0{return Err(format!("encountered unmatched {name} terminator"));}depth-=1;if depth==0{return Ok(index);}}}Err(format!("{name} block is missing its terminator"))}
fn to_fill(rule:FillRule)->Fill{match rule{FillRule::NonZero=>Fill::NonZero,FillRule::EvenOdd=>Fill::EvenOdd}}
fn cap(value:LineCap)->Cap{match value{LineCap::Butt=>Cap::Butt,LineCap::Round=>Cap::Round,LineCap::Square=>Cap::Square}}
fn join(value:LineJoin)->Join{match value{LineJoin::Miter=>Join::Miter,LineJoin::Round=>Join::Round,LineJoin::Bevel=>Join::Bevel}}
fn parse_path(value:&str)->Result<BezPath,String>{BezPath::from_svg(value).map_err(|e|format!("invalid path: {e:?}"))}
fn color(value:Rgba)->AlphaColor<Srgb>{AlphaColor::<Srgb>::from_rgba8(value.r,value.g,value.b,value.a)}
fn stops(values:&[crate::GradientStop])->ColorStops{ColorStops(values.iter().map(|s|ColorStop{offset:s.offset,color:color(s.color).into()}).collect())}
fn extend(spread:GradientSpread)->Extend{match spread{GradientSpread::Pad=>Extend::Pad,GradientSpread::Repeat=>Extend::Repeat,GradientSpread::Reflect=>Extend::Reflect}}
fn diag(code:&str,message:String)->RenderDiagnostic{RenderDiagnostic{severity:"error".into(),code:code.into(),message,declaration:None}}

#[cfg(test)]
mod tests{
    use super::*;
    #[test] fn premultiplies_external_texture_alpha(){let mut p=vec![200,100,50,128];premultiply_rgba(&mut p);assert_eq!(p,vec![100,50,25,128]);}
    #[test] fn preserves_image_meet_alignment(){assert_eq!(fit_image(0.0,0.0,100.0,100.0,200.0,100.0,"xMidYMid meet"),(0.0,25.0,100.0,50.0));}
    #[test] fn embedded_svg_keeps_intrinsic_dimensions(){let(_,w,h)=rasterize_svg_image("<svg xmlns='http://www.w3.org/2000/svg' width='12' height='7'><rect width='12' height='7'/></svg>").unwrap();assert_eq!((w,h),(12,7));}
}
