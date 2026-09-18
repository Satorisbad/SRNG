use roxmltree::{Document as XmlDocument, Node};
use std::collections::{HashMap, HashSet};

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
struct FilterInfo { chain:String }

#[derive(Default)]
struct Features { viewbox:bool, transforms:bool, opacity:bool, gradients:bool, text:bool, images:bool, uses:bool, filters:bool }

pub fn import_svg(svg:&str,source_name:&str,options:&ImportOptions)->ImportResult{
    let mut result=crate::svg_base::import_svg(svg,source_name,options);
    let features=promote(svg,&mut result.source);
    promote_use_references(svg,&mut result.source,&mut result.diagnostics);
    for diagnostic in &mut result.diagnostics{
        match diagnostic.code.as_str(){
            "S120" if features.viewbox=>info(diagnostic,"viewBox is represented by native SRNG viewbox/preserve-aspect-ratio semantics"),
            "S130" if features.transforms=>info(diagnostic,"SVG transform is represented by native SRNG transform semantics"),
            "S131" if features.opacity&&diagnostic.message.contains("opacity")=>info(diagnostic,"opacity is represented by native SRNG opacity semantics"),
            "S131" if features.filters&&diagnostic.message.contains("filter")=>info(diagnostic,"supported SVG filter primitives are represented by native SRNG filter-chain semantics"),
            "S230" if features.gradients&&diagnostic.message.contains("url(#")=>info(diagnostic,"SVG gradient paint is mapped to native SRNG gradient semantics"),
            "S301" if features.text=>info(diagnostic,"text is represented natively and uses deterministic vector fallback when no outline path is present"),
            "S302" if features.images&&diagnostic.message.contains("<image>")=>info(diagnostic,"embedded data images are represented by native SRNG image properties"),
            "S302" if features.uses&&diagnostic.message.contains("<use>")=>info(diagnostic,"local <use> instances are represented as native SRNG references"),
            _=>{}
        }
    }
    result
}
fn info(diagnostic:&mut ImportDiagnostic,message:&str){diagnostic.severity="info".into();diagnostic.message=message.into();}

