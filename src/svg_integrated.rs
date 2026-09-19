use roxmltree::{Document as XmlDocument, Node};
use std::collections::HashMap;

pub use crate::svg_filter_v06::{strip_svg_provenance, ImportDiagnostic, ImportOptions, ImportResult};

#[derive(Clone)]
struct PatternMeta { width:f64, height:f64, source:String, object_content:bool }

pub fn import_svg(svg: &str, source_name: &str, options: &ImportOptions) -> ImportResult {
    let mut result = crate::svg_filter_v06::import_svg(svg, source_name, options);
    normalize_integrated_resources(svg, &mut result.source);
    result
}

fn normalize_integrated_resources(svg: &str, source: &mut String) {
    let Ok(document) = XmlDocument::parse(svg) else { return; };
    let mut fills = HashMap::<String, String>::new();
    let mut generated = 0usize;
    for node in document.descendants().filter(|n| n.is_element() && n.tag_name().name() == "use") {
        generated += 1;
        let fallback = format!("use_{generated}");
        fills.insert(safe_ident(node.attribute("id").unwrap_or(&fallback)), computed_fill(node));
    }
    let patterns = document.descendants().filter(|n| n.is_element() && n.tag_name().name() == "pattern").filter_map(|node| {
        let id = node.attribute("id")?.to_string();
        let width = pattern_dimension(node.attribute("width")?)?;
        let height = pattern_dimension(node.attribute("height")?)?;
        let source_xml = standalone_xml(&svg[node.range()]);
        let object_content = node.attribute("patternContentUnits") == Some("objectBoundingBox");
        Some((id, PatternMeta { width, height, source:source_xml, object_content }))
    }).collect::<HashMap<_, _>>();

    let lines = source.lines().collect::<Vec<_>>();
    let mut direct_paths = HashMap::<String, String>::new();
    let mut scan = 0usize;
    while scan < lines.len() {
        let trimmed = lines[scan].trim();
        if trimmed.ends_with('{') && !trimmed.starts_with("relation ") {
            let id = trimmed.trim_end_matches('{').split_whitespace().nth(1).unwrap_or("").to_string();
            let mut block = Vec::<String>::new();
            scan += 1;
            while scan < lines.len() && lines[scan].trim() != "}" { block.push(lines[scan].to_string()); scan += 1; }
            if let Some(data) = property_value(&block, "data") { direct_paths.insert(id, data); }
        }
        scan += 1;
    }

    let mut out = String::with_capacity(source.len() + 512);
    let mut i = 0usize;
    while i < lines.len() {
        let trimmed = lines[i].trim();
        if trimmed.ends_with('{') && !trimmed.starts_with("relation ") {
            let mut header = lines[i].to_string();
            let mut block = Vec::<String>::new();
            i += 1;
            while i < lines.len() && lines[i].trim() != "}" { block.push(lines[i].to_string()); i += 1; }

            // Some lower importer paths still represent <use> as a zero-sized group with href.
            // Promote that representation here to the same first-class reference model used by
            // resource_reference, without flattening referenced group/symbol contents.
            let local_href = property_value(&block, "href").map(|v| v.trim_matches('"').to_string()).filter(|v| v.starts_with('#'));
            if !trimmed.starts_with("reference ") {
                if let Some(target) = local_href.as_deref() {
                    let id = trimmed.trim_end_matches('{').split_whitespace().nth(1).unwrap_or("use");
                    header = format!("reference {id} = \"{target}\" {{");
                    let x = property_value(&block, "source-x").and_then(|v| parse_dimension(&v)).unwrap_or(0.0);
                    let y = property_value(&block, "source-y").and_then(|v| parse_dimension(&v)).unwrap_or(0.0);
                    set_property(&mut block, "position", &format!("{}px {}px", format_number(x), format_number(y)));
                    block.retain(|line| {
                        let t=line.trim_start();
                        !t.starts_with("href:") && !t.starts_with("source-x:") && !t.starts_with("source-y:") && !t.starts_with("size: 0px 0px;")
                    });
                }
            }

            let normalized_header = header.trim().to_string();
            if let Some(rest) = normalized_header.strip_prefix("reference ") {
                let id = rest.split_whitespace().next().unwrap_or("");
                if let Some(fill) = fills.get(id) { set_property(&mut block, "fill", fill); }
                let target = normalized_header.split('"').nth(1).map(|v|v.trim_start_matches('#').to_string()).or_else(||property_value(&block,"resource-target").map(|v|v.trim_matches('"').trim_start_matches('#').to_string()));
                if let Some(target) = target { if let Some(data) = direct_paths.get(&target) { set_property(&mut block, "data", data); } }
            }

            if let Some(pattern_id) = property_value(&block, "pattern-ref").or_else(|| property_value(&block, "svg-pattern-ref")) {
                let id = pattern_id.trim_matches('"').trim().trim_start_matches("url(#").trim_end_matches(')');
                if let Some(meta) = patterns.get(id) {
                    let width = meta.width.to_string(); let height = meta.height.to_string();
                    set_property(&mut block, "pattern-width", &width); set_property(&mut block, "pattern-height", &height);
                    set_property(&mut block, "svg-pattern-width", &width); set_property(&mut block, "svg-pattern-height", &height);
                    let quoted = quote_srng(&meta.source); set_property(&mut block, "pattern-source", &quoted); set_property(&mut block, "svg-pattern-source-xml", &quoted);
                    if meta.object_content { block.retain(|line| !line.trim_start().starts_with("pattern-data:")); }
                }
            }
            for line in &mut block { if line.contains("xlink:") && !line.contains("xmlns:xlink") && (line.contains("<defs") || line.contains("<pattern")) { *line = inject_xlink_namespace_in_escaped_property(line); } }
            let binary_mask = property_value(&block, "mask-mode").or_else(|| property_value(&block, "svg-mask-mode")).is_some_and(|v| v.trim_matches('"') == "binary-opaque-clip");
            if binary_mask { block.retain(|line| { let t = line.trim_start(); !t.starts_with("mask-data:") && !t.starts_with("mask-type:") }); }

            out.push_str(&header); out.push('\n'); for line in block { out.push_str(&line); out.push('\n'); }
            if i < lines.len() { out.push_str(lines[i]); out.push('\n'); i += 1; }
            continue;
        }
        out.push_str(lines[i]); out.push('\n'); i += 1;
    }
    *source = out;
}

