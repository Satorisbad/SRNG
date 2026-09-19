use crate::{PreparedScene,RevisionGate,RenderDiagnostic};
use srng::runtime::Scene;

/// Native image normalization and validation layer.
/// All accepted embedded formats are decoded under bounded limits, then
/// normalized to a backend-neutral PNG data resource so CPU and GPU consume
/// identical image content without codec-specific backend behavior.
pub fn prepare_scene(scene:&Scene,revision:u64,gate:&RevisionGate)->PreparedScene{
    let mut normalized=scene.clone();let mut image_diagnostics=Vec::new();
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
    image-data: "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAFgwJ/lC9pWQAAAABJRU5ErkJggg==";
}
"#;let ir=srng::compile_to_json(source,"native-image.srng");let scene=execute_json(&ir,&RuntimeOptions::default()).unwrap();let gate=RevisionGate::default();let revision=gate.begin();let prepared=prepare_scene(&scene,revision,&gate);assert!(prepared.commands.iter().any(|c|matches!(c,crate::Command::DrawImage{image}if image.width==8.0&&image.height==9.0)),"{:?}",prepared.diagnostics);}#[test]fn external_image_is_rejected_without_rendering_placeholder(){let source=r#"
srng 0.1;
group image {
    position: 0px 0px;
    size: 10px 10px;
    href: "https://example.com/a.png";
}
"#;let ir=srng::compile_to_json(source,"external.srng");let scene=execute_json(&ir,&RuntimeOptions::default()).unwrap();let gate=RevisionGate::default();let revision=gate.begin();let prepared=prepare_scene(&scene,revision,&gate);assert!(prepared.diagnostics.iter().any(|d|d.code=="G260"));assert!(!prepared.commands.iter().any(|c|matches!(c,crate::Command::DrawImage{..})));}}
