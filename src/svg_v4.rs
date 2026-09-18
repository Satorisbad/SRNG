use roxmltree::{Document as XmlDocument, Node};
use std::collections::HashMap;

pub use crate::svg_base::{strip_svg_provenance, ImportDiagnostic, ImportOptions, ImportResult};

#[derive(Clone, Debug)]
struct Gradient {
    kind: &'static str,
    units: String,
    spread: String,
    transform: String,
    x1: String,
    y1: String,
    x2: String,
    y2: String,
    cx: String,
    cy: String,
    r: String,
    fx: String,
    fy: String,
    fr: String,
    stops: String,
}

#[derive(Clone, Debug)]
struct MaskInfo { units:String, content_units:String, x:String, y:String, width:String, height:String }
#[derive(Clone, Debug)]
struct UseTarget { data:String, fill:String, stroke:String }
#[derive(Clone, Debug)]
struct FilterInfo { chain:String }

#[derive(Default)]
struct Features { viewbox:bool, transforms:bool, opacity:bool, gradients:bool, text:bool, images:bool, uses:bool, filters:bool }

pub fn import_svg(svg:&str,source_name:&str,options:&ImportOptions)->ImportResult{
    let mut result=crate::svg_base::import_svg(svg,source_name,options);
    let features=promote(svg,&mut result.source);
    for diagnostic in &mut result.diagnostics{
        match diagnostic.code.as_str(){
            "S120" if features.viewbox=>info(diagnostic,"viewBox is represented by native SRNG viewbox/preserve-aspect-ratio semantics"),
            "S130" if features.transforms=>info(diagnostic,"SVG transform is represented by native SRNG transform semantics"),
            "S131" if features.opacity&&diagnostic.message.contains("opacity")=>info(diagnostic,"opacity is represented by native SRNG opacity semantics"),
            "S131" if features.filters&&diagnostic.message.contains("filter")=>info(diagnostic,"supported SVG filter primitives are represented by native SRNG filter-chain semantics"),
            "S230" if features.gradients&&diagnostic.message.contains("url(#")=>info(diagnostic,"SVG gradient paint is mapped to native SRNG gradient semantics"),
            "S301" if features.text=>info(diagnostic,"text is represented natively and uses deterministic vector fallback when no outline path is present"),
            "S302" if features.images&&diagnostic.message.contains("<image>")=>info(diagnostic,"embedded data images are represented by native SRNG image properties"),
            "S302" if features.uses&&diagnostic.message.contains("<use>")=>info(diagnostic,"local <use> references are resolved to native SRNG geometry"),
            _=>{}
        }
    }
    result
}
fn info(diagnostic:&mut ImportDiagnostic,message:&str){diagnostic.severity="info".into();diagnostic.message=message.into();}

