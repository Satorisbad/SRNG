use srng::runtime::Scene;
use std::collections::{BTreeMap, HashMap, HashSet};

/// Resolves SVG-style cumulative group opacity through active containment
/// relations before geometry/paint normalization consumes the final value.
pub(super) fn normalize(scene: &mut Scene) {
    let parents = scene
        .relations
        .iter()
        .filter(|r| r.active && r.kind.as_deref() == Some("contains"))
        .map(|r| (r.to.clone(), r.from.clone()))
        .collect::<HashMap<_, _>>();
    let authored = scene
        .nodes
        .iter()
        .map(|n| (n.id.clone(), node_opacity(&n.properties)))
        .collect::<HashMap<_, _>>();

    let mut memo = HashMap::new();
    for node in &mut scene.nodes {
        let mut visiting = HashSet::new();
        let opacity = cumulative_opacity(&node.id, &parents, &authored, &mut memo, &mut visiting);
        if opacity < 1.0 - f64::EPSILON {
            node.properties.insert("opacity".into(), format!("{opacity}"));
        } else {
            node.properties.remove("opacity");
        }
    }
}

fn cumulative_opacity(
    id: &str,
    parents: &HashMap<String, String>,
    authored: &HashMap<String, f64>,
    memo: &mut HashMap<String, f64>,
    visiting: &mut HashSet<String>,
) -> f64 {
    if let Some(value) = memo.get(id) {
        return *value;
    }
    if !visiting.insert(id.to_string()) {
        return authored.get(id).copied().unwrap_or(1.0);
    }
    let local = authored.get(id).copied().unwrap_or(1.0);
    let parent = parents
        .get(id)
        .map(|p| cumulative_opacity(p, parents, authored, memo, visiting))
        .unwrap_or(1.0);
    visiting.remove(id);
    let value = (local * parent).clamp(0.0, 1.0);
    memo.insert(id.to_string(), value);
    value
}

fn node_opacity(props: &BTreeMap<String, String>) -> f64 {
    props
        .get("opacity")
        .map(|v| unquote(v))
        .and_then(|v| v.parse::<f64>().ok())
        .unwrap_or(1.0)
        .clamp(0.0, 1.0)
}

fn unquote(value: &str) -> String {
    let value = value.trim();
    value
        .strip_prefix('"')
        .and_then(|v| v.strip_suffix('"'))
        .unwrap_or(value)
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cumulative_group_opacity_multiplies() {
        let parents = HashMap::from([
            ("child".to_string(), "group".to_string()),
            ("group".to_string(), "root".to_string()),
        ]);
        let authored = HashMap::from([
            ("root".to_string(), 1.0),
            ("group".to_string(), 0.5),
            ("child".to_string(), 0.5),
        ]);
        let mut memo = HashMap::new();
        let mut visiting = HashSet::new();
        assert!((cumulative_opacity("child", &parents, &authored, &mut memo, &mut visiting) - 0.25).abs() < 1e-9);
    }

    #[test]
    fn quoted_opacity_parses() {
        let mut props = BTreeMap::new();
        props.insert("opacity".into(), "\"0.4\"".into());
        assert!((node_opacity(&props) - 0.4).abs() < 1e-9);
    }
}
