use kurbo::{Affine, BezPath};
use srng::runtime::{Geometry, Scene, SceneNode};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::f64::consts::PI;

#[derive(Clone, Copy, Debug, PartialEq)]
struct Matrix {
    a: f64,
    b: f64,
    c: f64,
    d: f64,
    e: f64,
    f: f64,
}

impl Matrix {
    const ID: Self = Self { a: 1.0, b: 0.0, c: 0.0, d: 1.0, e: 0.0, f: 0.0 };

    fn mul(self, rhs: Self) -> Self {
        Self {
            a: self.a * rhs.a + self.c * rhs.b,
            b: self.b * rhs.a + self.d * rhs.b,
            c: self.a * rhs.c + self.c * rhs.d,
            d: self.b * rhs.c + self.d * rhs.d,
            e: self.a * rhs.e + self.c * rhs.f + self.e,
            f: self.b * rhs.e + self.d * rhs.f + self.f,
        }
    }

    fn translate(x: f64, y: f64) -> Self { Self { e: x, f: y, ..Self::ID } }
    fn scale(x: f64, y: f64) -> Self { Self { a: x, d: y, ..Self::ID } }
    fn rotate(rad: f64) -> Self {
        let (s, c) = rad.sin_cos();
        Self { a: c, b: s, c: -s, d: c, e: 0.0, f: 0.0 }
    }
    fn skew(x: f64, y: f64) -> Self { Self { a: 1.0, b: y.tan(), c: x.tan(), d: 1.0, e: 0.0, f: 0.0 } }
    fn point(self, x: f64, y: f64) -> (f64, f64) {
        (self.a * x + self.c * y + self.e, self.b * x + self.d * y + self.f)
    }
    fn affine(self) -> Affine { Affine::new([self.a, self.b, self.c, self.d, self.e, self.f]) }
    fn svg(self) -> String {
        format!("matrix({} {} {} {} {} {})", fmt(self.a), fmt(self.b), fmt(self.c), fmt(self.d), fmt(self.e), fmt(self.f))
    }
}

/// Bakes viewBox/transforms into geometry, resolves per-node paint opacity,
/// prepares native masks/clips, expands simple uses and materializes embedded
/// image resources. This mutates a working scene owned by the single semantic
/// pipeline rather than delegating through another prepare_scene layer.
pub(super) fn normalize(scene: &mut Scene) {
    let parent = parent_map(scene);
    let root_view = viewbox_roots(scene, &parent);
    let mut memo = HashMap::new();
    let mut visiting = HashSet::new();
    let ids = scene.nodes.iter().map(|n| n.id.clone()).collect::<Vec<_>>();
    let props = scene.nodes.iter().map(|n| (n.id.clone(), n.properties.clone())).collect::<HashMap<_, _>>();
    let mut world = HashMap::new();
    for id in &ids {
        let matrix = world_matrix(id, &parent, &props, &root_view, &mut memo, &mut visiting);
        world.insert(id.clone(), matrix);
    }

    let clip_ids = scene.nodes.iter()
        .filter_map(|n| n.properties.get("clip-units").map(|_| n.id.clone()))
        .collect::<HashSet<_>>();
    let viewport_width = scene.viewport.width;
    let viewport_height = scene.viewport.height;

    for node in &mut scene.nodes {
        let matrix = *world.get(&node.id).unwrap_or(&Matrix::ID);
        preprocess_paint(node, matrix);
        preprocess_mask(node, matrix);

        if node.kind == "text" && !node.properties.contains_key("data") {
            continue;
        }

        if node.kind == "group" && node.properties.contains_key("use-data") {
            let x = prop_number(&node.properties, "source-x").unwrap_or(0.0);
            let y = prop_number(&node.properties, "source-y").unwrap_or(0.0);
            if let Some(data) = node.properties.get("use-data").map(|v| unquote(v)) {
                node.properties.insert("data".into(), quote(&data));
                if let Some(fill) = node.properties.get("use-fill").cloned() {
                    node.properties.insert("fill".into(), fill);
                }
                if let Some(stroke) = node.properties.get("use-stroke").cloned() {
                    node.properties.insert("stroke".into(), stroke);
                }
                bake_node_path(node, matrix.mul(Matrix::translate(x, y)));
            }
            continue;
        }

        if node.kind == "group" {
            if let Some(href) = node.properties.get("href").map(|v| unquote(v)) {
                if href.starts_with("data:image/") {
                    synthesize_image(node, matrix, viewport_width, viewport_height, &href);
                    continue;
                }
            }
        }

        if !clip_ids.contains(&node.id) && node.kind != "canvas" {
            bake_node_path(node, matrix);
        }
    }

    materialize_target_clips(scene, &world);

    for reference in &mut scene.references {
        apply_opacity(&mut reference.properties, 1.0);
        if let Some(transform) = reference.properties.get("transform").map(|v| parse_transform(&unquote(v))) {
            if let Some(data) = reference.properties.get("data").map(|v| unquote(v)) {
                if let Some(out) = transform_path(&data, transform) {
                    reference.properties.insert("data".into(), quote(&out));
                }
            }
        }
    }
}