fn promote(svg:&str,source:&mut String)->Features{
    let Ok(doc)=XmlDocument::parse(svg)else{return Features::default()};
    let gradients=collect_gradients(&doc);let masks=collect_masks(&doc);let uses=collect_use_targets(&doc);let filters=collect_filters(&doc);
    let mut features=Features{viewbox:doc.root_element().attribute("viewBox").is_some(),transforms:doc.descendants().any(|n|n.is_element()&&n.attribute("transform").is_some()),opacity:doc.descendants().any(|n|n.is_element()&&["opacity","fill-opacity","stroke-opacity"].iter().any(|k|n.attribute(*k).is_some())),gradients:false,text:doc.descendants().any(|n|n.is_element()&&matches!(n.tag_name().name(),"text"|"tspan")),images:false,uses:false,filters:false};
    let mut out=String::with_capacity(source.len()+source.len()/3);
    for line in source.lines(){
        out.push_str(line);out.push('\n');let trimmed=line.trim_start();let indent=&line[..line.len()-trimmed.len()];
        for(from,to)in[("svg-attr-transform:","transform:"),("svg-attr-opacity:","opacity:"),("svg-attr-fill-opacity:","fill-opacity:"),("svg-attr-stroke-opacity:","stroke-opacity:"),("svg-attr-viewBox:","viewbox:"),("svg-attr-preserveAspectRatio:","preserve-aspect-ratio:"),("svg-attr-clipPathUnits:","clip-units:"),("svg-attr-maskUnits:","mask-units:"),("svg-attr-maskContentUnits:","mask-content-units:"),("svg-attr-font-size:","font-size:"),("svg-attr-font-family:","font-family:"),("svg-attr-font-weight:","font-weight:"),("svg-attr-font-style:","font-style:"),("svg-attr-text-anchor:","text-anchor:"),("svg-attr-letter-spacing:","letter-spacing:"),("svg-attr-word-spacing:","word-spacing:"),("svg-attr-x:","source-x:"),("svg-attr-y:","source-y:"),("svg-attr-width:","source-width:"),("svg-attr-height:","source-height:")]{if let Some(rest)=trimmed.strip_prefix(from){prop_raw(&mut out,indent,to.trim_end_matches(':'),rest.trim());if from=="svg-attr-preserveAspectRatio:"{prop_raw(&mut out,indent,"image-preserve-aspect-ratio",rest.trim());}}}
        if let Some(rest)=trimmed.strip_prefix("svg-attr-href:").or_else(||trimmed.strip_prefix("svg-attr-xlink-href:")){prop_raw(&mut out,indent,"href",rest.trim());if let Some(href)=parse_prop_rest(rest){if href.starts_with("data:image/"){prop(&mut out,indent,"image-data",&quote(&href));features.images=true;}if let Some(id)=href.strip_prefix('#'){if let Some(target)=uses.get(id){prop(&mut out,indent,"use-data",&quote(&target.data));prop(&mut out,indent,"use-fill",&target.fill);prop(&mut out,indent,"use-stroke",&target.stroke);features.uses=true;}}}}
        if let Some(fill)=parse_prop(trimmed,"svg-fill:"){if let Some(id)=url_fragment(&fill){if let Some(gradient)=gradients.get(id){emit_gradient(&mut out,indent,gradient);features.gradients=true;}}}
        if let Some(mask_ref)=parse_prop(trimmed,"mask-ref:").or_else(||parse_prop(trimmed,"svg-attr-mask:")){if let Some(id)=url_fragment(&mask_ref){if let Some(mask)=masks.get(id){prop(&mut out,indent,"mask-units",&quote(&mask.units));prop(&mut out,indent,"mask-content-units",&quote(&mask.content_units));prop(&mut out,indent,"mask-x",&quote(&mask.x));prop(&mut out,indent,"mask-y",&quote(&mask.y));prop(&mut out,indent,"mask-width",&quote(&mask.width));prop(&mut out,indent,"mask-height",&quote(&mask.height));}}}
        if let Some(filter)=parse_prop(trimmed,"svg-attr-filter:").or_else(||parse_prop(trimmed,"svg-filter:")){if let Some(id)=url_fragment(&filter){if let Some(info)=filters.get(id){prop(&mut out,indent,"filter-chain",&quote(&info.chain));features.filters=true;}}}
    }
    *source=out;features
}

fn emit_gradient(out:&mut String,indent:&str,g:&Gradient){prop(out,indent,"fill",g.kind);prop(out,indent,"gradient-kind",&quote(g.kind));prop(out,indent,"gradient-units",&quote(&g.units));prop(out,indent,"gradient-spread",&quote(&g.spread));if !g.transform.is_empty(){prop(out,indent,"gradient-transform",&quote(&g.transform));}prop(out,indent,"gradient-stops",&g.stops);if g.kind=="linear-gradient"{for(k,v)in[("gradient-x1",&g.x1),("gradient-y1",&g.y1),("gradient-x2",&g.x2),("gradient-y2",&g.y2)]{prop(out,indent,k,&quote(v));}}else{for(k,v)in[("gradient-cx",&g.cx),("gradient-cy",&g.cy),("gradient-r",&g.r),("gradient-fx",&g.fx),("gradient-fy",&g.fy),("gradient-fr",&g.fr)]{prop(out,indent,k,&quote(v));}}}

