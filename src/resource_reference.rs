use crate::svg_base::{ImportDiagnostic, ImportResult};
use roxmltree::{Document as XmlDocument, Node};
use std::collections::{BTreeMap, HashSet};

const INHERITED_STYLE_KEYS: &[&str] = &[
    "fill",
    "stroke",
    "fill-rule",
    "stroke-width",
    "stroke-linecap",
    "stroke-linejoin",
    "stroke-dasharray",
    "stroke-dashoffset",
    "opacity",
    "fill-opacity",
    "stroke-opacity",
    "visibility",
];

#[derive(Debug, Clone)]
struct ResourceNode {
    kind: String,
    id: String,
    properties: BTreeMap<String, String>,
    parent: Option<String>,
}

#[derive(Debug, Clone)]
struct UseInstance {
    id: String,
    target: String,
    properties: BTreeMap<String, String>,
}

pub(crate) fn promote_reusable_resources(svg: &str, result: &mut ImportResult) {
    let Ok(document) = XmlDocument::parse(svg) else { return; };
    let root = document.root_element();
    let (viewport_width, viewport_height) = root_viewport(root);
    let definitions = resource_roots(&document);
    let mut output = result.source.clone();
    let existing_ids = declared_ids(&output);
    let mut emitted = HashSet::<String>::new();
    let mut resource_nodes = Vec::<ResourceNode>::new();
    let mut reference_nodes = Vec::<UseInstance>::new();

    for definition in definitions {
        let Some(raw_id) = definition.attribute("id") else { continue; };
        let id = safe_ident(raw_id);
        if existing_ids.contains(&id) {
            continue;
        }
        emit_resource_node(
            definition,
            None,
            &BTreeMap::new(),
            viewport_width,
            viewport_height,
            &mut emitted,
            &mut resource_nodes,
            &mut reference_nodes,
            &mut result.diagnostics,
        );
    }

    if !resource_nodes.is_empty() || !reference_nodes.is_empty() {
        output.push_str("\n// Native reusable resource declarations. Resource-only nodes do not paint until referenced.\n");
    }
    for node in &resource_nodes {
        write_node(&mut output, node);
    }
    for reference in &reference_nodes {
        write_reference(&mut output, reference);
    }
    for node in &resource_nodes {
        if let Some(parent) = &node.parent {
            output.push_str(&format!("relation {parent} -> {} {{\n    kind: contains;\n}}\n\n", node.id));
        }
    }
    for reference in &reference_nodes {
        if let Some(parent) = reference.properties.get("resource-parent") {
            output.push_str(&format!("relation {} -> {} {{\n    kind: contains;\n}}\n\n", unquote(parent), reference.id));
        }
    }
    result.source = output;
}

fn resource_roots<'a>(document: &'a XmlDocument<'a>) -> Vec<Node<'a, 'a>> {
    let mut roots = Vec::new();
    let referenced = document
        .descendants()
        .filter(|node| node.is_element() && node.tag_name().name() == "use")
        .filter_map(local_href)
        .collect::<HashSet<_>>();

    for node in document.descendants().filter(|node| node.is_element()) {
        let Some(id) = node.attribute("id") else { continue; };
        if !referenced.contains(id) {
            continue;
        }
        if matches!(node.tag_name().name(), "linearGradient" | "radialGradient" | "pattern" | "mask" | "clipPath" | "filter") {
            continue;
        }
        let ancestor_is_same_resource = node
            .ancestors()
            .skip(1)
            .any(|ancestor| ancestor.is_element() && ancestor.attribute("id").is_some_and(|ancestor_id| referenced.contains(ancestor_id)));
        if !ancestor_is_same_resource {
            roots.push(node);
        }
    }
    roots
}

