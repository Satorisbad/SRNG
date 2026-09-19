use roxmltree::{Document as XmlDocument, Node};
use std::collections::HashMap;

pub use crate::svg_filter_v06::{strip_svg_provenance, ImportDiagnostic, ImportOptions, ImportResult};

pub fn import_svg(svg: &str, source_name: &str, options: &ImportOptions) -> ImportResult {
    let mut result = crate::svg_filter_v06::import_svg(svg, source_name, options);
    apply_use_computed_fill(svg, &mut result.source);
    result
}

fn apply_use_computed_fill(svg: &str, source: &mut String) {
    let Ok(document) = XmlDocument::parse(svg) else { return; };
    let mut fills = HashMap::<String, String>::new();
    let mut generated = 0usize;
    for node in document.descendants().filter(|n| n.is_element() && n.tag_name().name() == "use") {
        generated += 1;
        let fallback = format!("use_{generated}");
        let id = safe_ident(node.attribute("id").unwrap_or(&fallback));
        fills.insert(id, computed_fill(node));
    }
    if fills.is_empty() { return; }

    let lines = source.lines().collect::<Vec<_>>();
    let mut out = String::with_capacity(source.len() + fills.len() * 20);
    let mut i = 0usize;
    while i < lines.len() {
        let trimmed = lines[i].trim();
        if let Some(rest) = trimmed.strip_prefix("reference ") {
            let id = rest.split_whitespace().next().unwrap_or("");
            if let Some(fill) = fills.get(id) {
                out.push_str(lines[i]); out.push('\n'); i += 1;
                let mut wrote_fill = false;
                while i < lines.len() && lines[i].trim() != "}" {
                    if lines[i].trim_start().starts_with("fill:") {
                        out.push_str("    fill: "); out.push_str(fill); out.push_str(";\n");
                        wrote_fill = true;
                    } else {
                        out.push_str(lines[i]); out.push('\n');
                    }
                    i += 1;
                }
                if !wrote_fill { out.push_str("    fill: "); out.push_str(fill); out.push_str(";\n"); }
                if i < lines.len() { out.push_str(lines[i]); out.push('\n'); i += 1; }
                continue;
            }
        }
        out.push_str(lines[i]); out.push('\n'); i += 1;
    }
    *source = out;
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
        "black" => "#000000".into(),
        "white" => "#ffffff".into(),
        "red" => "#ff0000".into(),
        "green" => "#008000".into(),
        "blue" => "#0000ff".into(),
        _ => value.to_string(),
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
}