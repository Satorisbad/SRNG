use crate::model::*;
use srng::runtime::{Geometry, Scene};
use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap};

pub fn prepare_scene(scene: &Scene, revision: u64, gate: &RevisionGate) -> PreparedScene {
    let width = clamp_dimension(scene.viewport.width);
    let height = clamp_dimension(scene.viewport.height);
    let mut diagnostics = Vec::new();
    let mut commands = Vec::new();

    let node_map = scene
        .nodes
        .iter()
        .filter(|node| node.active)
        .map(|node| (node.id.as_str(), node))
        .collect::<HashMap<_, _>>();

    let defs_xml = scene
        .nodes
        .iter()
        .filter(|node| node.active)
        .filter_map(|node| {
            let tag = node.properties.get("svg-source-tag").map(|value| unquote(value));
            if tag.as_deref() == Some("defs") {
                node.properties.get("svg-source-xml").map(|value| unquote(value))
            } else {
                None
            }
        })
        .collect::<Vec<_>>()
        .join("\n");

    let mut ordered = Vec::new();
    for node in &scene.nodes {
        if node.active {
            ordered.push((node.paint_order, Source::Node(node)));
        }
    }
    for reference in &scene.references {
        if reference.active {
            ordered.push((reference.paint_order, Source::Reference(reference)));
        }
    }
    ordered.sort_by_key(|(order, _)| *order);

    for (_, source) in ordered {
        if !gate.is_current(revision) {
            break;
        }
        let (id, kind, geometry, props): (
            &str,
            &str,
            Cow<'_, Geometry>,
            Cow<'_, BTreeMap<String, String>>,
        ) = match source {
            Source::Node(node) => (
                node.id.as_str(),
                node.kind.as_str(),
                Cow::Borrowed(&node.geometry),
                Cow::Borrowed(&node.properties),
            ),
            Source::Reference(reference) => {
                let geometry = merge_geometry(reference.linked_geometry.as_ref(), &reference.geometry);
                let mut props = reference.linked_properties.clone();
                props.extend(reference.properties.clone());
                (
                    reference.id.as_str(),
                    reference.resolved_kind.as_deref().unwrap_or("unknown"),
                    Cow::Owned(geometry),
                    Cow::Owned(props),
                )
            }
        };

        let Some(path) = path_for(kind, &geometry, &props) else {
            let (code, message) = if kind == "text" {
                (
                    "G301",
                    format!("text node `{id}` requires pre-shaped outline path data in `data`"),
                )
            } else {
                (
                    "G101",
                    format!("`{id}` has no renderable geometry for kind `{kind}`"),
                )
            };
            diagnostics.push(diag("warning", code, message, id));
            continue;
        };

        let clip_id = props.get("clip").map(|value| unquote(value));
        if let Some(clip_id) = clip_id.as_deref() {
            if let Some(clip_node) = node_map.get(clip_id) {
                if let Some(clip_path) = path_for(
                    &clip_node.kind,
                    &clip_node.geometry,
                    &clip_node.properties,
                ) {
                    commands.push(Command::PushClip {
                        path: clip_path,
                        rule: fill_rule(&clip_node.properties),
                    });
                } else {
                    diagnostics.push(diag(
                        "error",
                        "G210",
                        format!("clip `{clip_id}` has no renderable geometry"),
                        id,
                    ));
                }
            } else {
                diagnostics.push(diag(
                    "error",
                    "G211",
                    format!("clip `{clip_id}` does not exist or is inactive"),
                    id,
                ));
            }
        }

        let mask_svg = match svg_mask_svg(&props, &defs_xml, &geometry, width, height) {
            Ok(mask) => mask,
            Err(message) => {
                diagnostics.push(diag("error", "G223", message, id));
                None
            }
        };
        if let Some(svg) = mask_svg.as_ref() {
            commands.push(Command::PushMaskSvg { svg: svg.clone() });
        }

        match svg_pattern_paint(&props, &defs_xml, &geometry) {
            Ok(Some(paint)) => commands.push(Command::Fill {
                path: path.clone(),
                paint,
                rule: fill_rule(&props),
            }),
            Ok(None) => {
                if let Some(fill) = props.get("fill") {
                    if !is_none_paint(fill) {
                        match parse_paint(fill, &props, &geometry) {
                            Ok(paint) => commands.push(Command::Fill {
                                path: path.clone(),
                                paint,
                                rule: fill_rule(&props),
                            }),
                            Err(message) => diagnostics.push(diag("error", "G220", message, id)),
                        }
                    }
                }
            }
            Err(message) => diagnostics.push(diag("error", "G222", message, id)),
        }

        if let Some(stroke) = props.get("stroke") {
            if !is_none_paint(stroke) {
                match parse_paint(stroke, &props, &geometry) {
                    Ok(paint) => commands.push(Command::Stroke {
                        path: path.clone(),
                        paint,
                        style: stroke_style(&props),
                    }),
                    Err(message) => diagnostics.push(diag("error", "G221", message, id)),
                }
            }
        }

        if mask_svg.is_some() {
            commands.push(Command::PopMask);
        }
        if clip_id.is_some() {
            commands.push(Command::PopClip);
        }
    }

    PreparedScene {
        width,
        height,
        revision,
        commands,
        diagnostics,
    }
}

