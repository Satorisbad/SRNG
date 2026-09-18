use roxmltree::{Document as XmlDocument, Node};
use std::collections::HashMap;

pub use crate::svg_final::{ImportDiagnostic, ImportOptions, ImportResult};
pub use crate::svg_final::strip_svg_provenance;

#[derive(Clone, Debug)]
struct FilterInfo {
    graph: String,
    units: String,
    primitive_units: String,
    x: String,
    y: String,
    width: String,
    height: String,
    unsupported: Vec<String>,
}

pub fn import_svg(svg:&str,source_name:&str,options:&ImportOptions)->ImportResult{
    let mut result=crate::svg_final::import_svg(svg,source_name,options);
    let Ok(document)=XmlDocument::parse(svg) else{return result};
    let filters=collect_filters(&document);
    promote_filters(&mut result.source,&filters);
    for filter in filters.values(){
        for primitive in &filter.unsupported{
            result.diagnostics.push(ImportDiagnostic{
                severity:"warning".into(),
                code:"S360".into(),
                message:format!("unsupported filter primitive <{primitive}> is retained as an explicit native filter-graph node and will be diagnosed and bypassed by the CPU renderer"),
                element:None,
            });
        }
    }
    if !filters.is_empty(){
        for diagnostic in &mut result.diagnostics{
            if diagnostic.code=="S131"&&diagnostic.message.contains("filter"){
                diagnostic.severity="info".into();
                diagnostic.message="SVG filter is represented by native SRNG filter-graph semantics".into();
            }
        }
    }
    result
}

fn promote_filters(source:&mut String,filters:&HashMap<String,FilterInfo>){
    let mut out=String::with_capacity(source.len()+source.len()/8);
    for line in source.lines(){
        out.push_str(line);out.push('\n');
        let trimmed=line.trim_start();let indent=&line[..line.len()-trimmed.len()];
        let raw=property_string(trimmed,"svg-attr-filter:").or_else(||property_string(trimmed,"svg-filter:"));
        let Some(raw)=raw else{continue};
        let Some(id)=url_fragment(&raw) else{continue};
        let Some(filter)=filters.get(id) else{continue};
        emit(out.as_mut(),indent,"filter-graph",&quote(&filter.graph));
        emit(out.as_mut(),indent,"filter-units",&quote(&filter.units));
        emit(out.as_mut(),indent,"filter-primitive-units",&quote(&filter.primitive_units));
        emit(out.as_mut(),indent,"filter-x",&quote(&filter.x));
        emit(out.as_mut(),indent,"filter-y",&quote(&filter.y));
        emit(out.as_mut(),indent,"filter-width",&quote(&filter.width));
        emit(out.as_mut(),indent,"filter-height",&quote(&filter.height));
    }
    *source=out;
}

fn emit(out:&mut String,indent:&str,key:&str,value:&str){out.push_str(indent);out.push_str(key);out.push_str(": ");out.push_str(value);out.push_str(";\n");}

