use crate::model::*;
use srng::runtime::{Geometry, Scene};
use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap};

pub fn prepare_scene(scene: &Scene, revision: u64, gate: &RevisionGate) -> PreparedScene {
    let width = clamp_dimension(scene.viewport.width);
    let height = clamp_dimension(scene.viewport.height);
    let mut diagnostics = Vec::new();
    let mut commands = Vec::new();

    let node_map = scene.nodes.iter().filter(|node| node.active).map(|node| (node.id.as_str(), node)).collect::<HashMap<_, _>>();
    let defs_xml = scene.nodes.iter().filter(|node| node.active).filter_map(|node| {
        let tag = node.properties.get("svg-source-tag").map(|value| unquote(value));
        if tag.as_deref() == Some("defs") { node.properties.get("svg-source-xml").map(|value| unquote(value)) } else { None }
    }).collect::<Vec<_>>().join("\n");

    let mut ordered = Vec::new();
    for node in &scene.nodes { if node.active { ordered.push((node.paint_order, Source::Node(node))); } }
    for reference in &scene.references { if reference.active { ordered.push((reference.paint_order, Source::Reference(reference))); } }
    ordered.sort_by_key(|(order, _)| *order);

    for (_, source) in ordered {
        if !gate.is_current(revision) { break; }
        let (id, kind, geometry, props): (&str, &str, Cow<'_, Geometry>, Cow<'_, BTreeMap<String, String>>) = match source {
            Source::Node(node) => (node.id.as_str(), node.kind.as_str(), Cow::Borrowed(&node.geometry), Cow::Borrowed(&node.properties)),
            Source::Reference(reference) => {
                let geometry = merge_geometry(reference.linked_geometry.as_ref(), &reference.geometry);
                let mut props = reference.linked_properties.clone();
                props.extend(reference.properties.clone());
                (reference.id.as_str(), reference.resolved_kind.as_deref().unwrap_or("unknown"), Cow::Owned(geometry), Cow::Owned(props))
            }
        };

        let image = match embedded_image(&props, &geometry) {
            Ok(image) => image,
            Err(message) => {
                diagnostics.push(diag("error", "G260", message, id));
                None
            }
        };
        let path = path_for(kind, &geometry, &props);
        if path.is_none() && image.is_none() {
            let is_image = props.contains_key("image-data") || props.contains_key("href");
            if is_image && diagnostics.last().is_some_and(|d| d.code == "G260") { continue; }
            let (code, message) = if kind == "text" { ("G301", format!("text node `{id}` requires pre-shaped outline path data in `data`")) } else { ("G101", format!("`{id}` has no renderable geometry for kind `{kind}`")) };
            diagnostics.push(diag("warning", code, message, id));
            continue;
        }

        let clip_id = props.get("clip").map(|value| unquote(value));
        if let Some(clip_id) = clip_id.as_deref() {
            if let Some(clip_node) = node_map.get(clip_id) {
                if let Some(clip_path) = path_for(&clip_node.kind, &clip_node.geometry, &clip_node.properties) {
                    commands.push(Command::PushClip { path: clip_path, rule: fill_rule(&clip_node.properties) });
                } else {
                    diagnostics.push(diag("error", "G210", format!("clip `{clip_id}` has no renderable geometry"), id));
                }
            } else {
                diagnostics.push(diag("error", "G211", format!("clip `{clip_id}` does not exist or is inactive"), id));
            }
        }

        let native_mask = match native_records(&props, "mask-data") {
            Ok(records) => records,
            Err(message) => { diagnostics.push(diag("error", "G223", message, id)); None }
        };
        let mut legacy_mask = None;
        if let Some(records) = native_mask.as_ref() {
            commands.push(Command::PushMask { records: records.clone() });
        } else {
            legacy_mask = match legacy_mask_svg(&props, &defs_xml, &geometry, width, height) {
                Ok(mask) => mask,
                Err(message) => { diagnostics.push(diag("error", "G223", message, id)); None }
            };
            if legacy_mask.is_some() { diagnostics.push(diag("warning", "G224", "mask is using legacy SVG compatibility data; reimport to native v0.5 resources".to_string(), id)); }
        }

        let filters = match parse_filters(&props) {
            Ok(filters) => filters,
            Err(message) => { diagnostics.push(diag("error", "G240", message, id)); Vec::new() }
        };
        if !filters.is_empty() { commands.push(Command::PushFilter { filters: filters.clone() }); }

        if let Some(image) = image {
            commands.push(Command::DrawImage { image });
        } else if let Some(path) = path.as_ref() {
            match pattern_paint(&props, &defs_xml, &geometry) {
                Ok(Some(paint)) => commands.push(Command::Fill { path: path.clone(), paint, rule: fill_rule(&props) }),
                Ok(None) => {
                    if let Some(fill) = props.get("fill") {
                        if !is_none_paint(fill) {
                            match parse_paint(fill, &props, &geometry) {
                                Ok(paint) => commands.push(Command::Fill { path: path.clone(), paint, rule: fill_rule(&props) }),
                                Err(message) => diagnostics.push(diag("error", "G220", message, id)),
                            }
                        }
                    }
                }
                Err(message) => diagnostics.push(diag("error", "G222", message, id)),
            }

            if let Some(stroke) = props.get("stroke") {
                if !is_none_paint(stroke) {
                    match parse_paint(stroke, &props, &geometry) {
                        Ok(paint) => commands.push(Command::Stroke { path: path.clone(), paint, style: stroke_style(&props) }),
                        Err(message) => diagnostics.push(diag("error", "G221", message, id)),
                    }
                }
            }
        }

        if !filters.is_empty() { commands.push(Command::PopFilter); }
        if native_mask.is_some() { commands.push(Command::PopMask); }
        if legacy_mask.is_some() { diagnostics.push(diag("error", "G225", "legacy SVG masks are no longer executable in the native v0.5 command model".to_string(), id)); }
        if clip_id.is_some() { commands.push(Command::PopClip); }
    }

    PreparedScene { width, height, revision, commands, diagnostics }
}