fn standalone_xml(xml:&str)->String { if xml.contains("xlink:") && !xml.contains("xmlns:xlink") { if let Some(pos)=xml.find('>') { let mut out=String::with_capacity(xml.len()+50); out.push_str(&xml[..pos]); out.push_str(" xmlns:xlink=\"http://www.w3.org/1999/xlink\""); out.push_str(&xml[pos..]); return out; } } xml.to_string() }
fn inject_xlink_namespace_in_escaped_property(line:&str)->String { if let Some(pos)=line.find("<defs") { let insert=pos+5; let mut out=line.to_string(); out.insert_str(insert," xmlns:xlink=\\\"http://www.w3.org/1999/xlink\\\""); return out; } if let Some(pos)=line.find("<pattern") { if let Some(end)=line[pos..].find('>'){ let insert=pos+end; let mut out=line.to_string(); out.insert_str(insert," xmlns:xlink=\\\"http://www.w3.org/1999/xlink\\\""); return out; } } line.to_string() }
fn quote_srng(value:&str)->String { format!("\"{}\"", value.replace('\\',"\\\\").replace('"',"\\\"").replace('\n',"\\n").replace('\r',"\\r")) }
fn property_value(lines:&[String],key:&str)->Option<String>{let prefix=format!("{key}:");lines.iter().find_map(|line|{let value=line.trim_start().strip_prefix(&prefix)?.trim().strip_suffix(';')?.trim();Some(value.to_string())})}
fn set_property(lines:&mut Vec<String>,key:&str,value:&str){let prefix=format!("{key}:");if let Some(line)=lines.iter_mut().find(|line|line.trim_start().starts_with(&prefix)){*line=format!("    {key}: {value};");}else{lines.push(format!("    {key}: {value};"));}}
fn pattern_dimension(value:&str)->Option<f64>{let value=value.trim();if let Some(percent)=value.strip_suffix('%'){return percent.trim().parse::<f64>().ok().map(|v|v/100.0);}value.trim_end_matches("px").parse::<f64>().ok().filter(|v|v.is_finite()&&*v>0.0)}
fn parse_dimension(value:&str)->Option<f64>{value.trim_matches('"').trim().trim_end_matches("px").parse().ok()}
fn format_number(value:f64)->String{if value.fract().abs()<1e-9{format!("{}",value as i64)}else{format!("{value:.6}").trim_end_matches('0').trim_end_matches('.').to_string()}}
fn computed_fill(node:Node<'_,'_>)->String{for current in node.ancestors().filter(|n|n.is_element()){if let Some(value)=style_property(current,"fill"){return normalize_fill(&value);}if let Some(value)=current.attribute("fill"){return normalize_fill(value);}}"#000000".into()}
fn style_property(node:Node<'_,'_>,key:&str)->Option<String>{node.attribute("style")?.split(';').find_map(|entry|{let(name,value)=entry.split_once(':')?;(name.trim()==key).then(||value.trim().to_string())})}
fn normalize_fill(value:&str)->String{let value=value.trim();match value.to_ascii_lowercase().as_str(){"black"=>"#000000".into(),"white"=>"#ffffff".into(),"red"=>"#ff0000".into(),"green"=>"#008000".into(),"blue"=>"#0000ff".into(),_=>value.to_string()}}
fn safe_ident(raw:&str)->String{let mut out=String::new();for(i,ch)in raw.chars().enumerate(){let valid=if i==0{ch.is_ascii_alphabetic()||ch=='_'||ch=='%'}else{ch.is_ascii_alphanumeric()||matches!(ch,'_'|'-'|'.'|'/'|'%')};out.push(if valid{ch}else{'_'});}if out.is_empty(){"svg_node".into()}else{out}}

