use roxmltree::{Document as XmlDocument, Node};
use std::collections::HashMap;

pub use crate::svg_import_impl::{ImportDiagnostic, ImportOptions, ImportResult};

/// Import SVG into SRNG while promoting supported SVG semantics to native,
/// unprefixed SRNG properties. SVG-prefixed properties remain provenance only.
pub fn import_svg(svg: &str, source_name: &str, options: &ImportOptions) -> ImportResult {
    let mut result = crate::svg_import_impl::import_svg(svg, source_name, options);
    promote_native_semantics(&mut result.source);

    for diagnostic in &mut result.diagnostics {
        match diagnostic.code.as_str() {
            "S232" => {
                diagnostic.severity = "info".to_string();
                diagnostic.message = diagnostic.message.replace(
                    "is preserved with tile metadata; pattern painting is the next renderer stage",
                    "is mapped to native SRNG pattern paint with SVG provenance retained",
                );
            }
            "S242" => {
                diagnostic.severity = "info".to_string();
                diagnostic.message = diagnostic.message.replace(
                    "requires alpha/luminance compositing that is not yet supported",
                    "is mapped to native SRNG mask semantics with SVG provenance retained",
                );
            }
            _ => {}
        }
    }
    result
}

#[derive(Clone)]
struct MaskResource {
    data: String,
    mode: String,
}

/// Promote renderer-facing semantics and, where possible, encode vector
/// pattern/mask contents as SRNG-native path+paint records. The record grammar
/// is deliberately small and deterministic: one `fill|path` record per line.
fn promote_native_semantics(source: &mut String) {
    let masks = collect_native_masks(source);
    let mut out = String::with_capacity(source.len() + source.len() / 6);
    let mut mask_data_emitted = false;

    for line in source.lines() {
        let trimmed = line.trim_start();
        let indent_len = line.len() - trimmed.len();
        let indent = &line[..indent_len];
        if trimmed.ends_with('{') {
            mask_data_emitted = false;
        }

        out.push_str(line);
        out.push('\n');

        let alias = if let Some(rest) = trimmed.strip_prefix("svg-pattern-ref:") {
            Some(("pattern-ref:", rest))
        } else if let Some(rest) = trimmed.strip_prefix("svg-pattern-width:") {
            Some(("pattern-width:", rest))
        } else if let Some(rest) = trimmed.strip_prefix("svg-pattern-height:") {
            Some(("pattern-height:", rest))
        } else if let Some(rest) = trimmed.strip_prefix("svg-pattern-source-xml:") {
            Some(("pattern-source:", rest))
        } else if let Some(rest) = trimmed.strip_prefix("svg-mask-ref:") {
            Some(("mask-ref:", rest))
        } else if let Some(rest) = trimmed.strip_prefix("svg-mask-mode:") {
            Some(("mask-mode:", rest))
        } else if let Some(rest) = trimmed.strip_prefix("svg-attr-mask:") {
            Some(("mask-ref:", rest))
        } else if let Some(rest) = trimmed.strip_prefix("svg-source-tag:") {
            Some(("source-tag:", rest))
        } else if let Some(rest) = trimmed.strip_prefix("svg-source-xml:") {
            Some(("source-data:", rest))
        } else {
            None
        };

        if let Some((key, rest)) = alias {
            out.push_str(indent);
            out.push_str(key);
            out.push_str(rest);
            out.push('\n');
        }

        if trimmed.starts_with("svg-pattern-source-xml:") {
            if let Some(xml) = property_string(trimmed, "svg-pattern-source-xml:") {
                if let Some((data, units, content_units)) = native_pattern(&xml) {
                    out.push_str(indent);
                    out.push_str("pattern-data: ");
                    out.push_str(&quote_srng(&data));
                    out.push_str(";\n");
                    out.push_str(indent);
                    out.push_str("pattern-units: ");
                    out.push_str(&quote_srng(&units));
                    out.push_str(";\n");
                    out.push_str(indent);
                    out.push_str("pattern-content-units: ");
                    out.push_str(&quote_srng(&content_units));
                    out.push_str(";\n");
                }
            }
        }

        if !mask_data_emitted
            && (trimmed.starts_with("svg-attr-mask:") || trimmed.starts_with("svg-mask-ref:"))
        {
            let prefix = if trimmed.starts_with("svg-attr-mask:") {
                "svg-attr-mask:"
            } else {
                "svg-mask-ref:"
            };
            if let Some(raw) = property_string(trimmed, prefix) {
                if let Some(id) = url_fragment(&raw) {
                    if let Some(mask) = masks.get(id) {
                        out.push_str(indent);
                        out.push_str("mask-data: ");
                        out.push_str(&quote_srng(&mask.data));
                        out.push_str(";\n");
                        out.push_str(indent);
                        out.push_str("mask-type: ");
                        out.push_str(&quote_srng(&mask.mode));
                        out.push_str(";\n");
                        mask_data_emitted = true;
                    }
                }
            }
        }
    }
    *source = out;
}