fn collect_filters(document:&XmlDocument<'_>)->HashMap<String,FilterInfo>{
    let mut filters=HashMap::new();
    for filter in document.descendants().filter(|n|n.is_element()&&n.tag_name().name()=="filter"){
        let Some(id)=filter.attribute("id") else{continue};
        let mut statements=Vec::new();let mut unsupported=Vec::new();let mut generated=0usize;
        for primitive in filter.children().filter(|n|n.is_element()){
            generated+=1;
            let result=primitive.attribute("result").map(str::to_string).unwrap_or_else(||format!("_filter_{generated}"));
            let input=primitive.attribute("in").map(|v|format!(" in={v}")).unwrap_or_default();
            let statement=match primitive.tag_name().name(){
                "feGaussianBlur"=>{
                    let values=number_tokens(primitive.attribute("stdDeviation").unwrap_or("0"));
                    let sx=values.first().cloned().unwrap_or_else(||"0".into());let sy=values.get(1).cloned().unwrap_or_else(||sx.clone());
                    format!("feGaussianBlur({input} sigma_x={sx} sigma_y={sy})->{result}")
                }
                "feOffset"=>format!("feOffset({input} dx={} dy={})->{result}",primitive.attribute("dx").unwrap_or("0"),primitive.attribute("dy").unwrap_or("0")),
                "feBlend"=>format!("feBlend({input}{} mode={})->{result}",primitive.attribute("in2").map(|v|format!(" in2={v}")).unwrap_or_default(),primitive.attribute("mode").unwrap_or("normal")),
                "feComposite"=>{
                    let op=primitive.attribute("operator").unwrap_or("over");
                    let coeff=if op=="arithmetic"{format!(" k1={} k2={} k3={} k4={}",primitive.attribute("k1").unwrap_or("0"),primitive.attribute("k2").unwrap_or("0"),primitive.attribute("k3").unwrap_or("0"),primitive.attribute("k4").unwrap_or("0"))}else{String::new()};
                    format!("feComposite({input}{} operator={op}{coeff})->{result}",primitive.attribute("in2").map(|v|format!(" in2={v}")).unwrap_or_default())
                }
                "feColorMatrix"=>{
                    let matrix=color_matrix_values(primitive);
                    format!("feColorMatrix({input} values={})->{result}",matrix.iter().map(|v|fmt(*v)).collect::<Vec<_>>().join("|"))
                }
                "feFlood"=>{
                    let color=flood_color(primitive);
                    format!("feFlood(color={color})->{result}")
                }
                "feMerge"=>{
                    let inputs=primitive.children().filter(|n|n.is_element()&&n.tag_name().name()=="feMergeNode").filter_map(|n|n.attribute("in")).collect::<Vec<_>>();
                    if inputs.is_empty(){format!("feMerge(SourceGraphic)->{result}")}else{format!("feMerge({})->{result}",inputs.join(" "))}
                }
                "feMorphology"=>{
                    let radius=number_tokens(primitive.attribute("radius").unwrap_or("0"));let rx=radius.first().cloned().unwrap_or_else(||"0".into());let ry=radius.get(1).cloned().unwrap_or_else(||rx.clone());
                    format!("feMorphology({input} operator={} radius_x={rx} radius_y={ry})->{result}",primitive.attribute("operator").unwrap_or("erode"))
                }
                "feComponentTransfer"=>{
                    let mut fields=Vec::new();
                    for(child,key)in[("feFuncR","r"),("feFuncG","g"),("feFuncB","b"),("feFuncA","a")]{if let Some(function)=primitive.children().find(|n|n.is_element()&&n.tag_name().name()==child){fields.push(format!("{key}={}",transfer_function(function)));}}
                    format!("feComponentTransfer({input} {})->{result}",fields.join(" "))
                }
                other=>{unsupported.push(other.to_string());format!("{other}({input})->{result}")}
            };
            statements.push(statement);
        }
        if statements.is_empty(){continue;}
        filters.insert(id.to_string(),FilterInfo{graph:statements.join("; "),units:filter.attribute("filterUnits").unwrap_or("objectBoundingBox").into(),primitive_units:filter.attribute("primitiveUnits").unwrap_or("userSpaceOnUse").into(),x:filter.attribute("x").unwrap_or("-10%").into(),y:filter.attribute("y").unwrap_or("-10%").into(),width:filter.attribute("width").unwrap_or("120%").into(),height:filter.attribute("height").unwrap_or("120%").into(),unsupported});
    }
    filters
}

