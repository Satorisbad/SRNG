use crate::{prepare, PreparedScene, RevisionGate};
use srng::runtime::Scene;
use std::collections::BTreeMap;

/// Prepare a scene after promoting SRNG-native semantic properties into the
/// compatibility keys consumed by the current renderer internals.
///
/// This keeps SVG provenance optional: imported SRNG can have every `svg-*`
/// property removed and still render through the native semantic aliases.
pub fn prepare_scene(scene: &Scene, revision: u64, gate: &RevisionGate) -> PreparedScene {
    let mut normalized = scene.clone();

    for node in &mut normalized.nodes {
        bridge_properties(&mut node.properties);
    }
    for reference in &mut normalized.references {
        bridge_properties(&mut reference.properties);
        bridge_properties(&mut reference.linked_properties);
    }

    prepare::prepare_scene(&normalized, revision, gate)
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bridges_native_semantic_keys_without_svg_provenance() {
        let mut props = BTreeMap::new();
        props.insert("pattern-ref".into(), "\"p\"".into());
        props.insert("pattern-width".into(), "4px".into());
        props.insert("source-tag".into(), "\"defs\"".into());
        bridge_properties(&mut props);
        assert_eq!(props.get("svg-pattern-ref").map(String::as_str), Some("\"p\""));
        assert_eq!(props.get("svg-pattern-width").map(String::as_str), Some("4px"));
        assert_eq!(props.get("svg-source-tag").map(String::as_str), Some("\"defs\""));
    }
}