fn parent_map(scene: &Scene) -> HashMap<String, String> {
    scene.relations.iter()
        .filter(|r| r.active && r.kind.as_deref() == Some("contains"))
        .map(|r| (r.to.clone(), r.from.clone()))
        .collect()
}

fn viewbox_roots(scene: &Scene, parent: &HashMap<String, String>) -> HashMap<String, Matrix> {
    let mut out = HashMap::new();
    for node in &scene.nodes {
        if parent.contains_key(&node.id) { continue; }
        let Some(value) = node.properties.get("viewbox").map(|v| unquote(v)) else { continue; };
        if let Some(matrix) = viewbox_matrix(
            &value,
            node.properties.get("preserve-aspect-ratio").map(|v| unquote(v)).as_deref(),
            scene.viewport.width,
            scene.viewport.height,
        ) {
            out.insert(node.id.clone(), matrix);
        }
    }
    out
}

fn world_matrix(
    id: &str,
    parent: &HashMap<String, String>,
    props: &HashMap<String, BTreeMap<String, String>>,
    roots: &HashMap<String, Matrix>,
    memo: &mut HashMap<String, Matrix>,
    visiting: &mut HashSet<String>,
) -> Matrix {
    if let Some(matrix) = memo.get(id) { return *matrix; }
    if !visiting.insert(id.to_string()) { return Matrix::ID; }
    let local = props.get(id)
        .and_then(|p| p.get("transform"))
        .map(|v| parse_transform(&unquote(v)))
        .unwrap_or(Matrix::ID);
    let base = if let Some(parent_id) = parent.get(id) {
        world_matrix(parent_id, parent, props, roots, memo, visiting)
    } else {
        roots.get(id).copied().unwrap_or(Matrix::ID)
    };
    let matrix = base.mul(local);
    visiting.remove(id);
    memo.insert(id.to_string(), matrix);
    matrix
}

fn viewbox_matrix(value: &str, preserve: Option<&str>, viewport_width: f64, viewport_height: f64) -> Option<Matrix> {
    let numbers = value
        .split(|c: char| c == ',' || c.is_whitespace())
        .filter(|s| !s.is_empty())
        .map(|s| s.parse::<f64>().ok())
        .collect::<Option<Vec<_>>>()?;
    if numbers.len() != 4 || numbers[2] <= 0.0 || numbers[3] <= 0.0 { return None; }
    let (x, y, width, height) = (numbers[0], numbers[1], numbers[2], numbers[3]);
    let sx = viewport_width / width;
    let sy = viewport_height / height;
    let preserve = preserve.unwrap_or("xMidYMid meet");
    if preserve.trim() == "none" {
        return Some(Matrix::translate(-x * sx, -y * sy).mul(Matrix::scale(sx, sy)));
    }
    let scale = if preserve.contains("slice") { sx.max(sy) } else { sx.min(sy) };
    let extra_x = viewport_width - width * scale;
    let extra_y = viewport_height - height * scale;
    let align_x = if preserve.contains("xMin") { 0.0 } else if preserve.contains("xMax") { extra_x } else { extra_x / 2.0 };
    let align_y = if preserve.contains("YMin") { 0.0 } else if preserve.contains("YMax") { extra_y } else { extra_y / 2.0 };
    Some(Matrix::translate(align_x - x * scale, align_y - y * scale).mul(Matrix::scale(scale, scale)))
}