fn promote(svg:&str,source:&mut String)->Features{
    let Ok(doc)=XmlDocument::parse(svg)else{return Features::default()};
    let gradients=collect_gradients(&doc);let masks=collect_masks(&doc);let filters=collect_filters(&doc);
    let mut features=Features{viewbox:doc.root_element().attribute("viewBox").is_some(),transforms:doc.descendants().any(|n|n.is_element()&&n.attribute("transform").is_some()),opacity:doc.descendants().any(|n|n.is_element()&&["opacity","fill-opacity","stroke-opacity"].iter().any(|k|n.attribute(*k).is_some())),gradients:false,text:doc.descendants().any(|n|n.is_element()&&matches!(n.tag_name().name(),"text"|"tspan")),images:false,uses:doc.descendants().any(|n|n.is_element()&&n.tag_name().name()=="use"),filters:false};
    let mut out=String::with_capacity(source.len()+source.len()/3);
    for line in source.lines(){
        out.push_str(line);out.push('\n');let trimmed=line.trim_start();let indent=&line[..line.len()-trimmed.len()];
        for(from,to)in[("svg-attr-transform:","transform:"),("svg-attr-opacity:","opacity:"),("svg-attr-fill-opacity:","fill-opacity:"),("svg-attr-stroke-opacity:","stroke-opacity:"),("svg-attr-viewBox:","viewbox:"),("svg-attr-preserveAspectRatio:","preserve-aspect-ratio:"),("svg-attr-clipPathUnits:","clip-units:"),("svg-attr-maskUnits:","mask-units:"),("svg-attr-maskContentUnits:","mask-content-units:"),("svg-attr-font-size:","font-size:"),("svg-attr-font-family:","font-family:"),("svg-attr-font-weight:","font-weight:"),("svg-attr-font-style:","font-style:"),("svg-attr-text-anchor:","text-anchor:"),("svg-attr-letter-spacing:","letter-spacing:"),("svg-attr-word-spacing:","word-spacing:"),("svg-attr-line-height:","line-height:"),("svg-attr-x:","source-x:"),("svg-attr-y:","source-y:"),("svg-attr-width:","source-width:"),("svg-attr-height:","source-height:")]{if let Some(rest)=trimmed.strip_prefix(from){prop_raw(&mut out,indent,to.trim_end_matches(':'),rest.trim());if from=="svg-attr-preserveAspectRatio:"{prop_raw(&mut out,indent,"image-preserve-aspect-ratio",rest.trim());}}}
        if let Some(rest)=trimmed.strip_prefix("svg-attr-href:").or_else(||trimmed.strip_prefix("svg-attr-xlink-href:")){prop_raw(&mut out,indent,"href",rest.trim());if let Some(href)=parse_prop_rest(rest){if href.starts_with("data:image/"){prop(&mut out,indent,"image-data",&quote(&href));features.images=true;}}}
        if let Some(fill)=parse_prop(trimmed,"svg-fill:"){if let Some(id)=url_fragment(&fill){if let Some(gradient)=gradients.get(id){emit_gradient(&mut out,indent,gradient);features.gradients=true;}}}
        if let Some(mask_ref)=parse_prop(trimmed,"mask-ref:").or_else(||parse_prop(trimmed,"svg-attr-mask:")){if let Some(id)=url_fragment(&mask_ref){if let Some(mask)=masks.get(id){prop(&mut out,indent,"mask-units",&quote(&mask.units));prop(&mut out,indent,"mask-content-units",&quote(&mask.content_units));prop(&mut out,indent,"mask-x",&quote(&mask.x));prop(&mut out,indent,"mask-y",&quote(&mask.y));prop(&mut out,indent,"mask-width",&quote(&mask.width));prop(&mut out,indent,"mask-height",&quote(&mask.height));}}}
        if let Some(filter)=parse_prop(trimmed,"svg-attr-filter:").or_else(||parse_prop(trimmed,"svg-filter:")){if let Some(id)=url_fragment(&filter){if let Some(info)=filters.get(id){prop(&mut out,indent,"filter-chain",&quote(&info.chain));features.filters=true;}}}
    }
    *source=out;features
}

fn promote_use_references(svg:&str,source:&mut String,diagnostics:&mut Vec<ImportDiagnostic>){
    let Ok(doc)=XmlDocument::parse(svg)else{return};
    let mut uses=HashMap::new();
    let mut generated=0usize;
    for node in doc.descendants().filter(|n|n.is_element()&&n.tag_name().name()=="use"){
        generated+=1;
        let id=safe_ident(node.attribute("id").unwrap_or(&format!("use_{generated}")));
        let href=node.attribute("href").or_else(||node.attribute(("http://www.w3.org/1999/xlink","href"))).unwrap_or("").trim().to_string();
        uses.insert(id,(node,href));
    }
    if uses.is_empty(){return;}

    let mut blocks=parse_srng_blocks(source);
    for (id,(node,href)) in uses{
        let Some(block)=blocks.iter_mut().find(|b|b.id==id)else{continue};
        if href.is_empty(){diagnostics.push(ImportDiagnostic{severity:"warning".into(),code:"S310".into(),message:"<use> has no href target".into(),element:Some(id.clone())});continue;}
        if !href.starts_with('#'){diagnostics.push(ImportDiagnostic{severity:"warning".into(),code:"S311".into(),message:format!("external <use> target `{href}` is preserved but not resolved"),element:Some(id.clone())});continue;}
        let target=safe_ident(href.trim_start_matches('#'));
        block.header=format!("reference {id} = \"#{target}\" {{");
        block.lines.retain(|line|{
            let t=line.trim_start();
            !t.starts_with("use-data:")&&!t.starts_with("use-fill:")&&!t.starts_with("use-stroke:")
        });
        ensure_prop(&mut block.lines,"position",&format!("{}px {}px",attr_num(node,"x",0.0),attr_num(node,"y",0.0)));
        if node.attribute("width").is_some()||node.attribute("height").is_some(){ensure_prop(&mut block.lines,"size",&format!("{}px {}px",attr_num(node,"width",0.0).max(0.0),attr_num(node,"height",0.0).max(0.0)));}
        ensure_prop(&mut block.lines,"resource-target",&quote(&target));
        ensure_prop(&mut block.lines,"resource-provenance",&quote(&format!("svg#{target}")));
        if let Some(v)=node.attribute("transform"){ensure_prop(&mut block.lines,"transform",&quote(v));}
        if let Some(v)=node.attribute("viewBox"){ensure_prop(&mut block.lines,"viewbox",&quote(v));}
        if let Some(v)=node.attribute("preserveAspectRatio"){ensure_prop(&mut block.lines,"preserve-aspect-ratio",&quote(v));}
        for key in ["fill","stroke","opacity","fill-opacity","stroke-opacity"]{if let Some(v)=node.attribute(key){ensure_prop(&mut block.lines,key,v);}}
    }
    *source=render_srng_blocks(source,&blocks);
}