#[allow(clippy::too_many_arguments)]
fn emit_resource_node(
    node: Node<'_, '_>,
    parent: Option<String>,
    inherited: &BTreeMap<String, String>,
    viewport_width: f64,
    viewport_height: f64,
    emitted: &mut HashSet<String>,
    nodes: &mut Vec<ResourceNode>,
    references: &mut Vec<UseInstance>,
    diagnostics: &mut Vec<ImportDiagnostic>,
) {
    let tag = node.tag_name().name();
    if matches!(tag, "metadata" | "title" | "desc" | "linearGradient" | "radialGradient" | "pattern" | "mask" | "clipPath" | "filter") {
        return;
    }

    let id = node.attribute("id").map(safe_ident).unwrap_or_else(|| generated_child_id(parent.as_deref().unwrap_or("resource"), nodes.len() + references.len() + 1));
    if !emitted.insert(id.clone()) {
        return;
    }
    let local = collect_style(node);
    let mut effective = inherited.clone();
    effective.extend(local.clone());

    if tag == "use" {
        let Some(target) = local_href(node) else {
            diagnostics.push(diag("warning", "S310", "resource <use> has no local href target", Some(&id)));
            return;
        };
        let mut props = reference_properties(node, &effective, viewport_width, viewport_height);
        props.insert("resource-only".into(), "true".into());
        props.insert("resource-provenance".into(), quote(&format!("svg#{target}")));
        if let Some(parent) = parent.clone() {
            props.insert("resource-parent".into(), quote(&parent));
        }
        references.push(UseInstance { id, target: safe_ident(&target), properties: props });
        return;
    }

    let mut properties = geometry_properties(node, viewport_width, viewport_height);
    properties.insert("resource-only".into(), "true".into());
    properties.insert("source-tag".into(), quote(tag));
    properties.insert("resource-source-id".into(), quote(node.attribute("id").unwrap_or(&id)));
    apply_paint_and_style(&effective, &mut properties);
    mark_instance_inheritance(&local, inherited, &mut properties);
    preserve_resource_attributes(node, &mut properties);

    if tag == "symbol" {
        properties.insert("resource-kind".into(), quote("symbol"));
        if let Some(view_box) = node.attribute("viewBox") {
            properties.insert("viewbox".into(), quote(view_box));
        }
        properties.insert(
            "preserve-aspect-ratio".into(),
            quote(node.attribute("preserveAspectRatio").unwrap_or("xMidYMid meet")),
        );
    } else if tag == "g" {
        properties.insert("resource-kind".into(), quote("group"));
    }
    if let Some(transform) = node.attribute("transform") {
        properties.insert("transform".into(), quote(transform));
    }

    nodes.push(ResourceNode {
        kind: resource_kind(tag).to_string(),
        id: id.clone(),
        properties,
        parent: parent.clone(),
    });

    for child in node.children().filter(|child| child.is_element()) {
        emit_resource_node(
            child,
            Some(id.clone()),
            &effective,
            viewport_width,
            viewport_height,
            emitted,
            nodes,
            references,
            diagnostics,
        );
    }
}

fn resource_kind(tag: &str) -> &str {
    match tag {
        "g" | "symbol" | "svg" => "group",
        "line" | "polyline" | "polygon" => "path",
        other => other,
    }
}

fn reference_properties(
    node: Node<'_, '_>,
    style: &BTreeMap<String, String>,
    viewport_width: f64,
    viewport_height: f64,
) -> BTreeMap<String, String> {
    let mut props = BTreeMap::new();
    let x = length(node.attribute("x"), viewport_width, 0.0);
    let y = length(node.attribute("y"), viewport_height, 0.0);
    props.insert("position".into(), format!("{}px {}px", fmt(x), fmt(y)));
    if node.attribute("width").is_some() || node.attribute("height").is_some() {
        let w = length(node.attribute("width"), viewport_width, 0.0).max(0.0);
        let h = length(node.attribute("height"), viewport_height, 0.0).max(0.0);
        props.insert("size".into(), format!("{}px {}px", fmt(w), fmt(h)));
    }
    for key in INHERITED_STYLE_KEYS {
        if let Some(value) = style.get(*key) {
            props.insert((*key).into(), normalize_style_value(key, value));
        }
    }
    if let Some(transform) = node.attribute("transform") {
        props.insert("transform".into(), quote(transform));
    }
    props
}

