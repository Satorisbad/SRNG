use crate::{prepare, PreparedScene, RevisionGate};
use srng::runtime::Scene;
use std::collections::BTreeMap;

/// Final semantic handoff for native SRNG resources.
///
/// v0.5 keeps SVG-prefixed fields as provenance only. Renderer-facing pattern,
/// mask, gradient, text and image state is consumed from native SRNG
/// properties. No compatibility <defs> tree is generated here.
pub fn prepare_scene(scene: &Scene, revision: u64, gate: &RevisionGate) -> PreparedScene {
    let mut normalized = scene.clone();
    for node in &mut normalized.nodes {
        normalize_properties(&mut node.properties);
    }
    for reference in &mut normalized.references {
        normalize_properties(&mut reference.properties);
        normalize_properties(&mut reference.linked_properties);
    }
    prepare::prepare_scene(&normalized, revision, gate)
}

fn normalize_properties(properties: &mut BTreeMap<String, String>) {
    const PROVENANCE_FALLBACKS: &[(&str, &str)] = &[
        ("pattern-ref", "svg-pattern-ref"),
        ("pattern-width", "svg-pattern-width"),
        ("pattern-height", "svg-pattern-height"),
        ("pattern-source", "svg-pattern-source-xml"),
        ("mask-ref", "svg-mask-ref"),
        ("mask-mode", "svg-mask-mode"),
        ("source-tag", "svg-source-tag"),
        ("source-data", "svg-source-xml"),
    ];

    // Importers predating v0.5 may still emit only provenance keys. Promote
    // them once to native names, never the other direction.
    for (native, provenance) in PROVENANCE_FALLBACKS {
        if properties.contains_key(*native) {
            continue;
        }
        if let Some(value) = properties.get(*provenance).cloned() {
            properties.insert((*native).to_string(), value);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provenance_is_promoted_to_native_without_reconstruction() {
        let mut props = BTreeMap::new();
        props.insert("svg-pattern-ref".into(), "\"p\"".into());
        props.insert("svg-pattern-width".into(), "4px".into());
        normalize_properties(&mut props);
        assert_eq!(props.get("pattern-ref").map(String::as_str), Some("\"p\""));
        assert_eq!(props.get("pattern-width").map(String::as_str), Some("4px"));
        assert!(!props.contains_key("svg-pattern-source-generated"));
    }
}