#[derive(Clone)]struct SrngBlock{id:String,header:String,lines:Vec<String>,start:usize,end:usize}
fn parse_srng_blocks(source:&str)->Vec<SrngBlock>{
    let lines=source.lines().collect::<Vec<_>>();let mut out=Vec::new();let mut i=0usize;
    while i<lines.len(){let t=lines[i].trim();if t.ends_with('{')&&!t.starts_with("relation "){let parts=t.trim_end_matches('{').split_whitespace().collect::<Vec<_>>();if parts.len()>=2{let id=parts[1].to_string();let start=i;let mut j=i+1;while j<lines.len()&&lines[j].trim()!="}"{j+=1;}if j<lines.len(){out.push(SrngBlock{id,header:lines[i].to_string(),lines:lines[i+1..j].iter().map(|v|(*v).to_string()).collect(),start,end:j});i=j+1;continue;}}}i+=1;}out
}
fn render_srng_blocks(source:&str,blocks:&[SrngBlock])->String{
    let mut by_start=HashMap::new();for b in blocks{by_start.insert(b.start,b);}let lines=source.lines().collect::<Vec<_>>();let mut out=String::new();let mut i=0usize;
    while i<lines.len(){if let Some(b)=by_start.get(&i){out.push_str(&b.header);out.push('\n');for l in &b.lines{out.push_str(l);out.push('\n');}out.push_str("}\n");i=b.end+1;}else{out.push_str(lines[i]);out.push('\n');i+=1;}}out
}
fn ensure_prop(lines:&mut Vec<String>,key:&str,value:&str){let prefix=format!("{key}:");if let Some(line)=lines.iter_mut().find(|l|l.trim_start().starts_with(&prefix)){*line=format!("    {key}: {value};");}else{lines.push(format!("    {key}: {value};"));}}
fn safe_ident(raw:&str)->String{let mut out=String::new();for(i,ch)in raw.chars().enumerate(){let valid=if i==0{ch.is_ascii_alphabetic()||ch=='_'||ch=='%'}else{ch.is_ascii_alphanumeric()||matches!(ch,'_'|'-'|'.'|'/'|'%')};out.push(if valid{ch}else{'_'});}if out.is_empty(){"svg_node".into()}else{out}}
fn attr_num(node:Node<'_, '_>,name:&str,default:f64)->f64{node.attribute(name).and_then(parse_number).unwrap_or(default)}

fn emit_gradient(out:&mut String,indent:&str,g:&Gradient){prop(out,indent,"fill",g.kind);prop(out,indent,"gradient-kind",&quote(g.kind));prop(out,indent,"gradient-units",&quote(&g.units));prop(out,indent,"gradient-spread",&quote(&g.spread));if !g.transform.is_empty(){prop(out,indent,"gradient-transform",&quote(&g.transform));}prop(out,indent,"gradient-stops",&g.stops);if g.kind=="linear-gradient"{for(k,v)in[("gradient-x1",&g.x1),("gradient-y1",&g.y1),("gradient-x2",&g.x2),("gradient-y2",&g.y2)]{prop(out,indent,k,&quote(v));}}else{for(k,v)in[("gradient-cx",&g.cx),("gradient-cy",&g.cy),("gradient-r",&g.r),("gradient-fx",&g.fx),("gradient-fy",&g.fy),("gradient-fr",&g.fr)]{prop(out,indent,k,&quote(v));}}}