enum Source<'a> {
    Node(&'a srng::runtime::SceneNode),
    Reference(&'a srng::runtime::SceneReference),
}

fn merge_geometry(linked: Option<&Geometry>, authored: &Geometry) -> Geometry {
    let linked = linked.cloned().unwrap_or_default();
    Geometry {
        x: authored.x.or(linked.x),
        y: authored.y.or(linked.y),
        width: authored.width.or(linked.width),
        height: authored.height.or(linked.height),
    }
}

fn path_for(kind: &str, geometry: &Geometry, props: &BTreeMap<String, String>) -> Option<PathData> {
    if let Some(data) = props.get("data") {
        let value = unquote(data);
        if !value.trim().is_empty() {
            return Some(PathData { svg: value });
        }
    }

    let x = geometry.x?;
    let y = geometry.y?;
    let width = geometry.width?;
    let height = geometry.height?;
    match kind {
        "rect" | "canvas" | "group" | "shadow" => Some(PathData {
            svg: format!("M {x} {y} h {width} v {height} h {} Z", -width),
        }),
        "ellipse" | "circle" => {
            let rx = width / 2.0;
            let ry = height / 2.0;
            let cx = x + rx;
            let cy = y + ry;
            Some(PathData {
                svg: format!(
                    "M {} {cy} A {rx} {ry} 0 1 0 {} {cy} A {rx} {ry} 0 1 0 {} {cy} Z",
                    cx - rx,
                    cx + rx,
                    cx - rx
                ),
            })
        }
        _ => None,
    }
}

fn svg_mask_svg(
    props: &BTreeMap<String, String>,
    defs_xml: &str,
    geometry: &Geometry,
    viewport_width: u16,
    viewport_height: u16,
) -> Result<Option<String>, String> {
    if props
        .get("svg-mask-mode")
        .map(|value| unquote(value))
        .as_deref()
        == Some("binary-opaque-clip")
    {
        return Ok(None);
    }

    let raw = props
        .get("svg-mask-ref")
        .map(|value| unquote(value))
        .or_else(|| props.get("svg-attr-mask").map(|value| unquote(value)));
    let Some(raw) = raw else { return Ok(None); };
    let mask_ref = if let Some(value) = raw.strip_prefix("url(#").and_then(|value| value.strip_suffix(')')) {
        value.to_string()
    } else if !raw.contains('(') && !raw.trim().is_empty() {
        raw.clone()
    } else {
        return Ok(None);
    };
    if defs_xml.trim().is_empty() {
        return Err(format!("SVG mask `{mask_ref}` has no preserved <defs> XML"));
    }

    let x = geometry.x.unwrap_or(0.0);
    let y = geometry.y.unwrap_or(0.0);
    let width = geometry.width.unwrap_or(f64::from(viewport_width));
    let height = geometry.height.unwrap_or(f64::from(viewport_height));
    let escaped_ref = xml_escape_attr(&mask_ref);
    Ok(Some(format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\" width=\"{viewport_width}\" height=\"{viewport_height}\" viewBox=\"0 0 {viewport_width} {viewport_height}\">{defs_xml}<rect x=\"{x}\" y=\"{y}\" width=\"{width}\" height=\"{height}\" fill=\"white\" mask=\"url(#{escaped_ref})\"/></svg>"
    )))
}