enum Source<'a> { Node(&'a srng::runtime::SceneNode), Reference(&'a srng::runtime::SceneReference) }

fn merge_geometry(linked: Option<&Geometry>, authored: &Geometry) -> Geometry {
    let linked = linked.cloned().unwrap_or_default();
    Geometry { x: authored.x.or(linked.x), y: authored.y.or(linked.y), width: authored.width.or(linked.width), height: authored.height.or(linked.height) }
}

fn embedded_image(props:&BTreeMap<String,String>, geometry:&Geometry)->Result<Option<EmbeddedImage>,String>{
    let Some(href)=props.get("image-data").or_else(||props.get("href")).map(|v|unquote(v)) else{return Ok(None)};
    if !href.starts_with("data:"){return Err("external/network image references are disabled; only data: images are accepted".into());}
    let decoded=crate::decode_data_uri(&href)?;
    let x=geometry.x.or_else(||props.get("source-x").and_then(|v|parse_number(v))).unwrap_or(0.0);
    let y=geometry.y.or_else(||props.get("source-y").and_then(|v|parse_number(v))).unwrap_or(0.0);
    let width=geometry.width.or_else(||props.get("source-width").and_then(|v|parse_number(v))).unwrap_or(f64::from(decoded.width));
    let height=geometry.height.or_else(||props.get("source-height").and_then(|v|parse_number(v))).unwrap_or(f64::from(decoded.height));
    if !x.is_finite()||!y.is_finite()||!width.is_finite()||!height.is_finite()||width<=0.0||height<=0.0{return Err("embedded image placement must have finite positive width/height".into());}
    Ok(Some(EmbeddedImage{href,codec:decoded.codec,intrinsic_width:decoded.width,intrinsic_height:decoded.height,pixels:decoded.pixels,x,y,width,height,preserve_aspect_ratio:props.get("image-preserve-aspect-ratio").map(|v|unquote(v)).unwrap_or_else(||"xMidYMid meet".into())}))
}

fn parse_filters(props:&BTreeMap<String,String>)->Result<Vec<FilterOp>,String>{
    let Some(raw)=props.get("filter-chain").map(|v|unquote(v)) else{return Ok(Vec::new())};
    let mut out=Vec::new();
    for part in raw.split(';').map(str::trim).filter(|v|!v.is_empty()){
        if let Some(inner)=part.strip_prefix("blur(").and_then(|v|v.strip_suffix(')')){
            let vals=inner.split_whitespace().filter_map(|v|v.parse::<f64>().ok()).collect::<Vec<_>>();
            let sx=*vals.first().ok_or_else(||"blur() requires sigma".to_string())?;let sy=*vals.get(1).unwrap_or(&sx);
            if sx<0.0||sy<0.0||!sx.is_finite()||!sy.is_finite(){return Err("blur sigma must be finite and non-negative".into());}
            out.push(FilterOp::GaussianBlur{sigma_x:sx,sigma_y:sy});
        }else if let Some(inner)=part.strip_prefix("offset(").and_then(|v|v.strip_suffix(')')){
            let vals=inner.split_whitespace().filter_map(|v|v.parse::<f64>().ok()).collect::<Vec<_>>();if vals.len()!=2{return Err("offset() requires dx dy".into());}out.push(FilterOp::Offset{dx:vals[0],dy:vals[1]});
        }else{return Err(format!("unsupported native filter operation `{part}`"));}
    }
    Ok(out)
}

fn path_for(kind: &str, geometry: &Geometry, props: &BTreeMap<String, String>) -> Option<PathData> {
    if let Some(data) = props.get("data") { let value = unquote(data); if !value.trim().is_empty() { return Some(PathData { svg: value }); } }
    let x=geometry.x?;let y=geometry.y?;let width=geometry.width?;let height=geometry.height?;
    match kind {
        "rect"|"canvas"|"group"|"shadow"=>Some(PathData{svg:format!("M {x} {y} h {width} v {height} h {} Z",-width)}),
        "ellipse"|"circle"=>{let rx=width/2.0;let ry=height/2.0;let cx=x+rx;let cy=y+ry;Some(PathData{svg:format!("M {} {cy} A {rx} {ry} 0 1 0 {} {cy} A {rx} {ry} 0 1 0 {} {cy} Z",cx-rx,cx+rx,cx-rx)})},
        _=>None,
    }
}

fn native_records(props:&BTreeMap<String,String>,key:&str)->Result<Option<Vec<VectorRecord>>,String>{let Some(data)=props.get(key).map(|v|unquote(v))else{return Ok(None)};let mut records=Vec::new();for(index,line)in data.lines().enumerate(){if line.trim().is_empty(){continue;}let Some((paint,path))=line.split_once('|')else{return Err(format!("{key} record {} is missing `paint|path` separator",index+1));};let paint=parse_color(paint.trim()).map(Paint::Solid).map_err(|error|format!("{key} record {}: {error}",index+1))?;if path.trim().is_empty(){return Err(format!("{key} record {} has an empty path",index+1));}records.push(VectorRecord{path:PathData{svg:path.trim().to_string()},paint});}if records.is_empty(){return Err(format!("{key} contains no renderable records"));}Ok(Some(records))}
fn legacy_mask_svg(props:&BTreeMap<String,String>,defs_xml:&str,geometry:&Geometry,viewport_width:u16,viewport_height:u16)->Result<Option<String>,String>{if props.get("mask-mode").or_else(||props.get("svg-mask-mode")).map(|v|unquote(v)).as_deref()==Some("binary-opaque-clip"){return Ok(None);}let raw=props.get("mask-ref").or_else(||props.get("svg-mask-ref")).or_else(||props.get("svg-attr-mask")).map(|v|unquote(v));let Some(raw)=raw else{return Ok(None)};let mask_ref=if let Some(v)=raw.strip_prefix("url(#").and_then(|v|v.strip_suffix(')')){v.to_string()}else if !raw.contains('(')&&!raw.trim().is_empty(){raw.clone()}else{return Ok(None)};if defs_xml.trim().is_empty(){return Err(format!("SVG mask `{mask_ref}` has no native mask-data and no preserved <defs> XML"));}let x=geometry.x.unwrap_or(0.0);let y=geometry.y.unwrap_or(0.0);let width=geometry.width.unwrap_or(f64::from(viewport_width));let height=geometry.height.unwrap_or(f64::from(viewport_height));let escaped_ref=xml_escape_attr(&mask_ref);Ok(Some(format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{viewport_width}\" height=\"{viewport_height}\"><defs>{defs_xml}</defs><rect x=\"{x}\" y=\"{y}\" width=\"{width}\" height=\"{height}\" fill=\"white\" mask=\"url(#{escaped_ref})\"/></svg>")))}
fn pattern_paint(props:&BTreeMap<String,String>,defs_xml:&str,geometry:&Geometry)->Result<Option<Paint>,String>{let pattern_ref=props.get("pattern-ref").or_else(||props.get("svg-pattern-ref")).map(|v|unquote(v));let Some(pattern_ref)=pattern_ref else{return Ok(None)};let stored_width=props.get("pattern-width").or_else(||props.get("svg-pattern-width")).and_then(|v|parse_number(v)).filter(|v|v.is_finite()&&*v>0.0).ok_or_else(||format!("pattern `{pattern_ref}` has no positive tile width"))?;let stored_height=props.get("pattern-height").or_else(||props.get("svg-pattern-height")).and_then(|v|parse_number(v)).filter(|v|v.is_finite()&&*v>0.0).ok_or_else(||format!("pattern `{pattern_ref}` has no positive tile height"))?;let fallback_pattern=props.get("pattern-source").or_else(||props.get("svg-pattern-source-xml")).map(|v|unquote(v)).unwrap_or_default();let units=props.get("pattern-units").map(|v|unquote(v)).unwrap_or_else(||if fallback_pattern.contains("patternUnits=\"objectBoundingBox\""){"objectBoundingBox".into()}else{"userSpaceOnUse".into()});let(tile_width,tile_height)=if units=="objectBoundingBox"{(stored_width*geometry.width.unwrap_or(1.0).abs(),stored_height*geometry.height.unwrap_or(1.0).abs())}else{(stored_width,stored_height)};if !tile_width.is_finite()||!tile_height.is_finite()||tile_width<=0.0||tile_height<=0.0{return Err(format!("pattern `{pattern_ref}` resolves to an empty tile"));}if let Some(records)=native_records(props,"pattern-data")?{return Ok(Some(Paint::Pattern{records,tile_width,tile_height}));}if fallback_pattern.trim().is_empty()&&defs_xml.trim().is_empty(){return Err(format!("pattern `{pattern_ref}` has no native pattern-data"));}let definitions=if defs_xml.trim().is_empty(){format!("<defs>{fallback_pattern}</defs>")}else{defs_xml.to_string()};let escaped_ref=xml_escape_attr(&pattern_ref);let svg=format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{tile_width}\" height=\"{tile_height}\"><defs>{definitions}</defs><rect width=\"{tile_width}\" height=\"{tile_height}\" fill=\"url(#{escaped_ref})\"/></svg>");Ok(Some(Paint::SvgPattern{svg,tile_width,tile_height}))}
fn xml_escape_attr(value:&str)->String{value.replace('&',"&amp;").replace('<',"&lt;").replace('>',"&gt;").replace('"',"&quot;").replace('\'',"&apos;")}