fn collect_gradients(doc:&XmlDocument<'_>)->HashMap<String,Gradient>{
    let nodes=doc.descendants().filter(|n|n.is_element()).filter_map(|n|{let kind=match n.tag_name().name(){"linearGradient"=>"linear-gradient","radialGradient"=>"radial-gradient",_=>return None};Some((n.attribute("id")?.to_string(),kind,n))}).collect::<Vec<_>>();
    let by_id=nodes.iter().map(|(id,kind,node)|(id.clone(),(*kind,*node))).collect::<HashMap<_,_>>();
    let mut out=HashMap::new();
    for(id,_,_)in &nodes{let mut visiting=HashSet::new();if let Some(g)=resolve_gradient(id,&by_id,&mut out,&mut visiting){out.insert(id.clone(),g);}}
    out
}
fn resolve_gradient<'a>(id:&str,by_id:&HashMap<String,(&'static str,Node<'a,'a>)>,memo:&mut HashMap<String,Gradient>,visiting:&mut HashSet<String>)->Option<Gradient>{
    if let Some(g)=memo.get(id){return Some(g.clone());}
    if !visiting.insert(id.to_string()){return None;}
    let(kind,node)=*by_id.get(id)?;
    let base=node.attribute("href").or_else(||node.attribute(("http://www.w3.org/1999/xlink","href"))).and_then(|h|h.strip_prefix('#')).and_then(|base_id|resolve_gradient(base_id,by_id,memo,visiting));
    let stops=gradient_stops(node);
    let g=Gradient{kind,units:node.attribute("gradientUnits").map(str::to_string).or_else(||base.as_ref().map(|g|g.units.clone())).unwrap_or_else(||"objectBoundingBox".into()),spread:node.attribute("spreadMethod").map(str::to_string).or_else(||base.as_ref().map(|g|g.spread.clone())).unwrap_or_else(||"pad".into()),transform:node.attribute("gradientTransform").map(str::to_string).or_else(||base.as_ref().map(|g|g.transform.clone())).unwrap_or_default(),x1:attr_or(&node,"x1",base.as_ref().map(|g|g.x1.as_str()),"0%"),y1:attr_or(&node,"y1",base.as_ref().map(|g|g.y1.as_str()),"0%"),x2:attr_or(&node,"x2",base.as_ref().map(|g|g.x2.as_str()),"100%"),y2:attr_or(&node,"y2",base.as_ref().map(|g|g.y2.as_str()),"0%"),cx:attr_or(&node,"cx",base.as_ref().map(|g|g.cx.as_str()),"50%"),cy:attr_or(&node,"cy",base.as_ref().map(|g|g.cy.as_str()),"50%"),r:attr_or(&node,"r",base.as_ref().map(|g|g.r.as_str()),"50%"),fx:node.attribute("fx").map(str::to_string).or_else(||base.as_ref().map(|g|g.fx.clone())).unwrap_or_else(||node.attribute("cx").or_else(||base.as_ref().map(|g|g.cx.as_str())).unwrap_or("50%").into()),fy:node.attribute("fy").map(str::to_string).or_else(||base.as_ref().map(|g|g.fy.clone())).unwrap_or_else(||node.attribute("cy").or_else(||base.as_ref().map(|g|g.cy.as_str())).unwrap_or("50%").into()),fr:attr_or(&node,"fr",base.as_ref().map(|g|g.fr.as_str()),"0%"),stops:if stops.is_empty(){base.as_ref().map(|g|g.stops.clone()).unwrap_or_default()}else{stops}};
    visiting.remove(id);memo.insert(id.to_string(),g.clone());Some(g)
}
fn attr_or(node:&Node<'_, '_>,key:&str,inherited:Option<&str>,default:&str)->String{node.attribute(key).or(inherited).unwrap_or(default).to_string()}
fn gradient_stops(gradient:Node<'_, '_>)->String{gradient.children().filter(|n|n.is_element()&&n.tag_name().name()=="stop").filter_map(|stop|{let mut color=stop.attribute("stop-color").unwrap_or("#000000").to_string();let mut opacity=stop.attribute("stop-opacity").and_then(parse_number).unwrap_or(1.0);if let Some(style)=stop.attribute("style"){for field in style.split(';'){if let Some((key,value))=field.split_once(':'){match key.trim(){"stop-color"=>color=value.trim().into(),"stop-opacity"=>opacity=parse_number(value).unwrap_or(opacity),_=>{}}}}}let(r,g,b,a)=color_rgba(&color)?;let alpha=(f64::from(a)*opacity.clamp(0.0,1.0)).round()as u8;Some(format!("{} #{r:02x}{g:02x}{b:02x}{alpha:02x}",normalize_offset(stop.attribute("offset").unwrap_or("0"))))}).collect::<Vec<_>>().join(", ")}