fn parse_transform(value: &str) -> Matrix {
    let mut out = Matrix::ID;
    let mut rest = value.trim();
    while let Some(open) = rest.find('(') {
        let name = rest[..open].trim();
        let after = &rest[open + 1..];
        let Some(close) = after.find(')') else { break; };
        let args = after[..close]
            .split(|c: char| c == ',' || c.is_whitespace())
            .filter(|s| !s.is_empty())
            .filter_map(|s| s.parse::<f64>().ok())
            .collect::<Vec<_>>();
        let matrix = match name {
            "matrix" if args.len() >= 6 => Matrix { a: args[0], b: args[1], c: args[2], d: args[3], e: args[4], f: args[5] },
            "translate" if !args.is_empty() => Matrix::translate(args[0], *args.get(1).unwrap_or(&0.0)),
            "scale" if !args.is_empty() => Matrix::scale(args[0], *args.get(1).unwrap_or(&args[0])),
            "rotate" if !args.is_empty() => {
                let rotation = Matrix::rotate(args[0] * PI / 180.0);
                if args.len() >= 3 {
                    Matrix::translate(args[1], args[2]).mul(rotation).mul(Matrix::translate(-args[1], -args[2]))
                } else { rotation }
            }
            "skewX" if !args.is_empty() => Matrix::skew(args[0] * PI / 180.0, 0.0),
            "skewY" if !args.is_empty() => Matrix::skew(0.0, args[0] * PI / 180.0),
            _ => Matrix::ID,
        };
        out = out.mul(matrix);
        rest = &after[close + 1..];
    }
    out
}

fn bake_node_path(node: &mut SceneNode, matrix: Matrix) {
    let Some(path) = node_path(node) else { return; };
    if let Some(out) = transform_path(&path, matrix) {
        node.properties.insert("data".into(), quote(&out));
    }
    if let Some(geometry) = transformed_bbox(&node.geometry, matrix) {
        node.geometry = geometry;
    }
    node.properties.remove("transform");
}

fn node_path(node: &SceneNode) -> Option<String> {
    if let Some(data) = node.properties.get("data").map(|v| unquote(v)) {
        if !data.trim().is_empty() { return Some(data); }
    }
    let Geometry { x: Some(x), y: Some(y), width: Some(width), height: Some(height) } = node.geometry else { return None; };
    match node.kind.as_str() {
        "rect" | "group" | "shadow" => Some(format!("M {x} {y} H {} V {} H {x} Z", x + width, y + height)),
        "circle" | "ellipse" => {
            let rx = width / 2.0;
            let ry = height / 2.0;
            let cx = x + rx;
            let cy = y + ry;
            Some(format!("M {} {cy} A {rx} {ry} 0 1 0 {} {cy} A {rx} {ry} 0 1 0 {} {cy} Z", cx - rx, cx + rx, cx - rx))
        }
        _ => None,
    }
}

fn transform_path(data: &str, matrix: Matrix) -> Option<String> {
    let mut path = BezPath::from_svg(data).ok()?;
    path.apply_affine(matrix.affine());
    Some(path.to_svg())
}

fn transformed_bbox(geometry: &Geometry, matrix: Matrix) -> Option<Geometry> {
    let (x, y, width, height) = (geometry.x?, geometry.y?, geometry.width?, geometry.height?);
    let points = [
        matrix.point(x, y),
        matrix.point(x + width, y),
        matrix.point(x, y + height),
        matrix.point(x + width, y + height),
    ];
    let min_x = points.iter().map(|p| p.0).fold(f64::INFINITY, f64::min);
    let max_x = points.iter().map(|p| p.0).fold(f64::NEG_INFINITY, f64::max);
    let min_y = points.iter().map(|p| p.1).fold(f64::INFINITY, f64::min);
    let max_y = points.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max);
    Some(Geometry { x: Some(min_x), y: Some(min_y), width: Some(max_x - min_x), height: Some(max_y - min_y) })
}

