use super::common::{quote, unquote, xml_escape};
use srng::runtime::{Geometry, Scene, SceneNode};
use std::collections::{BTreeMap, BTreeSet};

/// Bridges SRNG-native semantic resource properties into the compatibility
/// keys still consumed by the lower-level renderer preparation code.
/// Temporary SVG/XML is reconstructed in memory only when required.
pub(super) fn normalize(scene: &mut Scene) {
    for node in &mut scene.nodes {
        bridge_properties(&mut node.properties);
    }
    for reference in &mut scene.references {
        bridge_properties(&mut reference.properties);
        bridge_properties(&mut reference.linked_properties);
    }

    let has_preserved_defs = scene.nodes.iter().any(|node| {
        node.active
            && node
                .properties
                .get("svg-source-tag")
                .map(|value| unquote(value))
                .as_deref()
                == Some("defs")
            && node.properties.contains_key("svg-source-xml")
    });

    if !has_preserved_defs {
        if let Some(defs) = native_defs_xml(scene) {
            let mut properties = BTreeMap::new();
            properties.insert("fill".into(), "none".into());
            properties.insert("stroke".into(), "none".into());
            properties.insert("svg-source-tag".into(), quote("defs"));
            properties.insert("svg-source-xml".into(), quote(&defs));
            scene.nodes.push(SceneNode {
                id: "__srng_native_resources".into(),
                kind: "group".into(),
                source: scene.source.clone(),
                properties,
                geometry: Geometry {
                    x: Some(0.0),
                    y: Some(0.0),
                    width: Some(0.0),
                    height: Some(0.0),
                },
                paint_order: usize::MAX,
                active: true,
            });
        }
    }
}

fn bridge_properties(properties: &mut BTreeMap<String, String>) {
    const ALIASES: &[(&str, &str)] = &[
        ("pattern-ref", "svg-pattern-ref"),
        ("pattern-width", "svg-pattern-width"),
        ("pattern-height", "svg-pattern-height"),
        ("pattern-source", "svg-pattern-source-xml"),
        ("mask-ref", "svg-mask-ref"),
        ("mask-mode", "svg-mask-mode"),
        ("source-tag", "svg-source-tag"),
        ("source-data", "svg-source-xml"),
    ];

    for (native, compatibility) in ALIASES {
        if properties.contains_key(*compatibility) {
            continue;
        }
        if let Some(value) = properties.get(*native).cloned() {
            properties.insert((*compatibility).to_string(), value);
        }
    }

    if !properties.contains_key("svg-pattern-source-xml") {
        if let Some(xml) = pattern_xml(properties) {
            properties.insert("svg-pattern-source-xml".into(), quote(&xml));
        }
    }
}

fn native_defs_xml(scene: &Scene) -> Option<String> {
    let mut resources = Vec::new();
    let mut seen_patterns = BTreeSet::new();
    let mut seen_masks = BTreeSet::new();

    for properties in scene
        .nodes
        .iter()
        .filter(|node| node.active)
        .map(|node| &node.properties)
        .chain(
            scene
                .references
                .iter()
                .filter(|reference| reference.active)
                .flat_map(|reference| [&reference.linked_properties, &reference.properties]),
        )
    {
        if let Some(id) = properties.get("pattern-ref").map(|value| unquote(value)) {
            if seen_patterns.insert(id) {
                if let Some(xml) = pattern_xml(properties) {
                    resources.push(xml);
                }
            }
        }

        let binary = properties
            .get("mask-mode")
            .map(|value| unquote(value))
            .as_deref()
            == Some("binary-opaque-clip");
        if !binary {
            if let Some(raw) = properties.get("mask-ref").map(|value| unquote(value)) {
                if let Some(id) = ref_id(&raw) {
                    if seen_masks.insert(id.to_string()) {
                        if let Some(xml) = mask_xml(id, properties) {
                            resources.push(xml);
                        }
                    }
                }
            }
        }
    }

    if resources.is_empty() {
        None
    } else {
        Some(format!("<defs>{}</defs>", resources.join("")))
    }
}