fn collect_filters(doc:&XmlDocument<'_>)->HashMap<String,FilterInfo>{
    doc.descendants().filter(|n|n.is_element()&&n.tag_name().name()=="filter").filter_map(|filter|{let id=filter.attribute("id")?.to_string();let mut ops=Vec::new();for child in filter.children().filter(|n|n.is_element()){match child.tag_name().name(){"feGaussianBlur"=>{let values=child.attribute("stdDeviation").unwrap_or("0").split(|c:char|c==','||c.is_whitespace()).filter(|v|!v.is_empty()).collect::<Vec<_>>();if let Some(x)=values.first(){let y=values.get(1).copied().unwrap_or(x);ops.push(format!("blur({x} {y})"));}},"feOffset"=>{ops.push(format!("offset({} {})",child.attribute("dx").unwrap_or("0"),child.attribute("dy").unwrap_or("0")));},_=>return None}}(!ops.is_empty()).then_some((id,FilterInfo{chain:ops.join("; ")}))}).collect()
}
fn collect_masks(doc:&XmlDocument<'_>)->HashMap<String,MaskInfo>{doc.descendants().filter(|n|n.is_element()&&n.tag_name().name()=="mask").filter_map(|n|Some((n.attribute("id")?.into(),MaskInfo{units:n.attribute("maskUnits").unwrap_or("objectBoundingBox").into(),content_units:n.attribute("maskContentUnits").unwrap_or("userSpaceOnUse").into(),x:n.attribute("x").unwrap_or("-10%").into(),y:n.attribute("y").unwrap_or("-10%").into(),width:n.attribute("width").unwrap_or("120%").into(),height:n.attribute("height").unwrap_or("120%").into()}))).collect()}

fn normalize_offset(v:&str)->String{let v=v.trim();if let Some(p)=v.strip_suffix('%').and_then(parse_number){format!("{}",(p/100.0).clamp(0.0,1.0))}else{format!("{}",parse_number(v).unwrap_or(0.0).clamp(0.0,1.0))}}
fn parse_number(v:&str)->Option<f64>{v.trim().trim_end_matches("px").parse().ok()}
fn url_fragment(v:&str)->Option<&str>{v.trim().strip_prefix("url(#")?.strip_suffix(')')}
fn parse_prop(line:&str,prefix:&str)->Option<String>{Some(unquote(line.strip_prefix(prefix)?.trim().strip_suffix(';')?.trim()))}
fn parse_prop_rest(rest:&str)->Option<String>{Some(unquote(rest.trim().strip_suffix(';')?.trim()))}
fn unquote(v:&str)->String{let v=v.trim();let Some(inner)=v.strip_prefix('"').and_then(|x|x.strip_suffix('"'))else{return v.into()};inner.replace("\\n","\n").replace("\\\"","\"").replace("\\\\","\\")}
fn quote(v:&str)->String{format!("\"{}\"",v.replace('\\',"\\\\").replace('"',"\\\"").replace('\n',"\\n"))}
fn prop(out:&mut String,indent:&str,key:&str,value:&str){out.push_str(indent);out.push_str(key);out.push_str(": ");out.push_str(value);out.push_str(";\n");}
fn prop_raw(out:&mut String,indent:&str,key:&str,rest:&str){out.push_str(indent);out.push_str(key);out.push_str(": ");out.push_str(rest);if !rest.ends_with(';'){out.push(';');}out.push('\n');}
fn color_rgba(value:&str)->Option<(u8,u8,u8,u8)>{let v=value.trim();if v.eq_ignore_ascii_case("transparent"){return Some((0,0,0,0));}let h=v.strip_prefix('#')?;match h.len(){3=>Some((u8::from_str_radix(&h[0..1].repeat(2),16).ok()?,u8::from_str_radix(&h[1..2].repeat(2),16).ok()?,u8::from_str_radix(&h[2..3].repeat(2),16).ok()?,255)),4=>Some((u8::from_str_radix(&h[0..1].repeat(2),16).ok()?,u8::from_str_radix(&h[1..2].repeat(2),16).ok()?,u8::from_str_radix(&h[2..3].repeat(2),16).ok()?,u8::from_str_radix(&h[3..4].repeat(2),16).ok()?)),6=>Some((u8::from_str_radix(&h[0..2],16).ok()?,u8::from_str_radix(&h[2..4],16).ok()?,u8::from_str_radix(&h[4..6],16).ok()?,255)),8=>Some((u8::from_str_radix(&h[0..2],16).ok()?,u8::from_str_radix(&h[2..4],16).ok()?,u8::from_str_radix(&h[4..6],16).ok()?,u8::from_str_radix(&h[6..8],16).ok()?)),_=>None}}

#[cfg(test)]mod tests{
use super::*;
#[test]fn promotes_core_v4_semantics(){let svg=r##"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="100" viewBox="0 0 100 50"><defs><linearGradient id="derived" href="#base" spreadMethod="reflect"/><linearGradient id="base"><stop offset="0" stop-color="#f00"/><stop offset="100%" stop-color="#00f"/></linearGradient><path id="p" d="M0 0 L10 0 L10 10 Z" fill="#ff0000"/><use id="nested" href="#p"/><filter id="b"><feGaussianBlur stdDeviation="2"/><feOffset dx="1" dy="3"/></filter></defs><g transform="translate(2 3)" opacity="0.5"><rect width="100" height="50" fill="url(#derived)" filter="url(#b)"/><use href="#nested" x="5" y="6"/></g></svg>"##;let r=import_svg(svg,"v4.svg",&ImportOptions::default());assert!(r.source.contains("viewbox: \"0 0 100 50\";"));assert!(r.source.contains("gradient-spread: \"reflect\";"));assert!(r.source.contains("gradient-stops: 0 #ff0000ff, 1 #0000ffff;"));assert!(r.source.contains("filter-chain: \"blur(2 2); offset(1 3)\";"));assert!(r.source.contains("reference nested = \"#p\""));}
#[test]fn promotes_embedded_image_data(){let svg=r#"<svg xmlns="http://www.w3.org/2000/svg"><image x="1" y="2" width="3" height="4" href="data:image/png;base64,iVBORw0KGgo="/></svg>"#;let r=import_svg(svg,"image.svg",&ImportOptions::default());assert!(r.source.contains("image-data:"));assert!(r.source.contains("source-width: \"3\";"));}
#[test]fn resolves_forward_gradient_inheritance(){let svg=r##"<svg xmlns="http://www.w3.org/2000/svg"><defs><linearGradient id="child" href="#parent"/><linearGradient id="parent"><stop offset="0" stop-color="#fff"/><stop offset="1" stop-color="#000"/></linearGradient></defs><rect width="10" height="10" fill="url(#child)"/></svg>"##;let r=import_svg(svg,"gradient.svg",&ImportOptions::default());assert!(r.source.contains("gradient-stops: 0 #ffffffff, 1 #000000ff;"));}
#[test]fn imports_nested_use_as_reference(){let svg=r##"<svg xmlns="http://www.w3.org/2000/svg"><defs><rect id="base" width="3" height="4" fill="#f00"/><use id="alias" href="#base"/></defs><use id="instance" href="#alias" x="5" y="6"/></svg>"##;let r=import_svg(svg,"use.svg",&ImportOptions::default());assert!(r.source.contains("reference alias = \"#base\""));assert!(r.source.contains("reference instance = \"#alias\""));assert!(r.source.contains("position: 5px 6px;"));}
}