fn preprocess_paint(node: &mut SceneNode, matrix: Matrix) {
    let inherited_opacity = prop_number(&node.properties, "opacity").unwrap_or(1.0).clamp(0.0, 1.0);
    apply_opacity(&mut node.properties, inherited_opacity);
    let kind = node.properties.get("gradient-kind").map(|v| unquote(v));
    if kind.as_deref() != Some("linear-gradient") { return; }

    let units = node.properties.get("gradient-units").map(|v| unquote(v)).unwrap_or_else(|| "objectBoundingBox".into());
    let geometry = &node.geometry;
    let x = geometry.x.unwrap_or(0.0);
    let y = geometry.y.unwrap_or(0.0);
    let width = geometry.width.unwrap_or(1.0);
    let height = geometry.height.unwrap_or(1.0);
    let coord = |key: &str, axis_x: bool, default: &str| {
        resolve_coord(
            node.properties.get(key).map(|v| unquote(v)).as_deref().unwrap_or(default),
            &units,
            if axis_x { x } else { y },
            if axis_x { width } else { height },
        )
    };
    let start = matrix.point(coord("gradient-x1", true, "0%"), coord("gradient-y1", false, "0%"));
    let end = matrix.point(coord("gradient-x2", true, "100%"), coord("gradient-y2", false, "0%"));
    node.properties.insert("gradient-start".into(), format!("{}px {}px", fmt(start.0), fmt(start.1)));
    node.properties.insert("gradient-end".into(), format!("{}px {}px", fmt(end.0), fmt(end.1)));
    node.properties.insert("fill".into(), "linear-gradient".into());
}

fn resolve_coord(raw: &str, units: &str, origin: f64, extent: f64) -> f64 {
    let raw = raw.trim();
    if let Some(percent) = raw.strip_suffix('%').and_then(|v| v.parse::<f64>().ok()) {
        return if units == "objectBoundingBox" { origin + extent * percent / 100.0 } else { percent / 100.0 };
    }
    let value = raw.trim_end_matches("px").parse::<f64>().unwrap_or(0.0);
    if units == "objectBoundingBox" { origin + extent * value } else { value }
}

fn apply_opacity(props: &mut BTreeMap<String, String>, base: f64) {
    let fill_opacity = base * prop_number(props, "fill-opacity").unwrap_or(1.0);
    let stroke_opacity = base * prop_number(props, "stroke-opacity").unwrap_or(1.0);
    if let Some(value) = props.get("fill").cloned() {
        if let Some(color) = with_alpha(&value, fill_opacity) { props.insert("fill".into(), color); }
    }
    if let Some(value) = props.get("stroke").cloned() {
        if let Some(color) = with_alpha(&value, stroke_opacity) { props.insert("stroke".into(), color); }
    }
    if let Some(value) = props.get("gradient-stops").cloned() {
        props.insert("gradient-stops".into(), opacity_stops(&value, fill_opacity));
    }
    if let Some(value) = props.get("pattern-data").cloned() {
        props.insert("pattern-data".into(), opacity_records(&value, fill_opacity, false));
    }
    props.remove("opacity");
    props.remove("fill-opacity");
    props.remove("stroke-opacity");
}

fn preprocess_mask(node: &mut SceneNode, matrix: Matrix) {
    let Some(data) = node.properties.get("mask-data").cloned() else { return; };
    let mode = node.properties.get("mask-type").map(|v| unquote(v)).unwrap_or_else(|| "luminance".into());
    let units = node.properties.get("mask-content-units").map(|v| unquote(v)).unwrap_or_else(|| "userSpaceOnUse".into());
    let bbox = if units == "objectBoundingBox" {
        let geometry = &node.geometry;
        Matrix::translate(geometry.x.unwrap_or(0.0), geometry.y.unwrap_or(0.0))
            .mul(Matrix::scale(geometry.width.unwrap_or(1.0), geometry.height.unwrap_or(1.0)))
    } else { Matrix::ID };
    node.properties.insert("mask-data".into(), opacity_records(&data, 1.0, mode == "luminance"));
    let decoded = unquote(node.properties.get("mask-data").unwrap());
    node.properties.insert("mask-data".into(), quote(&transform_records(&decoded, matrix.mul(bbox))));
}

