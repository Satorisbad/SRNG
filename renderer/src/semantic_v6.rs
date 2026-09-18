use crate::{PreparedScene, RevisionGate};
use srng::runtime::Scene;

/// Normalizes radial-gradient SRNG properties into renderer-facing native
/// radial-gradient paint properties. No SVG XML is reconstructed here.
pub fn prepare_scene(scene: &Scene, revision: u64, gate: &RevisionGate) -> PreparedScene {
    let mut normalized = scene.clone();

    for node in &mut normalized.nodes {
        if node
            .properties
            .get("gradient-kind")
            .map(|v| unquote(v))
            .as_deref()
            != Some("radial-gradient")
        {
            continue;
        }

        let units = node
            .properties
            .get("gradient-units")
            .map(|v| unquote(v))
            .unwrap_or_else(|| "objectBoundingBox".into());
        let x = node.geometry.x.unwrap_or(0.0);
        let y = node.geometry.y.unwrap_or(0.0);
        let w = node.geometry.width.unwrap_or(1.0).abs();
        let h = node.geometry.height.unwrap_or(1.0).abs();
        let coord = |key: &str, origin: f64, extent: f64, default: &str| {
            resolve_coord(
                node.properties
                    .get(key)
                    .map(|v| unquote(v))
                    .as_deref()
                    .unwrap_or(default),
                &units,
                origin,
                extent,
            )
        };

        let cx = coord("gradient-cx", x, w, "50%");
        let cy = coord("gradient-cy", y, h, "50%");
        let fx = coord("gradient-fx", x, w, "50%");
        let fy = coord("gradient-fy", y, h, "50%");
        let raw_r = node
            .properties
            .get("gradient-r")
            .map(|v| unquote(v))
            .unwrap_or_else(|| "50%".into());
        let r = resolve_radius(&raw_r, &units, w, h);

        node.properties.insert("fill".into(), "radial-gradient".into());
        node.properties.insert("gradient-cx".into(), format_number(cx));
        node.properties.insert("gradient-cy".into(), format_number(cy));
        node.properties.insert("gradient-fx".into(), format_number(fx));
        node.properties.insert("gradient-fy".into(), format_number(fy));
        node.properties.insert("gradient-r".into(), format_number(r));
        node.properties.insert("gradient-units".into(), "userSpaceOnUse".into());
    }

    crate::semantic_v5::prepare_scene(&normalized, revision, gate)
}

fn resolve_coord(raw: &str, units: &str, origin: f64, extent: f64) -> f64 {
    let raw = raw.trim();
    if let Some(percent) = raw.strip_suffix('%').and_then(|v| v.parse::<f64>().ok()) {
        return if units == "objectBoundingBox" {
            origin + extent * percent / 100.0
        } else {
            percent / 100.0
        };
    }
    let value = raw.trim_end_matches("px").parse::<f64>().unwrap_or(0.0);
    if units == "objectBoundingBox" {
        origin + extent * value
    } else {
        value
    }
}

fn resolve_radius(raw: &str, units: &str, width: f64, height: f64) -> f64 {
    let raw = raw.trim();
    if let Some(percent) = raw.strip_suffix('%').and_then(|v| v.parse::<f64>().ok()) {
        return if units == "objectBoundingBox" {
            width.max(height) * percent / 100.0
        } else {
            percent / 100.0
        };
    }
    let value = raw.trim_end_matches("px").parse::<f64>().unwrap_or(0.5);
    if units == "objectBoundingBox" {
        width.max(height) * value
    } else {
        value
    }
}

fn unquote(value: &str) -> String {
    let value = value.trim();
    let Some(inner) = value.strip_prefix('"').and_then(|v| v.strip_suffix('"')) else {
        return value.to_string();
    };
    inner
        .replace("\\n", "\n")
        .replace("\\\"", "\"")
        .replace("\\\\", "\\")
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

    #[test]
    fn percentage_object_bbox_coordinates_resolve() {
        assert!((resolve_coord("50%", "objectBoundingBox", 10.0, 20.0) - 20.0).abs() < 1e-9);
    }

    #[test]
    fn object_bbox_radius_resolves_to_geometry() {
        assert!((resolve_radius("50%", "objectBoundingBox", 20.0, 10.0) - 10.0).abs() < 1e-9);
        assert!((resolve_radius("0.5", "objectBoundingBox", 20.0, 10.0) - 10.0).abs() < 1e-9);
    }
}