#[cfg(test)] mod tests { use super::*;
#[test]fn use_without_fill_gets_svg_default_black(){let svg="<svg xmlns='http://www.w3.org/2000/svg'><defs><g id='item'><rect width='3' height='4'/></g></defs><use id='plain' href='#item'/></svg>";let result=import_svg(svg,"test.svg",&ImportOptions::default());let block=result.source.split("reference plain").nth(1).unwrap_or("");assert!(block.split('}').next().unwrap_or("").contains("fill: #000000;"),"{}",result.source);}
#[test]fn explicit_none_is_not_replaced(){let svg="<svg xmlns='http://www.w3.org/2000/svg'><defs><rect id='item' width='1' height='1'/></defs><use id='plain' href='#item' fill='none'/></svg>";let result=import_svg(svg,"test.svg",&ImportOptions::default());let block=result.source.split("reference plain").nth(1).unwrap_or("");assert!(block.split('}').next().unwrap_or("").contains("fill: none;"),"{}",result.source);}
#[test]fn remaining_legacy_use_is_promoted_to_reference(){let svg="<svg xmlns='http://www.w3.org/2000/svg'><path id='p' d='M0 0 L2 0 L2 2 Z'/><use href='#p' x='7' y='9'/></svg>";let result=import_svg(svg,"test.svg",&ImportOptions::default());assert!(result.source.contains("reference use_"),"{}",result.source);assert!(result.source.contains("= \"#p\""),"{}",result.source);assert!(result.source.contains("position: 7px 9px;"),"{}",result.source);}
#[test]fn direct_path_reference_keeps_instance_path_without_flattening_group_resources(){let svg="<svg xmlns='http://www.w3.org/2000/svg'><defs><path id='p' d='M0 0 L2 0 L2 2 Z'/><g id='g'><path d='M0 0 L1 0 L1 1 Z'/></g></defs><use id='path_use' href='#p'/><use id='group_use' href='#g'/></svg>";let result=import_svg(svg,"test.svg",&ImportOptions::default());let path_block=result.source.split("reference path_use").nth(1).unwrap_or("").split('}').next().unwrap_or("");let group_block=result.source.split("reference group_use").nth(1).unwrap_or("").split('}').next().unwrap_or("");assert!(path_block.contains("data:"),"{}",result.source);assert!(!group_block.contains("data:"),"group resources must remain structured references: {}",result.source);}
#[test]fn integrated_pattern_keeps_positive_dimensions(){let svg="<svg xmlns='http://www.w3.org/2000/svg'><defs><pattern id='p' width='4' height='5'><rect width='4' height='5' fill='red'/></pattern></defs><rect width='8' height='8' fill='url(#p)'/></svg>";let result=import_svg(svg,"test.svg",&ImportOptions::default());assert!(result.source.contains("pattern-width: 4;"));assert!(result.source.contains("pattern-height: 5;"));}
#[test]fn object_bbox_content_uses_safe_svg_fallback(){let svg="<svg xmlns='http://www.w3.org/2000/svg'><defs><pattern id='p' patternContentUnits='objectBoundingBox' width='1' height='1'><rect width='.5' height='1'/></pattern></defs><rect width='8' height='8' fill='url(#p)'/></svg>";let result=import_svg(svg,"test.svg",&ImportOptions::default());assert!(!result.source.contains("pattern-data:"));assert!(result.source.contains("pattern-source:"));}
}