fn pattern_xml(properties: &BTreeMap<String, String>) -> Option<String> {
    let id = properties.get("pattern-ref").map(|value| unquote(value))?;
    let width = properties.get("pattern-width").and_then(|value| number(value))?;
    let height = properties.get("pattern-height").and_then(|value| number(value))?;
    let data = properties.get("pattern-data").map(|value| unquote(value))?;
    let units = properties
        .get("pattern-units")
        .map(|value| unquote(value))
        .unwrap_or_else(|| "userSpaceOnUse".to_string());
    let content_units = properties
        .get("pattern-content-units")
        .map(|value| unquote(value))
        .unwrap_or_else(|| "userSpaceOnUse".to_string());
    let shapes = native_shapes_xml(&data)?;
    Some(format!(
        "<pattern id=\"{}\" patternUnits=\"{}\" patternContentUnits=\"{}\" width=\"{}\" height=\"{}\">{}</pattern>",
        xml_escape(&id),
        xml_escape(&units),
        xml_escape(&content_units),
        width,
        height,
        shapes
    ))
}

fn mask_xml(id: &str, properties: &BTreeMap<String, String>) -> Option<String> {
    let data = properties.get("mask-data").map(|value| unquote(value))?;
    let mode = properties
        .get("mask-type")
        .map(|value| unquote(value))
        .unwrap_or_else(|| "luminance".to_string());
    let shapes = native_shapes_xml(&data)?;
    Some(format!(
        "<mask id=\"{}\" style=\"mask-type:{}\">{}</mask>",
        xml_escape(id),
        xml_escape(&mode),
        shapes
    ))
}

fn native_shapes_xml(data: &str) -> Option<String> {
    let mut xml = String::new();
    for record in data.lines().filter(|line| !line.trim().is_empty()) {
        let (fill, path) = record.split_once('|')?;
        xml.push_str("<path d=\"");
        xml.push_str(&xml_escape(path.trim()));
        xml.push_str("\" fill=\"");
        xml.push_str(&xml_escape(fill.trim()));
        xml.push_str("\"/>");
    }
    if xml.is_empty() { None } else { Some(xml) }
}

fn ref_id(value: &str) -> Option<&str> {
    value
        .strip_prefix("url(#")
        .and_then(|value| value.strip_suffix(')'))
        .or_else(|| (!value.contains('(') && !value.is_empty()).then_some(value))
}

fn number(value: &str) -> Option<f64> {
    let value = unquote(value);
    let trimmed = value.trim();
    let numeric = trimmed
        .strip_suffix("px")
        .map(str::trim)
        .unwrap_or(trimmed)
        .replace(' ', "");
    numeric.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bridges_native_semantic_keys_without_svg_provenance() {
        let mut props = BTreeMap::new();
        props.insert("pattern-ref".into(), "\"p\"".into());
        props.insert("pattern-width".into(), "4px".into());
        props.insert("pattern-height".into(), "4px".into());
        props.insert("pattern-data".into(), "\"#fff|M 0 0 h 2 v 4 h -2 Z\"".into());
        props.insert("pattern-units".into(), "\"userSpaceOnUse\"".into());
        bridge_properties(&mut props);
        assert_eq!(props.get("svg-pattern-ref").map(String::as_str), Some("\"p\""));
        assert!(props.contains_key("svg-pattern-source-xml"));
    }

    #[test]
    fn native_shape_records_reconstruct_resource_xml() {
        let xml = native_shapes_xml("#ff0000|M 0 0 h 2 v 4 h -2 Z\n#0000ff|M 2 0 h 2 v 4 h -2 Z").unwrap();
        assert!(xml.contains("fill=\"#ff0000\""));
        assert!(xml.contains("fill=\"#0000ff\""));
        assert!(!xml.contains("svg-"));
    }
}