fn svg_pattern_paint(
    props: &BTreeMap<String, String>,
    defs_xml: &str,
    geometry: &Geometry,
) -> Result<Option<Paint>, String> {
    let pattern_ref = props
        .get("pattern-ref")
        .or_else(|| props.get("svg-pattern-ref"))
        .map(|value| unquote(value));
    let Some(pattern_ref) = pattern_ref else {
        return Ok(None);
    };

    let stored_width = props
        .get("pattern-width")
        .or_else(|| props.get("svg-pattern-width"))
        .and_then(|value| parse_number(value))
        .filter(|value| value.is_finite() && *value > 0.0)
        .ok_or_else(|| format!("pattern `{pattern_ref}` has no positive tile width"))?;
    let stored_height = props
        .get("pattern-height")
        .or_else(|| props.get("svg-pattern-height"))
        .and_then(|value| parse_number(value))
        .filter(|value| value.is_finite() && *value > 0.0)
        .ok_or_else(|| format!("pattern `{pattern_ref}` has no positive tile height"))?;

    let fallback_pattern = props
        .get("pattern-source")
        .or_else(|| props.get("svg-pattern-source-xml"))
        .map(|value| unquote(value))
        .unwrap_or_default();
    let units = props
        .get("pattern-units")
        .map(|value| unquote(value))
        .unwrap_or_else(|| {
            if fallback_pattern.contains("patternUnits=\"objectBoundingBox\"") {
                "objectBoundingBox".into()
            } else {
                "userSpaceOnUse".into()
            }
        });
    let (tile_width, tile_height) = if units == "objectBoundingBox" {
        (
            stored_width * geometry.width.unwrap_or(1.0).abs(),
            stored_height * geometry.height.unwrap_or(1.0).abs(),
        )
    } else {
        (stored_width, stored_height)
    };
    if !tile_width.is_finite() || !tile_height.is_finite() || tile_width <= 0.0 || tile_height <= 0.0 {
        return Err(format!("pattern `{pattern_ref}` resolves to an empty tile"));
    }

    let native_data = props.get("pattern-data").map(|value| unquote(value));
    let svg = if let Some(data) = native_data {
        let mut shapes = String::new();
        for record in data.lines() {
            let Some((fill, path)) = record.split_once('|') else { continue; };
            shapes.push_str(&format!(
                "<path d=\"{}\" fill=\"{}\"/>",
                xml_escape_attr(path),
                xml_escape_attr(fill)
            ));
        }
        if shapes.is_empty() {
            return Err(format!("pattern `{pattern_ref}` has empty native pattern-data"));
        }
        format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{tile_width}\" height=\"{tile_height}\" viewBox=\"0 0 {tile_width} {tile_height}\">{shapes}</svg>"
        )
    } else {
        let definitions = if defs_xml.trim().is_empty() {
            if fallback_pattern.trim().is_empty() {
                return Err(format!("pattern `{pattern_ref}` has no native data or preserved definition"));
            }
            format!("<defs>{fallback_pattern}</defs>")
        } else {
            defs_xml.to_string()
        };
        let escaped_ref = xml_escape_attr(&pattern_ref);
        format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\" width=\"{tile_width}\" height=\"{tile_height}\" viewBox=\"0 0 {tile_width} {tile_height}\">{definitions}<rect x=\"0\" y=\"0\" width=\"{tile_width}\" height=\"{tile_height}\" fill=\"url(#{escaped_ref})\"/></svg>"
        )
    };

    Ok(Some(Paint::SvgPattern {
        svg,
        tile_width,
        tile_height,
    }))
}