fn geometry_properties(node: Node<'_, '_>, viewport_width: f64, viewport_height: f64) -> BTreeMap<String, String> {
    let mut props = BTreeMap::new();
    match node.tag_name().name() {
        "g" | "symbol" | "svg" => {
            props.insert("position".into(), "0px 0px".into());
            props.insert("size".into(), "0px 0px".into());
            props.insert("fill".into(), "none".into());
            props.insert("stroke".into(), "none".into());
        }
        "rect" => {
            let x = length(node.attribute("x"), viewport_width, 0.0);
            let y = length(node.attribute("y"), viewport_height, 0.0);
            let w = length(node.attribute("width"), viewport_width, 0.0).max(0.0);
            let h = length(node.attribute("height"), viewport_height, 0.0).max(0.0);
            props.insert("position".into(), pair(x, y));
            props.insert("size".into(), pair(w, h));
        }
        "circle" => {
            let cx = length(node.attribute("cx"), viewport_width, 0.0);
            let cy = length(node.attribute("cy"), viewport_height, 0.0);
            let r = length(node.attribute("r"), viewport_width, 0.0).max(0.0);
            props.insert("position".into(), pair(cx - r, cy - r));
            props.insert("size".into(), pair(2.0 * r, 2.0 * r));
        }
        "ellipse" => {
            let cx = length(node.attribute("cx"), viewport_width, 0.0);
            let cy = length(node.attribute("cy"), viewport_height, 0.0);
            let rx = length(node.attribute("rx"), viewport_width, 0.0).max(0.0);
            let ry = length(node.attribute("ry"), viewport_height, 0.0).max(0.0);
            props.insert("position".into(), pair(cx - rx, cy - ry));
            props.insert("size".into(), pair(2.0 * rx, 2.0 * ry));
        }
        "path" => {
            props.insert("position".into(), "0px 0px".into());
            if let Some(data) = node.attribute("d") {
                props.insert("data".into(), quote(data));
            }
        }
        "line" => {
            let x1 = length(node.attribute("x1"), viewport_width, 0.0);
            let y1 = length(node.attribute("y1"), viewport_height, 0.0);
            let x2 = length(node.attribute("x2"), viewport_width, 0.0);
            let y2 = length(node.attribute("y2"), viewport_height, 0.0);
            props.insert("position".into(), "0px 0px".into());
            props.insert("data".into(), quote(&format!("M {} {} L {} {}", fmt(x1), fmt(y1), fmt(x2), fmt(y2))));
        }
        "polyline" | "polygon" => {
            props.insert("position".into(), "0px 0px".into());
            if let Some(points) = node.attribute("points").and_then(points_path) {
                let data = if node.tag_name().name() == "polygon" { format!("{points} Z") } else { points };
                props.insert("data".into(), quote(&data));
            }
        }
        "text" | "tspan" => {
            let x = length(node.attribute("x"), viewport_width, 0.0);
            let y = length(node.attribute("y"), viewport_height, 0.0);
            props.insert("position".into(), pair(x, y));
            props.insert("content".into(), quote(node.text().unwrap_or("")));
        }
        _ => {
            props.insert("position".into(), "0px 0px".into());
            props.insert("size".into(), "0px 0px".into());
            props.insert("fill".into(), "none".into());
        }
    }
    props
}

fn apply_paint_and_style(style: &BTreeMap<String, String>, props: &mut BTreeMap<String, String>) {
    for key in INHERITED_STYLE_KEYS {
        if let Some(value) = style.get(*key) {
            props.insert((*key).into(), normalize_style_value(key, value));
        }
    }
}

fn mark_instance_inheritance(
    local: &BTreeMap<String, String>,
    inherited: &BTreeMap<String, String>,
    props: &mut BTreeMap<String, String>,
) {
    for key in ["fill", "stroke"] {
        if !local.contains_key(key) && !inherited.contains_key(key) {
            props.insert(format!("resource-inherit-{key}"), "true".into());
        }
    }
}

fn preserve_resource_attributes(node: Node<'_, '_>, props: &mut BTreeMap<String, String>) {
    for attr in node.attributes() {
        props.insert(format!("resource-attr-{}", safe_suffix(attr.name())), quote(attr.value()));
    }
}

fn write_node(output: &mut String, node: &ResourceNode) {
    output.push_str(&format!("{} {} {{\n", node.kind, node.id));
    for (key, value) in &node.properties {
        output.push_str(&format!("    {key}: {value};\n"));
    }
    output.push_str("}\n\n");
}

fn write_reference(output: &mut String, reference: &UseInstance) {
    output.push_str(&format!("reference {} = \"#{}\" {{\n", reference.id, reference.target));
    for (key, value) in &reference.properties {
        if key != "resource-parent" {
            output.push_str(&format!("    {key}: {value};\n"));
        }
    }
    output.push_str("}\n\n");
}

fn declared_ids(source: &str) -> HashSet<String> {
    source
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if !trimmed.ends_with('{') || trimmed.starts_with("relation ") {
                return None;
            }
            let fields = trimmed.split_whitespace().collect::<Vec<_>>();
            fields.get(1).map(|v| (*v).to_string())
        })
        .collect()
}

fn collect_style(node: Node<'_, '_>) -> BTreeMap<String, String> {
    let mut result = BTreeMap::new();
    for key in INHERITED_STYLE_KEYS {
        if let Some(value) = node.attribute(*key) {
            result.insert((*key).into(), value.trim().into());
        }
    }
    if let Some(style) = node.attribute("style") {
        for entry in style.split(';') {
            let Some((key, value)) = entry.split_once(':') else { continue; };
            let key = key.trim();
            if INHERITED_STYLE_KEYS.contains(&key) {
                result.insert(key.into(), value.trim().into());
            }
        }
    }
    result
}

fn normalize_style_value(key: &str, value: &str) -> String {
    if matches!(key, "fill" | "stroke") {
        normalize_color(value).unwrap_or_else(|| quote(value))
    } else if key == "stroke-width" {
        if let Some(number) = parse_number(value) {
            format!("{}px", fmt(number))
        } else {
            quote(value)
        }
    } else {
        value.to_string()
    }
}