fn parse_paint(value:&str,props:&BTreeMap<String,String>,geometry:&Geometry)->Result<Paint,String>{
    let value=value.trim();let spread=gradient_spread(props);
    if value=="linear-gradient"||value.starts_with("linear-gradient("){let stops_text=props.get("gradient-stops").ok_or_else(||"linear gradient requires `gradient-stops`".to_string())?;let stops=parse_gradient_stops(stops_text)?;if stops.len()<2{return Err("linear gradient requires at least two stops".into());}let(start,end)=gradient_points(props,geometry)?;return Ok(Paint::LinearGradient{start,end,stops,spread});}
    if value=="radial-gradient"||value.starts_with("radial-gradient("){let stops_text=props.get("gradient-stops").ok_or_else(||"radial gradient requires `gradient-stops`".to_string())?;let stops=parse_gradient_stops(stops_text)?;if stops.len()<2{return Err("radial gradient requires at least two stops".into());}let center=(required_number(props,"gradient-cx")?,required_number(props,"gradient-cy")?);let focal=(props.get("gradient-fx").and_then(|v|parse_number(v)).unwrap_or(center.0),props.get("gradient-fy").and_then(|v|parse_number(v)).unwrap_or(center.1));let radius=required_number(props,"gradient-r")?;let focal_radius=props.get("gradient-fr").and_then(|v|parse_number(v)).unwrap_or(0.0);return Ok(Paint::RadialGradient{center,focal,focal_radius,radius,stops,spread});}
    parse_color(value).map(Paint::Solid)
}
fn gradient_spread(props:&BTreeMap<String,String>)->GradientSpread{match props.get("gradient-spread").map(|v|unquote(v)){Some(v)if v=="repeat"=>GradientSpread::Repeat,Some(v)if v=="reflect"=>GradientSpread::Reflect,_=>GradientSpread::Pad}}
fn gradient_points(props:&BTreeMap<String,String>,geometry:&Geometry)->Result<((f64,f64),(f64,f64)),String>{if let(Some(a),Some(b))=(props.get("gradient-start"),props.get("gradient-end")){let a=parse_pair(a).ok_or_else(||"invalid gradient-start".to_string())?;let b=parse_pair(b).ok_or_else(||"invalid gradient-end".to_string())?;return Ok((a,b));}let x=geometry.x.unwrap_or(0.0);let y=geometry.y.unwrap_or(0.0);let w=geometry.width.unwrap_or(1.0);Ok(((x,y),(x+w,y)))}
fn parse_pair(value:&str)->Option<(f64,f64)>{let v=unquote(value);let n=v.split_whitespace().filter_map(parse_number).collect::<Vec<_>>();if n.len()==2{Some((n[0],n[1]))}else{None}}
fn required_number(props:&BTreeMap<String,String>,key:&str)->Result<f64,String>{props.get(key).and_then(|v|parse_number(v)).ok_or_else(||format!("missing or invalid `{key}`"))}
fn parse_gradient_stops(value:&str)->Result<Vec<GradientStop>,String>{let v=unquote(value);let mut out=Vec::new();for part in v.split(',').map(str::trim).filter(|p|!p.is_empty()){let mut fields=part.split_whitespace();let offset=fields.next().ok_or_else(||"gradient stop missing offset".to_string())?.trim_end_matches('%').parse::<f32>().map_err(|_|"invalid gradient stop offset".to_string())?;let color=fields.next().ok_or_else(||"gradient stop missing color".to_string())?;let offset=if part.contains('%'){offset/100.0}else{offset};out.push(GradientStop{offset:offset.clamp(0.0,1.0),color:parse_color(color)?});}Ok(out)}
fn parse_color(value:&str)->Result<Rgba,String>{let v=unquote(value);let v=v.trim();let Some(hex)=v.strip_prefix('#')else{return Err(format!("unsupported color `{v}`"));};match hex.len(){6=>Ok(Rgba{r:u8::from_str_radix(&hex[0..2],16).map_err(|_|"bad color")?,g:u8::from_str_radix(&hex[2..4],16).map_err(|_|"bad color")?,b:u8::from_str_radix(&hex[4..6],16).map_err(|_|"bad color")?,a:255}),8=>Ok(Rgba{r:u8::from_str_radix(&hex[0..2],16).map_err(|_|"bad color")?,g:u8::from_str_radix(&hex[2..4],16).map_err(|_|"bad color")?,b:u8::from_str_radix(&hex[4..6],16).map_err(|_|"bad color")?,a:u8::from_str_radix(&hex[6..8],16).map_err(|_|"bad color")?}),_=>Err(format!("unsupported color `{v}`"))}}
fn stroke_style(props:&BTreeMap<String,String>)->StrokeStyle{StrokeStyle{width:props.get("stroke-width").and_then(|v|parse_number(v)).unwrap_or(1.0),miter_limit:props.get("stroke-miterlimit").and_then(|v|parse_number(v)).unwrap_or(4.0),line_cap:match props.get("stroke-linecap").map(|v|unquote(v)).as_deref(){Some("round")=>LineCap::Round,Some("square")=>LineCap::Square,_=>LineCap::Butt},line_join:match props.get("stroke-linejoin").map(|v|unquote(v)).as_deref(){Some("round")=>LineJoin::Round,Some("bevel")=>LineJoin::Bevel,_=>LineJoin::Miter},dash:props.get("stroke-dasharray").map(|v|unquote(v).split(|c:char|c==','||c.is_whitespace()).filter_map(parse_number).collect()).unwrap_or_default(),dash_offset:props.get("stroke-dashoffset").and_then(|v|parse_number(v)).unwrap_or(0.0)}}
fn fill_rule(props:&BTreeMap<String,String>)->FillRule{if props.get("fill-rule").map(|v|unquote(v))==Some("evenodd".into()){FillRule::EvenOdd}else{FillRule::NonZero}}
fn is_none_paint(value:&str)->bool{matches!(unquote(value).trim(),"none"|"transparent")}
fn parse_number(value:&str)->Option<f64>{unquote(value).trim().trim_end_matches("px").parse().ok()}
fn clamp_dimension(value:f64)->u16{if !value.is_finite()||value<=0.0{1}else{value.ceil().clamp(1.0,f64::from(u16::MAX))as u16}}
fn unquote(value:&str)->String{let value=value.trim();let Some(inner)=value.strip_prefix('"').and_then(|v|v.strip_suffix('"'))else{return value.to_string()};inner.replace("\\n","\n").replace("\\\"","\"").replace("\\\\","\\")}
fn diag(severity:&str,code:&str,message:String,declaration:&str)->RenderDiagnostic{RenderDiagnostic{severity:severity.into(),code:code.into(),message,declaration:Some(declaration.into())}}