fn collect_gradients(doc:&XmlDocument<'_>)->HashMap<String,Gradient>{
    let nodes=doc.descendants().filter(|n|n.is_element()).filter_map(|n|{let kind=match n.tag_name().name(){"linearGradient"=>"linear-gradient","radialGradient"=>"radial-gradient",_=>return None};Some((n.attribute("id")?.to_string(),kind,n))}).collect::<Vec<_>>();let mut out=HashMap::new();
    for(id,kind,node)in nodes{let inherited=node.attribute("href").or_else(||node.attribute(("http://www.w3.org/1999/xlink","href"))).and_then(|h|h.strip_prefix('#')).and_then(|base|out.get(base).cloned());let stops=gradient_stops(node);let base=inherited;out.insert(id,Gradient{kind,units:node.attribute("gradientUnits").map(str::to_string).or_else(||base.as_ref().map(|g:&Gradient|g.units.clone())).unwrap_or_else(||"objectBoundingBox".into()),spread:node.attribute("spreadMethod").map(str::to_string).or_else(||base.as_ref().map(|g|g.spread.clone())).unwrap_or_else(||"pad".into()),transform:node.attribute("gradientTransform").map(str::to_string).or_else(||base.as_ref().map(|g|g.transform.clone())).unwrap_or_default(),x1:attr_or(&node,"x1",base.as_ref().map(|g|g.x1.as_str()),"0%"),y1:attr_or(&node,"y1",base.as_ref().map(|g|g.y1.as_str()),"0%"),x2:attr_or(&node,"x2",base.as_ref().map(|g|g.x2.as_str()),"100%"),y2:attr_or(&node,"y2",base.as_ref().map(|g|g.y2.as_str()),"0%"),cx:attr_or(&node,"cx",base.as_ref().map(|g|g.cx.as_str()),"50%"),cy:attr_or(&node,"cy",base.as_ref().map(|g|g.cy.as_str()),"50%"),r:attr_or(&node,"r",base.as_ref().map(|g|g.r.as_str()),"50%"),fx:node.attribute("fx").map(str::to_string).or_else(||base.as_ref().map(|g|g.fx.clone())).unwrap_or_else(||node.attribute("cx").unwrap_or("50%").into()),fy:node.attribute("fy").map(str::to_string).or_else(||base.as_ref().map(|g|g.fy.clone())).unwrap_or_else(||node.attribute("cy").unwrap_or("50%").into()),fr:attr_or(&node,"fr",base.as_ref().map(|g|g.fr.as_str()),"0%"),stops:if stops.is_empty(){base.as_ref().map(|g|g.stops.clone()).unwrap_or_default()}else{stops}});}
    out
}
fn attr_or(node:&Node<'_, '_>,key:&str,inherited:Option<&str>,default:&str)->String{node.attribute(key).or(inherited).unwrap_or(default).to_string()}
fn gradient_stops(gradient:Node<'_, '_>)->String{gradient.children().filter(|n|n.is_element()&&n.tag_name().name()=="stop").filter_map(|stop|{let mut color=stop.attribute("stop-color").unwrap_or("#000000").to_string();let mut opacity=stop.attribute("stop-opacity").and_then(parse_number).unwrap_or(1.0);if let Some(style)=stop.attribute("style"){for field in style.split(';'){if let Some((key,value))=field.split_once(':'){match key.trim(){"stop-color"=>color=value.trim().into(),"stop-opacity"=>opacity=parse_number(value).unwrap_or(opacity),_=>{}}}}}let(r,g,b,a)=color_rgba(&color)?;let alpha=(f64::from(a)*opacity.clamp(0.0,1.0)).round()as u8;Some(format!("{} #{r:02x}{g:02x}{b:02x}{alpha:02x}",normalize_offset(stop.attribute("offset").unwrap_or("0"))))}).collect::<Vec<_>>().join(", ")}