fn opacity_records(value: &str, opacity: f64, luminance: bool) -> String {
    let data = unquote(value);
    let mut out = Vec::new();
    for line in data.lines() {
        let Some((color, path)) = line.split_once('|') else { continue; };
        let color = if luminance { luminance_alpha(color) } else { with_alpha(color, opacity).unwrap_or_else(|| color.to_string()) };
        out.push(format!("{color}|{path}"));
    }
    quote(&out.join("\n"))
}

fn transform_records(value: &str, matrix: Matrix) -> String {
    value.lines()
        .filter_map(|line| {
            let (color, path) = line.split_once('|')?;
            transform_path(path, matrix).map(|path| format!("{color}|{path}"))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn materialize_target_clips(scene: &mut Scene, world: &HashMap<String, Matrix>) {
    let clips = scene.nodes.iter().map(|node| (node.id.clone(), node.clone())).collect::<HashMap<_, _>>();
    let mut extra = Vec::new();
    for node in &mut scene.nodes {
        let Some(clip_id) = node.properties.get("clip").map(|v| unquote(v)) else { continue; };
        let Some(source) = clips.get(&clip_id) else { continue; };
        let units = source.properties.get("clip-units").map(|v| unquote(v)).unwrap_or_else(|| "userSpaceOnUse".into());
        let Some(data) = source.properties.get("data").map(|v| unquote(v)) else { continue; };
        let base = *world.get(&node.id).unwrap_or(&Matrix::ID);
        let matrix = if units == "objectBoundingBox" {
            let geometry = &node.geometry;
            Matrix::translate(geometry.x.unwrap_or(0.0), geometry.y.unwrap_or(0.0))
                .mul(Matrix::scale(geometry.width.unwrap_or(1.0), geometry.height.unwrap_or(1.0)))
        } else { base };
        let Some(path) = transform_path(&data, matrix) else { continue; };
        let mut clone = source.clone();
        clone.id = format!("__clip_{}", node.id);
        clone.properties.insert("data".into(), quote(&path));
        clone.properties.remove("clip-units");
        node.properties.insert("clip".into(), clone.id.clone());
        extra.push(clone);
    }
    scene.nodes.extend(extra);
}

fn synthesize_image(node: &mut SceneNode, matrix: Matrix, viewport_width: f64, viewport_height: f64, href: &str) {
    let x = prop_number(&node.properties, "source-x").unwrap_or(0.0);
    let y = prop_number(&node.properties, "source-y").unwrap_or(0.0);
    let width = prop_number(&node.properties, "source-width").unwrap_or(0.0);
    let height = prop_number(&node.properties, "source-height").unwrap_or(0.0);
    if width <= 0.0 || height <= 0.0 { return; }
    let id = format!("__image_{}", xml_escape(&node.id));
    let preserve = node.properties.get("image-preserve-aspect-ratio").map(|v| unquote(v)).unwrap_or_else(|| "xMidYMid meet".into());
    let xml = format!(
        "<pattern id=\"{id}\" patternUnits=\"userSpaceOnUse\" width=\"{viewport_width}\" height=\"{viewport_height}\"><image href=\"{}\" x=\"{x}\" y=\"{y}\" width=\"{width}\" height=\"{height}\" preserveAspectRatio=\"{}\" transform=\"{}\"/></pattern>",
        xml_escape(href), xml_escape(&preserve), matrix.svg()
    );
    full_view_pattern(node, &id, &xml, viewport_width, viewport_height);
}

fn full_view_pattern(node: &mut SceneNode, id: &str, xml: &str, viewport_width: f64, viewport_height: f64) {
    node.geometry = Geometry { x: Some(0.0), y: Some(0.0), width: Some(viewport_width), height: Some(viewport_height) };
    node.properties.insert("data".into(), quote(&format!("M 0 0 H {viewport_width} V {viewport_height} H 0 Z")));
    node.properties.insert("pattern-ref".into(), quote(id));
    node.properties.insert("pattern-width".into(), format!("{viewport_width}px"));
    node.properties.insert("pattern-height".into(), format!("{viewport_height}px"));
    node.properties.insert("pattern-source".into(), quote(xml));
}

fn prop_number(properties: &BTreeMap<String, String>, key: &str) -> Option<f64> {
    unquote(properties.get(key)?).trim().trim_end_matches("px").parse().ok()
}

fn with_alpha(value: &str, opacity: f64) -> Option<String> {
    let value = unquote(value);
    let hex = value.strip_prefix('#')?;
    let byte = |slice: &str| u8::from_str_radix(slice, 16).ok();
    let (r, g, b, a) = match hex.len() {
        3 => (byte(&hex[0..1].repeat(2))?, byte(&hex[1..2].repeat(2))?, byte(&hex[2..3].repeat(2))?, 255),
        4 => (byte(&hex[0..1].repeat(2))?, byte(&hex[1..2].repeat(2))?, byte(&hex[2..3].repeat(2))?, byte(&hex[3..4].repeat(2))?),
        6 => (byte(&hex[0..2])?, byte(&hex[2..4])?, byte(&hex[4..6])?, 255),
        8 => (byte(&hex[0..2])?, byte(&hex[2..4])?, byte(&hex[4..6])?, byte(&hex[6..8])?),
        _ => return None,
    };
    Some(format!("#{r:02x}{g:02x}{b:02x}{:02x}", (f64::from(a) * opacity.clamp(0.0, 1.0)).round() as u8))
}

fn luminance_alpha(value: &str) -> String {
    let value = unquote(value);
    let Some(hex) = value.strip_prefix('#') else { return "#ffffff00".into(); };
    let hex = if hex.len() == 3 {
        format!("{}{}{}{}{}{}", &hex[0..1], &hex[0..1], &hex[1..2], &hex[1..2], &hex[2..3], &hex[2..3])
    } else { hex[..hex.len().min(6)].to_string() };
    if hex.len() < 6 { return "#ffffff00".into(); }
    let r = u8::from_str_radix(&hex[0..2], 16).unwrap_or(0);
    let g = u8::from_str_radix(&hex[2..4], 16).unwrap_or(0);
    let b = u8::from_str_radix(&hex[4..6], 16).unwrap_or(0);
    let alpha = (0.2126 * f64::from(r) + 0.7152 * f64::from(g) + 0.0722 * f64::from(b)).round() as u8;
    format!("#ffffff{alpha:02x}")
}

fn opacity_stops(value: &str, opacity: f64) -> String {
    unquote(value).split(',').map(|record| {
        let mut fields = record.split_whitespace();
        let offset = fields.next().unwrap_or("0");
        let color = fields.next().unwrap_or("#000000");
        format!("{offset} {}", with_alpha(color, opacity).unwrap_or_else(|| color.to_string()))
    }).collect::<Vec<_>>().join(", ")
}

fn unquote(value: &str) -> String {
    let value = value.trim();
    let Some(inner) = value.strip_prefix('"').and_then(|v| v.strip_suffix('"')) else { return value.to_string(); };
    inner.replace("\\n", "\n").replace("\\\"", "\"").replace("\\\\", "\\")
}

fn quote(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n"))
}

fn fmt(value: f64) -> String {
    if value.fract().abs() < 1e-9 { format!("{}", value as i64) }
    else { format!("{value:.6}").trim_end_matches('0').trim_end_matches('.').to_string() }
}

fn xml_escape(value: &str) -> String {
    value.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&apos;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transform_parser_handles_full_svg_list() {
        let matrix = parse_transform("translate(10 20) scale(2) rotate(90)");
        let point = matrix.point(1.0, 0.0);
        assert!((point.0 - 10.0).abs() < 1e-6);
        assert!((point.1 - 22.0).abs() < 1e-6);
    }

    #[test]
    fn viewbox_meet_centers_content() {
        let matrix = viewbox_matrix("0 0 100 100", Some("xMidYMid meet"), 200.0, 100.0).unwrap();
        assert_eq!(matrix.point(0.0, 0.0), (50.0, 0.0));
    }

    #[test]
    fn luminance_mask_converts_gray_to_alpha() {
        assert_eq!(luminance_alpha("#808080"), "#ffffff80");
    }
}