fn local_href(node: Node<'_, '_>) -> Option<String> {
    let href = node
        .attribute("href")
        .or_else(|| node.attribute(("http://www.w3.org/1999/xlink", "href")))?;
    href.strip_prefix('#').map(str::to_string)
}

fn root_viewport(root: Node<'_, '_>) -> (f64, f64) {
    let view_box = root.attribute("viewBox").and_then(parse_viewbox);
    let width = length(root.attribute("width"), view_box.map(|v| v.2).unwrap_or(300.0), view_box.map(|v| v.2).unwrap_or(300.0));
    let height = length(root.attribute("height"), view_box.map(|v| v.3).unwrap_or(150.0), view_box.map(|v| v.3).unwrap_or(150.0));
    (width, height)
}

fn parse_viewbox(value: &str) -> Option<(f64, f64, f64, f64)> {
    let values = value
        .split(|ch: char| ch == ',' || ch.is_whitespace())
        .filter(|value| !value.is_empty())
        .map(str::parse::<f64>)
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    (values.len() == 4).then(|| (values[0], values[1], values[2], values[3]))
}

fn length(value: Option<&str>, percent_base: f64, default: f64) -> f64 {
    let Some(value) = value else { return default; };
    let value = value.trim();
    if let Some(percent) = value.strip_suffix('%').and_then(parse_number) {
        return percent_base * percent / 100.0;
    }
    for (unit, factor) in [("px", 1.0), ("pt", 96.0 / 72.0), ("mm", 96.0 / 25.4), ("cm", 96.0 / 2.54), ("in", 96.0)] {
        if let Some(number) = value.strip_suffix(unit).and_then(parse_number) {
            return number * factor;
        }
    }
    parse_number(value).unwrap_or(default)
}

fn points_path(value: &str) -> Option<String> {
    let values = value
        .split(|ch: char| ch == ',' || ch.is_whitespace())
        .filter(|value| !value.is_empty())
        .map(str::parse::<f64>)
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    if values.len() < 2 || values.len() % 2 != 0 {
        return None;
    }
    let mut path = format!("M {} {}", fmt(values[0]), fmt(values[1]));
    for pair in values[2..].chunks_exact(2) {
        path.push_str(&format!(" L {} {}", fmt(pair[0]), fmt(pair[1])));
    }
    Some(path)
}

fn generated_child_id(parent: &str, index: usize) -> String {
    safe_ident(&format!("{parent}__resource_{index}"))
}

fn safe_ident(raw: &str) -> String {
    let mut output = String::new();
    for (index, ch) in raw.chars().enumerate() {
        let valid = if index == 0 {
            ch.is_ascii_alphabetic() || ch == '_' || ch == '%'
        } else {
            ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | '/' | '%')
        };
        output.push(if valid { ch } else { '_' });
    }
    if output.is_empty() { "resource".into() } else { output }
}

fn safe_suffix(raw: &str) -> String {
    raw.chars()
        .map(|ch| if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.') { ch } else { '-' })
        .collect()
}

fn normalize_color(value: &str) -> Option<String> {
    let value = value.trim();
    if value.eq_ignore_ascii_case("none") {
        return Some("none".into());
    }
    if value.starts_with('#') && matches!(value.len(), 4 | 5 | 7 | 9) {
        return Some(value.into());
    }
    match value.to_ascii_lowercase().as_str() {
        "black" => Some("#000000".into()),
        "white" => Some("#ffffff".into()),
        "red" => Some("#ff0000".into()),
        "green" => Some("#008000".into()),
        "blue" => Some("#0000ff".into()),
        "transparent" => Some("#00000000".into()),
        _ => None,
    }
}

fn pair(x: f64, y: f64) -> String {
    format!("{}px {}px", fmt(x), fmt(y))
}

fn fmt(value: f64) -> String {
    if value.fract().abs() < 1e-9 {
        format!("{}", value as i64)
    } else {
        let mut value = format!("{value:.6}");
        while value.ends_with('0') { value.pop(); }
        if value.ends_with('.') { value.pop(); }
        value
    }
}

fn parse_number(value: &str) -> Option<f64> {
    value.trim().parse().ok()
}

fn quote(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n"))
}

fn unquote(value: &str) -> String {
    value.trim().strip_prefix('"').and_then(|v| v.strip_suffix('"')).unwrap_or(value).to_string()
}

fn diag(severity: &str, code: &str, message: &str, element: Option<&str>) -> ImportDiagnostic {
    ImportDiagnostic {
        severity: severity.into(),
        code: code.into(),
        message: message.into(),
        element: element.map(str::to_string),
    }
}