fn collect_filters(doc:&XmlDocument<'_>)->HashMap<String,FilterInfo>{
    doc.descendants().filter(|n|n.is_element()&&n.tag_name().name()=="filter").filter_map(|filter|{let id=filter.attribute("id")?.to_string();let mut ops=Vec::new();for child in filter.children().filter(|n|n.is_element()){match child.tag_name().name(){"feGaussianBlur"=>{let values=child.attribute("stdDeviation").unwrap_or("0").split(|c:char|c==','||c.is_whitespace()).filter(|v|!v.is_empty()).collect::<Vec<_>>();if let Some(x)=values.first(){let y=values.get(1).copied().unwrap_or(x);ops.push(format!("blur({x} {y})"));}},"feOffset"=>{ops.push(format!("offset({} {})",child.attribute("dx").unwrap_or("0"),child.attribute("dy").unwrap_or("0")));},_=>return None}}(!ops.is_empty()).then_some((id,FilterInfo{chain:ops.join("; ")}))}).collect()
}
fn collect_masks(doc:&XmlDocument<'_>)->HashMap<String,MaskInfo>{doc.descendants().filter(|n|n.is_element()&&n.tag_name().name()=="mask").filter_map(|n|Some((n.attribute("id")?.into(),MaskInfo{units:n.attribute("maskUnits").unwrap_or("objectBoundingBox").into(),content_units:n.attribute("maskContentUnits").unwrap_or("userSpaceOnUse").into(),x:n.attribute("x").unwrap_or("-10%").into(),y:n.attribute("y").unwrap_or("-10%").into(),width:n.attribute("width").unwrap_or("120%").into(),height:n.attribute("height").unwrap_or("120%").into()}))).collect()}
fn collect_use_targets(doc:&XmlDocument<'_>)->HashMap<String,UseTarget>{doc.descendants().filter(|n|n.is_element()).filter_map(|n|{let id=n.attribute("id")?.to_string();let data=shape_path(n)?;let fill=n.attribute("fill").and_then(solid_color).unwrap_or_else(||"#000000".into());let stroke=n.attribute("stroke").and_then(solid_color).unwrap_or_else(||"none".into());Some((id,UseTarget{data,fill,stroke}))}).collect()}
fn shape_path(n:Node<'_, '_>)->Option<String>{let num=|key:&str,default:f64|n.attribute(key).and_then(parse_number).unwrap_or(default);match n.tag_name().name(){"path"=>n.attribute("d").map(str::to_string),"rect"=>{let(x,y,w,h)=(num("x",0.0),num("y",0.0),num("width",0.0),num("height",0.0));Some(format!("M {x} {y} H {} V {} H {x} Z",x+w,y+h))},"circle"=>{let(cx,cy,r)=(num("cx",0.0),num("cy",0.0),num("r",0.0));Some(format!("M {} {cy} A {r} {r} 0 1 0 {} {cy} A {r} {r} 0 1 0 {} {cy} Z",cx-r,cx+r,cx-r))},"ellipse"=>{let(cx,cy,rx,ry)=(num("cx",0.0),num("cy",0.0),num("rx",0.0),num("ry",0.0));Some(format!("M {} {cy} A {rx} {ry} 0 1 0 {} {cy} A {rx} {ry} 0 1 0 {} {cy} Z",cx-rx,cx+rx,cx-rx))},"line"=>Some(format!("M {} {} L {} {}",num("x1",0.0),num("y1",0.0),num("x2",0.0),num("y2",0.0))),"polygon"|"polyline"=>{let vals=n.attribute("points")?.split(|c:char|c==','||c.is_whitespace()).filter(|s|!s.is_empty()).collect::<Vec<_>>();if vals.len()<2||vals.len()%2!=0{return None;}let mut d=format!("M {} {}",vals[0],vals[1]);for p in vals[2..].chunks_exact(2){d.push_str(&format!(" L {} {}",p[0],p[1]));}if n.tag_name().name()=="polygon"{d.push_str(" Z");}Some(d)},_=>None}}
fn solid_color(value:&str)->Option<String>{let(r,g,b,a)=color_rgba(value)?;Some(if a==255{format!("#{r:02x}{g:02x}{b:02x}")}else{format!("#{r:02x}{g:02x}{b:02x}{a:02x}")})}
fn color_rgba(value:&str)->Option<(u8,u8,u8,u8)>{let lower=value.trim().to_ascii_lowercase();let named=match lower.as_str(){"black"=>"#000000","white"=>"#ffffff","red"=>"#ff0000","green"=>"#008000","blue"=>"#0000ff","yellow"=>"#ffff00","gray"|"grey"=>"#808080","transparent"=>"#00000000",_=>lower.as_str()};let h=named.strip_prefix('#')?;let byte=|s:&str|u8::from_str_radix(s,16).ok();match h.len(){3=>Some((byte(&h[0..1].repeat(2))?,byte(&h[1..2].repeat(2))?,byte(&h[2..3].repeat(2))?,255)),4=>Some((byte(&h[0..1].repeat(2))?,byte(&h[1..2].repeat(2))?,byte(&h[2..3].repeat(2))?,byte(&h[3..4].repeat(2))?)),6=>Some((byte(&h[0..2])?,byte(&h[2..4])?,byte(&h[4..6])?,255)),8=>Some((byte(&h[0..2])?,byte(&h[2..4])?,byte(&h[4..6])?,byte(&h[6..8])?)),_=>None}}
fn normalize_offset(v:&str)->String{let v=v.trim();if let Some(p)=v.strip_suffix('%').and_then(parse_number){format!("{}",(p/100.0).clamp(0.0,1.0))}else{format!("{}",parse_number(v).unwrap_or(0.0).clamp(0.0,1.0))}}
fn parse_number(v:&str)->Option<f64>{v.trim().trim_end_matches("px").parse().ok()}
fn url_fragment(v:&str)->Option<&str>{v.trim().strip_prefix("url(#")?.strip_suffix(')')}
fn parse_prop(line:&str,prefix:&str)->Option<String>{Some(unquote(line.strip_prefix(prefix)?.trim().strip_suffix(';')?.trim()))}
fn parse_prop_rest(rest:&str)->Option<String>{Some(unquote(rest.trim().strip_suffix(';')?.trim()))}
fn unquote(v:&str)->String{let v=v.trim();let Some(inner)=v.strip_prefix('"').and_then(|x|x.strip_suffix('"'))else{return v.into()};inner.replace("\\n","\n").replace("\\\"","\"").replace("\\\\","\\")}
fn quote(v:&str)->String{format!("\"{}\"",v.replace('\\',"\\\\").replace('"',"\\\"").replace('\n',"\\n"))}
fn prop(out:&mut String,indent:&str,key:&str,value:&str){out.push_str(indent);out.push_str(key);out.push_str(": ");out.push_str(value);out.push_str(";\n");}
fn prop_raw(out:&mut String,indent:&str,key:&str,rest:&str){out.push_str(indent);out.push_str(key);out.push_str(": ");out.push_str(rest);if !rest.ends_with(';'){out.push(';');}out.push('\n');}

#[cfg(test)]mod tests{
use super::*;
#[test]fn promotes_core_v4_semantics(){let svg=r##"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="100" viewBox="0 0 100 50"><defs><linearGradient id="g" spreadMethod="reflect"><stop offset="0" stop-color="#f00"/><stop offset="100%" stop-color="#00f"/></linearGradient><path id="p" d="M0 0 L10 0 L10 10 Z" fill="#ff0000"/><filter id="b"><feGaussianBlur stdDeviation="2"/><feOffset dx="1" dy="3"/></filter></defs><g transform="translate(2 3)" opacity="0.5"><rect width="100" height="50" fill="url(#g)" filter="url(#b)"/><use href="#p" x="5" y="6"/></g></svg>"##;let r=import_svg(svg,"v4.svg",&ImportOptions::default());assert!(r.source.contains("viewbox: \"0 0 100 50\";"));assert!(r.source.contains("gradient-spread: \"reflect\";"));assert!(r.source.contains("filter-chain: \"blur(2 2); offset(1 3)\";"));assert!(r.source.contains("use-fill: #ff0000;"));}
#[test]fn promotes_embedded_image_data(){let svg=r#"<svg xmlns="http://www.w3.org/2000/svg"><image x="1" y="2" width="3" height="4" href="data:image/png;base64,iVBORw0KGgo="/></svg>"#;let r=import_svg(svg,"image.svg",&ImportOptions::default());assert!(r.source.contains("image-data:"));assert!(r.source.contains("source-width: \"3\";"));}
}