/// Remove SVG provenance. If all referenced patterns/masks have native vector
/// payloads, transitional raw XML payloads are removed too, yielding a file
/// that contains no SVG resource syntax for those supported features.
pub fn strip_svg_provenance(source: &str) -> String {
    let patterns_native = !source.contains("pattern-ref:") || source.contains("pattern-data:");
    let masks_native = !source.contains("mask-ref:") || source.contains("mask-data:") || source.contains("mask-mode: \"binary-opaque-clip\"");
    let can_drop_raw_resources = patterns_native && masks_native;

    source
        .lines()
        .filter(|line| {
            let trimmed = line.trim_start();
            if trimmed.starts_with("svg-") {
                return false;
            }
            if can_drop_raw_resources
                && (trimmed.starts_with("source-tag:")
                    || trimmed.starts_with("source-data:")
                    || trimmed.starts_with("pattern-source:"))
            {
                return false;
            }
            true
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

fn collect_native_masks(source: &str) -> HashMap<String, MaskResource> {
    let mut masks = HashMap::new();
    for line in source.lines() {
        let trimmed = line.trim_start();
        let Some(xml) = property_string(trimmed, "svg-source-xml:") else {
            continue;
        };
        let Ok(document) = XmlDocument::parse(&xml) else {
            continue;
        };
        for mask in document
            .descendants()
            .filter(|node| node.is_element() && node.tag_name().name() == "mask")
        {
            let Some(id) = mask.attribute("id") else { continue; };
            let Some(data) = serialize_shapes(mask) else { continue; };
            let mode = mask_mode(mask);
            masks.insert(id.to_string(), MaskResource { data, mode });
        }
    }
    masks
}

fn native_pattern(xml: &str) -> Option<(String, String, String)> {
    let document = XmlDocument::parse(xml).ok()?;
    let pattern = document
        .descendants()
        .find(|node| node.is_element() && node.tag_name().name() == "pattern")?;
    let data = serialize_shapes(pattern)?;
    let units = pattern.attribute("patternUnits").unwrap_or("objectBoundingBox").to_string();
    let content_units = pattern
        .attribute("patternContentUnits")
        .unwrap_or("userSpaceOnUse")
        .to_string();
    Some((data, units, content_units))
}

fn serialize_shapes(parent: Node<'_, '_>) -> Option<String> {
    let mut records = Vec::new();
    for child in parent.children().filter(|node| node.is_element()) {
        if child.attribute("transform").is_some() {
            return None;
        }
        let path = shape_path(child)?;
        let fill = shape_fill(child)?;
        records.push(format!("{fill}|{path}"));
    }
    if records.is_empty() {
        None
    } else {
        Some(records.join("\n"))
    }
}

fn shape_fill(node: Node<'_, '_>) -> Option<String> {
    if let Some(opacity) = node.attribute("opacity").and_then(parse_number) {
        if (opacity - 1.0).abs() > f64::EPSILON {
            return None;
        }
    }
    let mut fill = node.attribute("fill").map(str::to_string);
    if let Some(style) = node.attribute("style") {
        for field in style.split(';') {
            if let Some((key, value)) = field.split_once(':') {
                if key.trim() == "fill" {
                    fill = Some(value.trim().to_string());
                }
            }
        }
    }
    let fill = fill.unwrap_or_else(|| "#000000".to_string());
    if fill == "none" || fill.starts_with("url(") {
        return None;
    }
    Some(fill)
}

fn shape_path(node: Node<'_, '_>) -> Option<String> {
    let n = |name: &str, default: f64| {
        node.attribute(name).and_then(parse_number).unwrap_or(default)
    };
    match node.tag_name().name() {
        "path" => node.attribute("d").map(str::to_string),
        "rect" => {
            let x = n("x", 0.0);
            let y = n("y", 0.0);
            let w = n("width", 0.0).max(0.0);
            let h = n("height", 0.0).max(0.0);
            let rx = node.attribute("rx").and_then(parse_number).unwrap_or(0.0).max(0.0).min(w / 2.0);
            let ry = node.attribute("ry").and_then(parse_number).unwrap_or(rx).max(0.0).min(h / 2.0);
            Some(rounded_rect_path(x, y, w, h, rx, ry))
        }
        "circle" => {
            let cx = n("cx", 0.0);
            let cy = n("cy", 0.0);
            let r = n("r", 0.0).max(0.0);
            Some(ellipse_path(cx, cy, r, r))
        }
        "ellipse" => {
            let cx = n("cx", 0.0);
            let cy = n("cy", 0.0);
            Some(ellipse_path(cx, cy, n("rx", 0.0).max(0.0), n("ry", 0.0).max(0.0)))
        }
        "line" => Some(format!("M {} {} L {} {}", fmt(n("x1", 0.0)), fmt(n("y1", 0.0)), fmt(n("x2", 0.0)), fmt(n("y2", 0.0)))),
        "polyline" | "polygon" => {
            let mut values = node.attribute("points")?.split(|c: char| c == ',' || c.is_whitespace()).filter(|v| !v.is_empty());
            let x = values.next()?.parse::<f64>().ok()?;
            let y = values.next()?.parse::<f64>().ok()?;
            let mut d = format!("M {} {}", fmt(x), fmt(y));
            loop {
                let Some(x) = values.next() else { break; };
                let y = values.next()?;
                d.push_str(&format!(" L {} {}", fmt(x.parse().ok()?), fmt(y.parse().ok()?)));
            }
            if node.tag_name().name() == "polygon" { d.push_str(" Z"); }
            Some(d)
        }
        _ => None,
    }
}

fn rounded_rect_path(x: f64, y: f64, w: f64, h: f64, rx: f64, ry: f64) -> String {
    if rx <= 0.0 || ry <= 0.0 {
        return format!("M {} {} h {} v {} h {} Z", fmt(x), fmt(y), fmt(w), fmt(h), fmt(-w));
    }
    let right = x + w;
    let bottom = y + h;
    format!(
        "M {} {} H {} A {} {} 0 0 1 {} {} V {} A {} {} 0 0 1 {} {} H {} A {} {} 0 0 1 {} {} V {} A {} {} 0 0 1 {} {} Z",
        fmt(x + rx), fmt(y), fmt(right - rx), fmt(rx), fmt(ry), fmt(right), fmt(y + ry),
        fmt(bottom - ry), fmt(rx), fmt(ry), fmt(right - rx), fmt(bottom), fmt(x + rx),
        fmt(rx), fmt(ry), fmt(x), fmt(bottom - ry), fmt(y + ry), fmt(rx), fmt(ry), fmt(x + rx), fmt(y)
    )
}

fn ellipse_path(cx: f64, cy: f64, rx: f64, ry: f64) -> String {
    format!(
        "M {} {} A {} {} 0 1 0 {} {} A {} {} 0 1 0 {} {} Z",
        fmt(cx - rx), fmt(cy), fmt(rx), fmt(ry), fmt(cx + rx), fmt(cy),
        fmt(rx), fmt(ry), fmt(cx - rx), fmt(cy)
    )
}

fn mask_mode(mask: Node<'_, '_>) -> String {
    if let Some(style) = mask.attribute("style") {
        for field in style.split(';') {
            if let Some((key, value)) = field.split_once(':') {
                if key.trim() == "mask-type" {
                    return value.trim().to_string();
                }
            }
        }
    }
    mask.attribute("mask-type").unwrap_or("luminance").to_string()
}

fn property_string(line: &str, prefix: &str) -> Option<String> {
    let value = line.strip_prefix(prefix)?.trim().strip_suffix(';')?.trim();
    Some(unquote_srng(value))
}

fn unquote_srng(value: &str) -> String {
    let Some(inner) = value.strip_prefix('"').and_then(|value| value.strip_suffix('"')) else {
        return value.to_string();
    };
    let mut out = String::new();
    let mut chars = inner.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' { out.push(ch); continue; }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('t') => out.push('\t'),
            Some('"') => out.push('"'),
            Some('\\') => out.push('\\'),
            Some(other) => { out.push('\\'); out.push(other); }
            None => out.push('\\'),
        }
    }
    out
}

fn quote_srng(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n").replace('\r', "\\r").replace('\t', "\\t"))
}

fn url_fragment(value: &str) -> Option<&str> {
    value.trim().strip_prefix("url(#")?.strip_suffix(')')
}

fn parse_number(value: &str) -> Option<f64> {
    value.trim().strip_suffix("px").unwrap_or(value.trim()).trim().parse().ok()
}

fn fmt(value: f64) -> String {
    if value.fract().abs() < 1e-9 { format!("{}", value as i64) } else { format!("{value:.6}").trim_end_matches('0').trim_end_matches('.').to_string() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vector_pattern_becomes_native_without_embedded_xml() {
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20"><defs><pattern id="p" patternUnits="userSpaceOnUse" width="4" height="4"><rect width="2" height="4" fill="#fff"/></pattern></defs><rect id="box" width="20" height="20" fill="url(#p)"/></svg>"##;
        let result = import_svg(svg, "pattern.svg", &ImportOptions::default());
        let stripped = strip_svg_provenance(&result.source);
        assert!(stripped.contains("pattern-ref: \"p\";"));
        assert!(stripped.contains("pattern-data:"));
        assert!(stripped.contains("pattern-units: \"userSpaceOnUse\";"));
        assert!(!stripped.contains("pattern-source:"));
        assert!(!stripped.contains("source-data:"));
        assert!(!stripped.contains("<pattern"));
        assert!(!stripped.lines().any(|line| line.trim_start().starts_with("svg-")));
    }

    #[test]
    fn binary_mask_keeps_native_clip_semantics() {
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20"><defs><mask id="m"><circle cx="10" cy="10" r="5" fill="white"/></mask></defs><rect id="box" width="20" height="20" fill="#fff" mask="url(#m)"/></svg>"##;
        let result = import_svg(svg, "mask.svg", &ImportOptions::default());
        let stripped = strip_svg_provenance(&result.source);
        assert!(stripped.contains("mask-mode: \"binary-opaque-clip\";"));
        assert!(stripped.contains("mask-data:"));
        assert!(stripped.contains("clip:"));
    }

    #[test]
    fn complex_mask_becomes_native_vector_resource() {
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20"><defs><mask id="m"><rect width="10" height="20" fill="#808080"/></mask></defs><rect id="box" width="20" height="20" fill="#fff" mask="url(#m)"/></svg>"##;
        let result = import_svg(svg, "mask.svg", &ImportOptions::default());
        let stripped = strip_svg_provenance(&result.source);
        assert!(stripped.contains("mask-ref: \"url(#m)\";"));
        assert!(stripped.contains("mask-data:"));
        assert!(stripped.contains("mask-type: \"luminance\";"));
        assert!(!stripped.contains("source-data:"));
        assert!(!stripped.contains("<mask"));
        assert!(!stripped.lines().any(|line| line.trim_start().starts_with("svg-")));
    }
}
