use crate::{PreparedScene, RevisionGate};
use srng::runtime::Scene;

/// Native image normalization layer.
///
/// Authored SRNG image nodes use their normal geometry for placement and size.
/// SVG-imported image nodes may additionally carry source-* compatibility
/// properties. Older semantic layers expect those source-* values, so bridge
/// normal SRNG geometry into that representation without changing the authored
/// scene or requiring SVG-specific properties from native SRNG.
pub fn prepare_scene(scene: &Scene, revision: u64, gate: &RevisionGate) -> PreparedScene {
    let mut normalized = scene.clone();

    for node in &mut normalized.nodes {
        if node.kind != "group" {
            continue;
        }
        let is_image = node
            .properties
            .get("image-data")
            .or_else(|| node.properties.get("href"))
            .map(|value| unquote(value).starts_with("data:image/"))
            .unwrap_or(false);
        if !is_image {
            continue;
        }

        if !node.properties.contains_key("source-x") {
            if let Some(value) = node.geometry.x {
                node.properties.insert("source-x".into(), format_number(value));
            }
        }
        if !node.properties.contains_key("source-y") {
            if let Some(value) = node.geometry.y {
                node.properties.insert("source-y".into(), format_number(value));
            }
        }
        if !node.properties.contains_key("source-width") {
            if let Some(value) = node.geometry.width {
                node.properties.insert("source-width".into(), format_number(value));
            }
        }
        if !node.properties.contains_key("source-height") {
            if let Some(value) = node.geometry.height {
                node.properties.insert("source-height".into(), format_number(value));
            }
        }
    }

    crate::semantic_v7::prepare_scene(&normalized, revision, gate)
}

fn unquote(value: &str) -> String {
    let value = value.trim();
    value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .unwrap_or(value)
        .to_string()
}

fn format_number(value: f64) -> String {
    if value.fract().abs() < 1e-9 {
        format!("{}", value as i64)
    } else {
        format!("{value:.6}")
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use srng::runtime::{execute_json, RuntimeOptions};

    #[test]
    fn native_image_geometry_is_bridged_to_source_geometry() {
        let source = r#"
srng 0.1;
group image {
    position: 2px 3px;
    size: 8px 9px;
    image-data: "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAFgwJ/lC9pWQAAAABJRU5ErkJggg==";
}
"#;
        let ir = srng::compile_to_json(source, "native-image-regression.srng");
        let scene = execute_json(&ir, &RuntimeOptions::default()).expect("runtime scene");
        let gate = RevisionGate::default();
        let revision = gate.begin();
        let prepared = prepare_scene(&scene, revision, &gate);
        assert!(prepared.commands.iter().any(|command| matches!(
            command,
            crate::Command::DrawImage { image }
                if image.width == 8.0 && image.height == 9.0
        )));
    }
}