fn xml_escape_attr(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn parse_paint(
    value: &str,
    props: &BTreeMap<String, String>,
    geometry: &Geometry,
) -> Result<Paint, String> {
    let value = value.trim();
    if value == "linear-gradient" || value.starts_with("linear-gradient(") {
        let stops_text = props
            .get("gradient-stops")
            .ok_or_else(|| "linear gradient requires `gradient-stops`".to_string())?;
        let stops = parse_gradient_stops(stops_text)?;
        if stops.len() < 2 {
            return Err("linear gradient requires at least two stops".to_string());
        }
        let x = geometry.x.unwrap_or(0.0);
        let y = geometry.y.unwrap_or(0.0);
        let width = geometry.width.unwrap_or(1.0);
        let height = geometry.height.unwrap_or(0.0);
        let start = props
            .get("gradient-start")
            .and_then(|value| parse_pair(value))
            .unwrap_or((x, y));
        let end = props
            .get("gradient-end")
            .and_then(|value| parse_pair(value))
            .unwrap_or((x + width, y + height));
        return Ok(Paint::LinearGradient { start, end, stops });
    }
    if value == "radial-gradient" || value.starts_with("radial-gradient(") {
        let stops_text = props
            .get("gradient-stops")
            .ok_or_else(|| "radial gradient requires `gradient-stops`".to_string())?;
        let stops = parse_gradient_stops(stops_text)?;
        if stops.len() < 2 {
            return Err("radial gradient requires at least two stops".to_string());
        }
        let x = geometry.x.unwrap_or(0.0);
        let y = geometry.y.unwrap_or(0.0);
        let width = geometry.width.unwrap_or(1.0).abs();
        let height = geometry.height.unwrap_or(1.0).abs();
        let cx = props.get("gradient-cx").and_then(|v| parse_number(v)).unwrap_or(x + width / 2.0);
        let cy = props.get("gradient-cy").and_then(|v| parse_number(v)).unwrap_or(y + height / 2.0);
        let fx = props.get("gradient-fx").and_then(|v| parse_number(v)).unwrap_or(cx);
        let fy = props.get("gradient-fy").and_then(|v| parse_number(v)).unwrap_or(cy);
        let radius = props
            .get("gradient-r")
            .and_then(|v| parse_number(v))
            .unwrap_or(width.max(height) / 2.0);
        if !radius.is_finite() || radius <= 0.0 {
            return Err("radial gradient radius must be a positive finite number".to_string());
        }
        return Ok(Paint::RadialGradient {
            center: (cx, cy),
            focal: (fx, fy),
            radius,
            stops,
        });
    }
    parse_color(value).map(Paint::Solid)
}

fn parse_gradient_stops(value: &str) -> Result<Vec<GradientStop>, String> {
    value
        .split(',')
        .map(|part| {
            let fields = part.split_whitespace().collect::<Vec<_>>();
            if fields.len() < 2 {
                return Err("gradient stop requires offset and color".to_string());
            }

            let (offset_text, color_text, percent) = if fields.len() >= 3 && fields[1] == "%" {
                (fields[0], fields[2], true)
            } else if let Some(offset) = fields[0].strip_suffix('%') {
                (offset, fields[1], true)
            } else {
                (fields[0], fields[1], false)
            };

            let mut offset = offset_text
                .parse::<f32>()
                .map_err(|_| "invalid gradient stop offset".to_string())?;
            if percent {
                offset /= 100.0;
            }
            if !(0.0..=1.0).contains(&offset) {
                return Err("gradient stop offset must be in 0..=1".to_string());
            }

            Ok(GradientStop {
                offset,
                color: parse_color(color_text)?,
            })
        })
        .collect()
}

fn parse_color(value: &str) -> Result<Rgba, String> {
    let value = unquote(value);
    let hex = value.strip_prefix('#').ok_or_else(|| {
        format!("unsupported paint `{value}`; use #RGB, #RGBA, #RRGGBB or #RRGGBBAA")
    })?;

    fn byte(value: &str) -> Result<u8, String> {
        u8::from_str_radix(value, 16).map_err(|_| "invalid hexadecimal color".to_string())
    }

    match hex.len() {
        3 => Ok(Rgba {
            r: byte(&hex[0..1].repeat(2))?,
            g: byte(&hex[1..2].repeat(2))?,
            b: byte(&hex[2..3].repeat(2))?,
            a: 255,
        }),
        4 => Ok(Rgba {
            r: byte(&hex[0..1].repeat(2))?,
            g: byte(&hex[1..2].repeat(2))?,
            b: byte(&hex[2..3].repeat(2))?,
            a: byte(&hex[3..4].repeat(2))?,
        }),
        6 => Ok(Rgba {
            r: byte(&hex[0..2])?,
            g: byte(&hex[2..4])?,
            b: byte(&hex[4..6])?,
            a: 255,
        }),
        8 => Ok(Rgba {
            r: byte(&hex[0..2])?,
            g: byte(&hex[2..4])?,
            b: byte(&hex[4..6])?,
            a: byte(&hex[6..8])?,
        }),
        _ => Err("invalid hexadecimal color length".to_string()),
    }
}

fn fill_rule(props: &BTreeMap<String, String>) -> FillRule {
    match props.get("fill-rule").map(|value| unquote(value)).as_deref() {
        Some("evenodd") | Some("even-odd") => FillRule::EvenOdd,
        _ => FillRule::NonZero,
    }
}

fn stroke_style(props: &BTreeMap<String, String>) -> StrokeStyle {
    let mut style = StrokeStyle::default();
    if let Some(value) = props.get("stroke-width").and_then(|value| parse_number(value)) {
        style.width = value.max(0.0);
    }
    if let Some(value) = props
        .get("stroke-miterlimit")
        .and_then(|value| parse_number(value))
    {
        style.miter_limit = value.max(0.0);
    }
    if let Some(value) = props
        .get("stroke-dashoffset")
        .and_then(|value| parse_number(value))
    {
        style.dash_offset = value;
    }
    if let Some(value) = props.get("stroke-dasharray") {
        style.dash = value
            .split(|character: char| character == ',' || character.is_whitespace())
            .filter_map(parse_number)
            .filter(|value| *value >= 0.0)
            .collect();
    }
    style.line_cap = match props
        .get("stroke-linecap")
        .map(|value| unquote(value))
        .as_deref()
    {
        Some("round") => LineCap::Round,
        Some("square") => LineCap::Square,
        _ => LineCap::Butt,
    };
    style.line_join = match props
        .get("stroke-linejoin")
        .map(|value| unquote(value))
        .as_deref()
    {
        Some("round") => LineJoin::Round,
        Some("bevel") => LineJoin::Bevel,
        _ => LineJoin::Miter,
    };
    style
}

fn parse_pair(value: &str) -> Option<(f64, f64)> {
    let mut values = value.split_whitespace().filter_map(parse_number);
    Some((values.next()?, values.next()?))
}

fn parse_number(value: &str) -> Option<f64> {
    let value = unquote(value);
    let trimmed = value.trim();
    let numeric = trimmed
        .strip_suffix("px")
        .map(str::trim)
        .unwrap_or(trimmed);
    numeric.parse().ok()
}

fn unquote(value: &str) -> String {
    let trimmed = value.trim();
    let Some(inner) = trimmed
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
    else {
        return trimmed.to_string();
    };
    unescape_string(inner)
}

fn unescape_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        match chars.next() {
            Some('\\') => out.push('\\'),
            Some('"') => out.push('"'),
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('t') => out.push('\t'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

fn is_none_paint(value: &str) -> bool {
    unquote(value).eq_ignore_ascii_case("none")
}

fn clamp_dimension(value: f64) -> u16 {
    value.round().clamp(1.0, u16::MAX as f64) as u16
}

fn diag(severity: &str, code: &str, message: String, id: &str) -> RenderDiagnostic {
    RenderDiagnostic {
        severity: severity.to_string(),
        code: code.to_string(),
        message,
        declaration: Some(id.to_string()),
    }
}
