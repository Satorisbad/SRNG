use crate::{Command, FillRule, LineCap, LineJoin, Paint, PreparedScene, RenderDiagnostic, Rgba};
use kurbo::{BezPath, Cap, Join, Stroke as KurboStroke};
use peniko::{color::{AlphaColor, Srgb}, ColorStop, ColorStops, Fill, Gradient};
use std::fmt;
use vello_hybrid::{RenderSize, RenderTargetConfig, Renderer as HybridRenderer, Resources, Scene as HybridScene, TextureBindings};
use wgpu::{CommandEncoder, Device, Queue, TextureFormat, TextureView};

#[derive(Debug)]
pub enum GpuRenderError { Scene(Vec<RenderDiagnostic>), Backend(String) }
impl fmt::Display for GpuRenderError { fn fmt(&self,f:&mut fmt::Formatter<'_>)->fmt::Result{match self{Self::Scene(d)=>write!(f,"scene preparation produced {} renderer diagnostic(s)",d.len()),Self::Backend(m)=>write!(f,"GPU renderer error: {m}")}} }
impl std::error::Error for GpuRenderError {}

#[derive(Debug)]
pub struct GpuRenderer{renderer:HybridRenderer,resources:Resources,target_width:u32,target_height:u32,target_format:TextureFormat}
impl GpuRenderer{
    pub fn new(device:&Device,target_format:TextureFormat,target_width:u32,target_height:u32)->Self{let config=RenderTargetConfig{format:target_format,width:target_width.max(1),height:target_height.max(1)};let(renderer,resources)=HybridRenderer::new(device,&config);Self{renderer,resources,target_width:config.width,target_height:config.height,target_format}}
    pub fn target_format(&self)->TextureFormat{self.target_format}
    pub fn target_size(&self)->(u32,u32){(self.target_width,self.target_height)}
    pub fn render_to_view(&mut self,scene:&PreparedScene,device:&Device,queue:&Queue,encoder:&mut CommandEncoder,view:&TextureView)->Result<(),GpuRenderError>{let width=u32::from(scene.width);let height=u32::from(scene.height);if width>self.target_width||height>self.target_height{return Err(GpuRenderError::Backend(format!("prepared scene {width}x{height} exceeds renderer target {}x{}",self.target_width,self.target_height)));}let hybrid_scene=build_scene(scene).map_err(GpuRenderError::Scene)?;let render_size=RenderSize{width,height};let texture_bindings=TextureBindings::new();self.renderer.render(&hybrid_scene,&mut self.resources,device,queue,encoder,&render_size,view,&texture_bindings).map_err(|e|GpuRenderError::Backend(e.to_string()))}
}

pub fn build_scene(scene:&PreparedScene)->Result<HybridScene,Vec<RenderDiagnostic>>{let mut output=HybridScene::new(scene.width,scene.height);let mut diagnostics=scene.diagnostics.clone();for command in &scene.commands{if let Err(message)=apply(&mut output,command){diagnostics.push(RenderDiagnostic{severity:"error".into(),code:"G600".into(),message,declaration:None});}}if diagnostics.iter().any(|d|d.severity=="error"){Err(diagnostics)}else{Ok(output)}}
fn apply(context:&mut HybridScene,command:&Command)->Result<(),String>{match command{Command::PushClip{path,rule}=>{context.set_fill_rule(to_fill(*rule));context.push_clip_path(&parse_path(&path.svg)?);},Command::PopClip=>context.pop_clip_path(),Command::PushMask{..}|Command::PopMask=>return Err("native mask compositing requires GPU texture-layer support; use CPU backend for masks in v0.5".into()),Command::Fill{path,paint,rule}=>{context.set_fill_rule(to_fill(*rule));set_paint(context,paint)?;context.fill_path(&parse_path(&path.svg)?);},Command::Stroke{path,paint,style}=>{set_paint(context,paint)?;let stroke=KurboStroke::new(style.width).with_miter_limit(style.miter_limit).with_caps(cap(style.line_cap)).with_join(join(style.line_join)).with_dashes(style.dash_offset,style.dash.iter());context.set_stroke(stroke);context.stroke_path(&parse_path(&path.svg)?);}}Ok(())}
fn to_fill(rule:FillRule)->Fill{match rule{FillRule::NonZero=>Fill::NonZero,FillRule::EvenOdd=>Fill::EvenOdd}}
fn cap(value:LineCap)->Cap{match value{LineCap::Butt=>Cap::Butt,LineCap::Round=>Cap::Round,LineCap::Square=>Cap::Square}}
fn join(value:LineJoin)->Join{match value{LineJoin::Miter=>Join::Miter,LineJoin::Round=>Join::Round,LineJoin::Bevel=>Join::Bevel}}
fn parse_path(value:&str)->Result<BezPath,String>{BezPath::from_svg(value).map_err(|e|format!("invalid path: {e:?}"))}
fn color(value:Rgba)->AlphaColor<Srgb>{AlphaColor::<Srgb>::from_rgba8(value.r,value.g,value.b,value.a)}
fn stops(values:&[crate::GradientStop])->ColorStops{ColorStops(values.iter().map(|s|ColorStop{offset:s.offset,color:color(s.color).into()}).collect())}
fn set_paint(context:&mut HybridScene,paint:&Paint)->Result<(),String>{match paint{Paint::Solid(v)=>context.set_paint(color(*v)),Paint::LinearGradient{start,end,stops:values}=>context.set_paint(Gradient::new_linear(*start,*end).with_stops(stops(values))),Paint::RadialGradient{center,focal,radius,stops:values}=>context.set_paint(Gradient::new_two_point_radial(*focal,0.0,*center,*radius as f32).with_stops(stops(values))),Paint::Pattern{..}|Paint::SvgPattern{..}=>return Err("pattern paints require GPU texture binding support; use CPU backend for patterns in v0.5".into())}Ok(())}