fn color_matrix_values(node:Node<'_, '_>)->[f64;20]{
    let kind=node.attribute("type").unwrap_or("matrix");
    match kind{
        "saturate"=>{let s=node.attribute("values").and_then(parse_f64).unwrap_or(1.0);[0.213+0.787*s,0.715-0.715*s,0.072-0.072*s,0.0,0.0,0.213-0.213*s,0.715+0.285*s,0.072-0.072*s,0.0,0.0,0.213-0.213*s,0.715-0.715*s,0.072+0.928*s,0.0,0.0,0.0,0.0,0.0,1.0,0.0]},
        "luminanceToAlpha"=>[0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.2125,0.7154,0.0721,0.0,0.0],
        "hueRotate"=>hue_rotate(node.attribute("values").and_then(parse_f64).unwrap_or(0.0)),
        _=>{let values=number_tokens(node.attribute("values").unwrap_or("1 0 0 0 0 0 1 0 0 0 0 0 1 0 0 0 0 0 1 0")).into_iter().filter_map(|v|v.parse::<f64>().ok()).collect::<Vec<_>>();let mut out=[0.0;20];if values.len()==20{out.copy_from_slice(&values)}else{out=[1.0,0.0,0.0,0.0,0.0,0.0,1.0,0.0,0.0,0.0,0.0,0.0,1.0,0.0,0.0,0.0,0.0,0.0,1.0,0.0]}out}
    }
}
fn hue_rotate(degrees:f64)->[f64;20]{let(a,b)=(degrees.to_radians().cos(),degrees.to_radians().sin());[0.213+0.787*a-0.213*b,0.715-0.715*a-0.715*b,0.072-0.072*a+0.928*b,0.0,0.0,0.213-0.213*a+0.143*b,0.715+0.285*a+0.140*b,0.072-0.072*a-0.283*b,0.0,0.0,0.213-0.213*a-0.787*b,0.715-0.715*a+0.715*b,0.072+0.928*a+0.072*b,0.0,0.0,0.0,0.0,0.0,1.0,0.0]}
fn transfer_function(node:Node<'_, '_>)->String{match node.attribute("type").unwrap_or("identity"){"table"=>format!("table:{}",number_tokens(node.attribute("tableValues").unwrap_or("")).join("|")),"discrete"=>format!("discrete:{}",number_tokens(node.attribute("tableValues").unwrap_or("")).join("|")),"linear"=>format!("linear:{}|{}",node.attribute("slope").unwrap_or("1"),node.attribute("intercept").unwrap_or("0")),"gamma"=>format!("gamma:{}|{}|{}",node.attribute("amplitude").unwrap_or("1"),node.attribute("exponent").unwrap_or("1"),node.attribute("offset").unwrap_or("0")),_=>"identity".into()}}
fn flood_color(node:Node<'_, '_>)->String{let mut color=node.attribute("flood-color").unwrap_or("#000000").to_string();let mut opacity=node.attribute("flood-opacity").and_then(parse_f64).unwrap_or(1.0);if let Some(style)=node.attribute("style"){for field in style.split(';'){if let Some((key,value))=field.split_once(':'){match key.trim(){"flood-color"=>color=value.trim().into(),"flood-opacity"=>opacity=parse_f64(value).unwrap_or(opacity),_=>{}}}}}let(mut r,mut g,mut b,mut a)=normalize_color(&color);a=((f64::from(a)*opacity.clamp(0.0,1.0)).round())as u8;format!("#{r:02x}{g:02x}{b:02x}{a:02x}")}
fn normalize_color(value:&str)->(u8,u8,u8,u8){let v=value.trim();if v.eq_ignore_ascii_case("transparent"){return(0,0,0,0)};for(name,c)in[("black",(0,0,0,255)),("white",(255,255,255,255)),("red",(255,0,0,255)),("green",(0,128,0,255)),("blue",(0,0,255,255))]{if v.eq_ignore_ascii_case(name){return c}}let Some(h)=v.strip_prefix('#')else{return(0,0,0,255)};let byte=|s:&str|u8::from_str_radix(s,16).unwrap_or(0);match h.len(){3=>(byte(&h[0..1].repeat(2)),byte(&h[1..2].repeat(2)),byte(&h[2..3].repeat(2)),255),4=>(byte(&h[0..1].repeat(2)),byte(&h[1..2].repeat(2)),byte(&h[2..3].repeat(2)),byte(&h[3..4].repeat(2))),6=>(byte(&h[0..2]),byte(&h[2..4]),byte(&h[4..6]),255),8=>(byte(&h[0..2]),byte(&h[2..4]),byte(&h[4..6]),byte(&h[6..8])),_=>(0,0,0,255)}}
fn number_tokens(value:&str)->Vec<String>{value.split(|c:char|c==','||c.is_whitespace()).filter(|v|!v.is_empty()).map(str::to_string).collect()}
fn parse_f64(value:&str)->Option<f64>{value.trim().parse().ok()}
fn property_string(line:&str,prefix:&str)->Option<String>{let value=line.strip_prefix(prefix)?.trim().strip_suffix(';')?.trim();Some(unquote(value))}
fn unquote(value:&str)->String{let value=value.trim();let Some(inner)=value.strip_prefix('"').and_then(|v|v.strip_suffix('"'))else{return value.into()};inner.replace("\\n","\n").replace("\\\"","\"").replace("\\\\","\\")}
fn url_fragment(value:&str)->Option<&str>{value.trim().strip_prefix("url(#")?.strip_suffix(')')}
fn quote(value:&str)->String{format!("\"{}\"",value.replace('\\',"\\\\").replace('"',"\\\"").replace('\n',"\\n"))}
fn fmt(value:f64)->String{if value.fract().abs()<1e-9{format!("{}",value as i64)}else{format!("{value:.9}").trim_end_matches('0').trim_end_matches('.').to_string()}}

#[cfg(test)]mod tests{use super::*;#[test]fn imports_named_results_and_regions(){let svg=r#"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20"><defs><filter id="f" x="0" y="0" width="20" height="20" filterUnits="userSpaceOnUse"><feGaussianBlur in="SourceAlpha" stdDeviation="2" result="b"/><feOffset in="b" dx="2" result="o"/><feMerge result="m"><feMergeNode in="o"/><feMergeNode in="SourceGraphic"/></feMerge></filter></defs><rect width="10" height="10" filter="url(#f)"/></svg>"#;let result=import_svg(svg,"f.svg",&ImportOptions::default());assert!(result.source.contains("filter-graph:"));assert!(result.source.contains("SourceAlpha"));assert!(result.source.contains("->b"));assert!(result.source.contains("filter-units: \"userSpaceOnUse\""));}#[test]fn unsupported_filter_is_retained_and_diagnosed(){let svg=r#"<svg xmlns="http://www.w3.org/2000/svg"><defs><filter id="f"><feTurbulence result="n"/></filter></defs><rect width="1" height="1" filter="url(#f)"/></svg>"#;let result=import_svg(svg,"u.svg",&ImportOptions::default());assert!(result.source.contains("feTurbulence"));assert!(result.diagnostics.iter().any(|d|d.code=="S360"));}}
