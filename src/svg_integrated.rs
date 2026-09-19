use roxmltree::{Document as XmlDocument, Node};
use std::collections::HashMap;

pub use crate::svg_filter_v06::{strip_svg_provenance, ImportDiagnostic, ImportOptions, ImportResult};

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
        Some((id, (width, height)))
    }).collect::<HashMap<_, _>>();

    let lines = source.lines().collect::<Vec<_>>();
    let mut out = String::with_capacity(source.len() + 256);
    let mut i = 0usize;
    while i < lines.len() {
        let trimmed = lines[i].trim();
        if trimmed.ends_with('{') && !trimmed.starts_with("relation ") {
            let header = lines[i];
            let mut block = Vec::<String>::new();
            i += 1;
            while i < lines.len() && lines[i].trim() != "}" { block.push(lines[i].to_string()); i += 1; }

            if let Some(rest) = trimmed.strip_prefix("reference ") {
                let id = rest.split_whitespace().next().unwrap_or("");
                if let Some(fill) = fills.get(id) { set_property(&mut block, "fill", fill); }
            }

            if let Some(pattern_id) = property_value(&block, "pattern-ref").or_else(|| property_value(&block, "svg-pattern-ref")) {
                let id = pattern_id.trim_matches('"').trim().trim_start_matches("url(#").trim_end_matches(')');
                if let Some((width, height)) = patterns.get(id) {
                    set_property(&mut block, "pattern-width", &format!("{width}px"));
                    set_property(&mut block, "pattern-height", &format!("{height}px"));
                }
            }

            let binary_mask = property_value(&block, "mask-mode").or_else(|| property_value(&block, "svg-mask-mode")).is_some_and(|v| v.trim_matches('"') == "binary-opaque-clip");
            if binary_mask {
                block.retain(|line| {
                    let t = line.trim_start();
                    !t.starts_with("mask-data:") && !t.starts_with("mask-type:")
                });
            }

            out.push_str(header); out.push('\n');
            for line in block { out.push_str(&line); out.push('\n'); }
            if i < lines.len() { out.push_str(lines[i]); out.push('\n'); i += 1; }
            continue;
        }
        out.push_str(lines[i]); out.push('\n'); i += 1;
    }
    *source = out;
}

fn property_value(lines: &[String], key: &str) -> Option<String> {
    let prefix = format!("{key}:");
    lines.iter().find_map(|line| {
        let value = line.trim_start().strip_prefix(&prefix)?.trim().strip_suffix(';')?.trim();
        Some(value.to_string())
    })
}

fn set_property(lines: &mut Vec<String>, key: &str, value: &str) {
    let prefix = format!("{key}:");
    if let Some(line) = lines.iter_mut().find(|line| line.trim_start().starts_with(&prefix)) {
        *line = format!("    {key}: {value};");
    } else {
        lines.push(format!("    {key}: {value};"));
    }
}

fn pattern_dimension(value: &str) -> Option<f64> {
    let value = value.trim();
    if let Some(percent) = value.strip_suffix('%') { return percent.trim().parse::<f64>().ok().map(|v| v / 100.0); }
    value.trim_end_matches("px").parse::<f64>().ok().filter(|v| v.is_finite() && *v > 0.0)
}

fn computed_fill(node: Node<'_, '_>) -> String {
    for current in node.ancestors().filter(|n| n.is_element()) {
        if let Some(value) = style_property(current, "fill") { return normalize_fill(&value); }
        if let Some(value) = current.attribute("fill") { return normalize_fill(value); }
    }
    "#000000".into()
}

fn style_property(node: Node<'_, '_>, key: &str) -> Option<String> {
    node.attribute("style")?.split(';').find_map(|entry| {
        let (name, value) = entry.split_once(':')?;
        (name.trim() == key).then(|| value.trim().to_string())
    })
}

fn normalize_fill(value: &str) -> String {
    let value = value.trim();
    match value.to_ascii_lowercase().as_str() {
        "black" => "#000000".into(), "white" => "#ffffff".into(), "red" => "#ff0000".into(),
        "green" => "#008000".into(), "blue" => "#0000ff".into(), _ => value.to_string(),
    }
}

fn safe_ident(raw: &str) -> String {
    let mut out = String::new();
    for (i, ch) in raw.chars().enumerate() {
        let valid = if i == 0 { ch.is_ascii_alphabetic() || ch == '_' || ch == '%' } else { ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | '/' | '%') };
        out.push(if valid { ch } else { '_' });
    }
    if out.is_empty() { "svg_node".into() } else { out }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn use_without_fill_gets_svg_default_black() {
        let svg = "<svg xmlns='http://www.w3.org/2000/svg'><defs><g id='item'><rect width='3' height='4'/></g></defs><use id='plain' href='#item'/></svg>";
        let result = import_svg(svg, "test.svg", &ImportOptions::default());
        let block = result.source.split("reference plain").nth(1).unwrap_or("");
        assert!(block.split('}').next().unwrap_or("").contains("fill: #000000;"), "{}", result.source);
    }
    #[test]
    fn explicit_none_is_not_replaced() {
        let svg = "<svg xmlns='http://www.w3.org/2000/svg'><defs><rect id='item' width='1' height='1'/></defs><use id='plain' href='#item' fill='none'/></svg>";
        let result = import_svg(svg, "test.svg", &ImportOptions::default());
        let block = result.source.split("reference plain").nth(1).unwrap_or("");
        assert!(block.split('}').next().unwrap_or("").contains("fill: none;"), "{}", result.source);
    }
    #[test]
    fn integrated_pattern_keeps_positive_dimensions() {
        let svg = "<svg xmlns='http://www.w3.org/2000/svg'><defs><pattern id='p' width='4' height='5'><rect width='4' height='5' fill='red'/></pattern></defs><rect width='8' height='8' fill='url(#p)'/></svg>";
        let result = import_svg(svg, "test.svg", &ImportOptions::default());
        assert!(result.source.contains("pattern-width: 4px;"));
        assert!(result.source.contains("pattern-height: 5px;"));
    }
}