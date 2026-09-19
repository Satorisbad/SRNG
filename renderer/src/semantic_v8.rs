use crate::{PreparedScene,RevisionGate,RenderDiagnostic};
use kurbo::{Affine,BezPath};
use srng::runtime::Scene;

/// Final v0.6 normalization layer for native resources.
pub fn prepare_scene(scene:&Scene,revision:u64,gate:&RevisionGate)->PreparedScene{
    let mut normalized=scene.clone();let mut image_diagnostics=Vec::new();
    normalize_direct_references(&mut normalized);
    for node in &mut normalized.nodes{
        let raw=node.properties.get("image-data").or_else(||node.properties.get("href")).map(|v|unquote(v));let Some(raw)=raw else{continue};
        let image_like=node.kind=="group"||node.kind=="image"||node.properties.contains_key("image-data");if !image_like{continue;}
        if !raw.starts_with("data:"){
            node.active=false;image_diagnostics.push(diag("error","G260","external/network image references are disabled; only data: images are accepted",&node.id));continue;
        }
        match crate::image::decode_data_uri(&raw).and_then(|decoded|crate::image::png_data_uri(&decoded)){
            Ok(png)=>{node.properties.insert("image-data".into(),quote(&png));node.properties.remove("href");bridge_geometry(node);}
            Err(message)=>{node.active=false;image_diagnostics.push(diag("error","G260",&message,&node.id));}
        }
    }
    let mut prepared=crate::semantic_v7::prepare_scene(&normalized,revision,gate);prepared.diagnostics.extend(image_diagnostics);prepared
}

/// Direct `<use>` references keep source-element paint precedence: a paint
/// explicitly present on the referenced element wins over inherited paint on
/// the instance. Instance x/y are geometry, so apply that translation to path
/// data before the lower renderer path stage consumes the reference.
fn normalize_direct_references(scene:&mut Scene){
    for reference in &mut scene.references{
        if !reference.resolved||!reference.resolved_nodes.is_empty(){continue;}
        for key in ["fill","stroke"]{
            let linked=reference.linked_properties.get(key).map(|v|unquote(v));
            if linked.as_deref().is_some_and(|v|!v.eq_ignore_ascii_case("none")){reference.properties.remove(key);}
        }
        let dx=reference.geometry.x.unwrap_or(0.0);let dy=reference.geometry.y.unwrap_or(0.0);
        if dx==0.0&&dy==0.0{continue;}
        let data=reference.properties.get("data").or_else(||reference.linked_properties.get("data")).map(|v|unquote(v));
        if let Some(data)=data{if let Ok(mut path)=BezPath::from_svg(&data){path.apply_affine(Affine::translate((dx,dy)));reference.properties.insert("data".into(),quote(&path.to_svg()));}}
    }
}
fn bridge_geometry(node:&mut srng::runtime::SceneNode){let authored_position=node.properties.get("position").and_then(|v|parse_pair(v));let authored_size=node.properties.get("size").and_then(|v|parse_pair(v));let x=node.geometry.x.or_else(||authored_position.map(|p|p.0));let y=node.geometry.y.or_else(||authored_position.map(|p|p.1));let w=node.geometry.width.or_else(||authored_size.map(|p|p.0));let h=node.geometry.height.or_else(||authored_size.map(|p|p.1));for(key,value)in[("source-x",x),("source-y",y),("source-width",w),("source-height",h)]{if !node.properties.contains_key(key){if let Some(v)=value{node.properties.insert(key.into(),fmt(v));}}}}
fn parse_pair(value:&str)->Option<(f64,f64)>{let value=unquote(value);let mut values=value.split(|c:char|c.is_whitespace()||c==',').filter(|p|!p.is_empty()).filter_map(|p|p.trim_end_matches("px").parse::<f64>().ok());Some((values.next()?,values.next()?))}
fn unquote(value:&str)->String{let value=value.trim();value.strip_prefix('"').and_then(|v|v.strip_suffix('"')).unwrap_or(value).to_string()}
fn quote(value:&str)->String{format!("\"{}\"",value.replace('\\',"\\\\").replace('"',"\\\""))}
fn fmt(value:f64)->String{if value.fract().abs()<1e-9{format!("{}",value as i64)}else{format!("{value:.6}").trim_end_matches('0').trim_end_matches('.').to_string()}}
fn diag(severity:&str,code:&str,message:&str,id:&str)->RenderDiagnostic{RenderDiagnostic{severity:severity.into(),code:code.into(),message:message.into(),declaration:Some(id.into())}}
#[cfg(test)]mod tests{use super::*;use srng::runtime::{execute_json,RuntimeOptions};#[test]fn native_image_geometry_survives_normalization(){let source=r#"
srng 0.1;
group image {
    position: 2px 3px;
    size: 8px 9px;
    image-data: "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=";
}
"#;let ir=srng::compile_to_json(source,"native-image.srng");let scene=execute_json(&ir,&RuntimeOptions::default()).unwrap();let gate=RevisionGate::default();let revision=gate.begin();let prepared=prepare_scene(&scene,revision,&gate);assert!(prepared.commands.iter().any(|c|matches!(c,crate::Command::DrawImage{image}if image.width==8.0&&image.height==9.0)),"{:?}",prepared.diagnostics);}#[test]fn external_image_is_rejected_without_rendering_placeholder(){let source=r#"
srng 0.1;
group image {
    position: 0px 0px;
    size: 10px 10px;
    href: "https://example.com/a.png";
}
"#;let ir=srng::compile_to_json(source,"external.srng");let scene=execute_json(&ir,&RuntimeOptions::default()).unwrap();let gate=RevisionGate::default();let revision=gate.begin();let prepared=prepare_scene(&scene,revision,&gate);assert!(prepared.diagnostics.iter().any(|d|d.code=="G260"));assert!(!prepared.commands.iter().any(|c|matches!(c,crate::Command::DrawImage{..})));}}
