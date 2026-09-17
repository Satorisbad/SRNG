use srng::runtime::{Geometry, Scene, SceneNode};
use std::collections::BTreeMap;

/// Converts native radial-gradient declarations into the temporary local
/// compatibility resources consumed by the current pattern rasterizer.
/// Native SRNG properties remain the source of truth.
pub(super) fn normalize(scene: &mut Scene) {
    let vw = scene.viewport.width;
    let vh = scene.viewport.height;
    let mut resources = Vec::new();

    for node in &mut scene.nodes {
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
        let r = if let Some(percent) = raw_r
            .trim()
            .strip_suffix('%')
            .and_then(|v| v.parse::<f64>().ok())
        {
            if units == "objectBoundingBox" {
                w.max(h) * percent / 100.0
            } else {
                percent / 100.0
            }
        } else {
            raw_r.trim_end_matches("px").parse().unwrap_or(0.5)
        };

        let stops = svg_stops(
            node.properties
                .get("gradient-stops")
                .map(String::as_str)
                .unwrap_or(""),
        );
        let id = format!("__radial_{}", safe_id(&node.id));
        let gradient_id = format!("{id}_gradient");
        let gradient = format!(
            "<radialGradient id=\"{gradient_id}\" gradientUnits=\"userSpaceOnUse\" cx=\"{cx}\" cy=\"{cy}\" r=\"{r}\" fx=\"{fx}\" fy=\"{fy}\">{stops}</radialGradient>"
        );
        let pattern = format!(
            "<pattern id=\"{id}\" patternUnits=\"userSpaceOnUse\" width=\"{vw}\" height=\"{vh}\"><rect x=\"0\" y=\"0\" width=\"{vw}\" height=\"{vh}\" fill=\"url(#{gradient_id})\"/></pattern>"
        );
        let standalone = format!(
            "<pattern id=\"{id}\" patternUnits=\"userSpaceOnUse\" width=\"{vw}\" height=\"{vh}\"><defs>{gradient}</defs><rect x=\"0\" y=\"0\" width=\"{vw}\" height=\"{vh}\" fill=\"url(#{gradient_id})\"/></pattern>"
        );

        node.properties.insert("pattern-ref".into(), quote(&id));
        node.properties
            .insert("pattern-width".into(), format!("{vw}px"));
        node.properties
            .insert("pattern-height".into(), format!("{vh}px"));
        node.properties
            .insert("pattern-source".into(), quote(&standalone));
        node.properties.remove("gradient-kind");

        resources.push(format!("{gradient}{pattern}"));
    }

    if !resources.is_empty() {
        let mut properties = BTreeMap::new();
        properties.insert("fill".into(), "none".into());
        properties.insert("stroke".into(), "none".into());
        properties.insert("svg-source-tag".into(), quote("defs"));
        properties.insert(
            "svg-source-xml".into(),
            quote(&format!("<defs>{}</defs>", resources.join(""))),
        );
        scene.nodes.push(SceneNode {
            id: "__srng_radial_resources".into(),
            kind: "group".into(),
            source: scene.source.clone(),
            properties,
            geometry: Geometry {
                x: Some(0.0),
                y: Some(0.0),
                width: Some(0.0),
                height: Some(0.0),
            },
            paint_order: usize::MAX - 1,
            active: true,
        });
    }
}

fn resolve_coord(raw: &str, units: &str, origin: f64, extent: f64) -> f64 {
    let raw = raw.trim();
    if let Some(percent) = raw
        .strip_suffix('%')
        .and_then(|v| v.parse::<f64>().ok())
    {
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

fn svg_stops(value: &str) -> String {
    unquote(value)
        .split(',')
        .filter_map(|record| {
            let fields = record.split_whitespace().collect::<Vec<_>>();
            if fields.len() < 2 {
                return None;
            }
            Some(format!(
                "<stop offset=\"{}\" stop-color=\"{}\"/>",
                escape(fields[0]),
                escape(fields[1])
            ))
        })
        .collect::<Vec<_>>()
        .join("")
}

fn safe_id(value: &str) -> String {
    value
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn unquote(value: &str) -> String {
    let value = value.trim();
    let Some(inner) = value
        .strip_prefix('"')
        .and_then(|v| v.strip_suffix('"'))
    else {
        return value.to_string();
    };
    inner
        .replace("\\n", "\n")
        .replace("\\\"", "\"")
        .replace("\\\\", "\\")
}

fn quote(value: &str) -> String {
    format!(
        "\"{}\"",
        value
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
    )
}

fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentage_object_bbox_coordinates_resolve() {
        assert!((resolve_coord("50%", "objectBoundingBox", 10.0, 20.0) - 20.0).abs() < 1e-9);
    }

    #[test]
    fn stop_records_become_svg_stops() {
        let xml = svg_stops("0 #ff0000ff, 1 #0000ffff");
        assert!(xml.contains("offset=\"0\""));
        assert!(xml.contains("#0000ffff"));
    }
}
