use roxmltree::{Document as XmlDocument, Node};
use std::collections::{BTreeMap, HashMap};

#[derive(Debug, Clone)]
pub struct ImportOptions {
    pub file_id: Option<String>,
    pub preserve_source_attributes: bool,
}

impl Default for ImportOptions {
    fn default() -> Self {
        Self {
            file_id: None,
            preserve_source_attributes: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportDiagnostic {
    pub severity: String,
    pub code: String,
    pub message: String,
    pub element: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ImportResult {
    pub source: String,
    pub diagnostics: Vec<ImportDiagnostic>,
}

impl ImportResult {
    pub fn has_errors(&self) -> bool {
        self.diagnostics.iter().any(|d| d.severity == "error")
    }
}

pub fn import_svg(svg: &str, source_name: &str, options: &ImportOptions) -> ImportResult {
    let xml = match XmlDocument::parse(svg) {
        Ok(xml) => xml,
        Err(error) => {
            return ImportResult {
                source: "srng 0.1;\n".into(),
                diagnostics: vec![diag("error", "S001", format!("invalid SVG/XML: {error}"), None)],
            }
        }
    };

    let root = xml.root_element();
    if root.tag_name().name() != "svg" {
        return ImportResult {
            source: "srng 0.1;\n".into(),
            diagnostics: vec![diag(
                "error",
                "S002",
                format!("expected <svg> root, found <{}>", root.tag_name().name()),
                None,
            )],
        };
    }

    let view_box = parse_view_box(root.attribute("viewBox"));
    let width = root
        .attribute("width")
        .and_then(|v| resolve_length(v, Axis::X, 300.0, 150.0))
        .or_else(|| view_box.map(|(_, _, w, _)| w))
        .unwrap_or(300.0);
    let height = root
        .attribute("height")
        .and_then(|v| resolve_length(v, Axis::Y, width, 150.0))
        .or_else(|| view_box.map(|(_, _, _, h)| h))
        .unwrap_or(150.0);

    let mut importer = Importer {
        svg,
        options,
        width,
        height,
        declarations: Vec::new(),
        relations: Vec::new(),
        diagnostics: Vec::new(),
        ids: HashMap::new(),
        generated: 0,
        clip_defs: HashMap::new(),
        mask_clip_defs: HashMap::new(),
        patterns: HashMap::new(),
    };

    let root_id = importer.unique_id(root.attribute("id").unwrap_or("svg_root"));
    let mut root_props = BTreeMap::new();
    root_props.insert("position".into(), "0px 0px".into());
    root_props.insert("size".into(), format!("{}px {}px", fmt(width), fmt(height)));
    root_props.insert("fill".into(), "none".into());
    root_props.insert("svg-source-tag".into(), quoted("svg"));
    root_props.insert("svg-source".into(), quoted(source_name));
    importer.preserve_attributes(root, &mut root_props);
    importer.declarations.push(NodeDecl {
        kind: "canvas".into(),
        id: root_id.clone(),
        props: root_props,
    });

    if let Some((x, y, w, h)) = view_box {
        if x != 0.0 || y != 0.0 || (w - width).abs() > f64::EPSILON || (h - height).abs() > f64::EPSILON {
            importer.warn("S120", "viewBox mapping is preserved as metadata but is not yet baked into geometry", Some(&root_id));
        }
    }

    importer.collect_definitions(root);

    let mut inherited = BTreeMap::new();
    inherited.insert("fill".into(), "#000000".into());
    inherited.insert("stroke".into(), "none".into());

    for child in root.children().filter(|n| n.is_element()) {
        importer.visit(child, &root_id, &inherited);
    }

    let mut out = String::from("srng 0.1;\n");
    let file_id = options.file_id.clone().unwrap_or_else(|| source_name.to_string());
    out.push_str(&format!("file {};\n\n", quoted(&file_id)));
    for node in &importer.declarations {
        node.write_to(&mut out);
    }
    for (from, to) in &importer.relations {
        out.push_str(&format!("relation {from} -> {to} {{\n    kind: contains;\n}}\n\n"));
    }

    ImportResult {
        source: out,
        diagnostics: importer.diagnostics,
    }
}

#[derive(Debug, Clone)]
struct PatternDef {
    width: f64,
    height: f64,
    xml: String,
}

struct Importer<'a> {
    svg: &'a str,
    options: &'a ImportOptions,
    width: f64,
    height: f64,
    declarations: Vec<NodeDecl>,
    relations: Vec<(String, String)>,
    diagnostics: Vec<ImportDiagnostic>,
    ids: HashMap<String, usize>,
    generated: usize,
    clip_defs: HashMap<String, String>,
    mask_clip_defs: HashMap<String, String>,
    patterns: HashMap<String, PatternDef>,
}

impl Importer<'_> {
    fn collect_definitions(&mut self, root: Node<'_, '_>) {
        for node in root.descendants().filter(|n| n.is_element()) {
            match node.tag_name().name() {
                "clipPath" => self.collect_clip_path(node),
                "mask" => self.collect_mask(node),
                "pattern" => self.collect_pattern(node),
                _ => {}
            }
        }
    }

    fn collect_clip_path(&mut self, node: Node<'_, '_>) {
        let Some(raw_id) = node.attribute("id") else { return; };
        let Some(path) = definition_path(node, self.width, self.height) else {
            self.warn("S240", "clipPath contains geometry that cannot yet be converted to an SRNG clip", Some(raw_id));
            return;
        };
        let id = self.unique_id(&format!("svg_clip_{raw_id}"));
        let mut props = BTreeMap::new();
        props.insert("position".into(), "0px 0px".into());
        props.insert("data".into(), quoted(&path));
        props.insert("fill".into(), "none".into());
        props.insert("stroke".into(), "none".into());
        props.insert("svg-definition".into(), quoted("clipPath"));
        props.insert("svg-source-tag".into(), quoted("clipPath"));
        self.preserve_attributes(node, &mut props);
        self.declarations.push(NodeDecl { kind: "path".into(), id: id.clone(), props });
        self.clip_defs.insert(raw_id.to_string(), id);
    }

    fn collect_mask(&mut self, node: Node<'_, '_>) {
        let Some(raw_id) = node.attribute("id") else { return; };
        if !mask_is_binary_opaque(node) {
            return;
        }
        let Some(path) = definition_path(node, self.width, self.height) else { return; };
        let id = self.unique_id(&format!("svg_maskclip_{raw_id}"));
        let mut props = BTreeMap::new();
        props.insert("position".into(), "0px 0px".into());
        props.insert("data".into(), quoted(&path));
        props.insert("fill".into(), "none".into());
        props.insert("stroke".into(), "none".into());
        props.insert("svg-definition".into(), quoted("binary-mask-as-clip"));
        props.insert("svg-source-tag".into(), quoted("mask"));
        self.preserve_attributes(node, &mut props);
        self.declarations.push(NodeDecl { kind: "path".into(), id: id.clone(), props });
        self.mask_clip_defs.insert(raw_id.to_string(), id);
    }

    fn collect_pattern(&mut self, node: Node<'_, '_>) {
        let Some(raw_id) = node.attribute("id") else { return; };
        let width = node
            .attribute("width")
            .and_then(|v| resolve_length(v, Axis::X, self.width, self.height));
        let height = node
            .attribute("height")
            .and_then(|v| resolve_length(v, Axis::Y, self.width, self.height));
        if let (Some(width), Some(height)) = (width, height) {
            self.patterns.insert(
                raw_id.to_string(),
                PatternDef { width, height, xml: self.node_xml(node) },
            );
        }
    }

    fn visit(&mut self, node: Node<'_, '_>, parent: &str, inherited: &BTreeMap<String, String>) {
        let tag = node.tag_name().name();
        let local = collect_style(node);
        let mut effective = inherited.clone();
        effective.extend(local.clone());

        if local.get("display").is_some_and(|v| v == "none")
            || matches!(tag, "defs" | "metadata" | "title" | "desc" | "clipPath" | "mask" | "pattern")
        {
            let id = self.node_id(node, tag);
            let mut props = placeholder(tag, self.node_xml(node));
            self.preserve_attributes(node, &mut props);
            self.declarations.push(NodeDecl { kind: "group".into(), id: id.clone(), props });
            self.relations.push((parent.into(), id.clone()));
            self.info("S140", "non-rendering SVG subtree preserved as metadata", Some(&id));
            return;
        }

        let id = self.node_id(node, tag);
        let mut props = BTreeMap::new();
        props.insert("svg-source-tag".into(), quoted(tag));
        self.preserve_attributes(node, &mut props);

        let kind = match tag {
            "g" | "symbol" => {
                props.insert("position".into(), "0px 0px".into());
                props.insert("size".into(), "0px 0px".into());
                props.insert("fill".into(), "none".into());
                props.insert("stroke".into(), "none".into());
                "group"
            }
            "rect" => {
                let x = self.len(node, "x", Axis::X, 0.0, &id);
                let y = self.len(node, "y", Axis::Y, 0.0, &id);
                let w = self.len(node, "width", Axis::X, 0.0, &id).max(0.0);
                let h = self.len(node, "height", Axis::Y, 0.0, &id).max(0.0);
                props.insert("position".into(), pair(x, y));
                props.insert("size".into(), pair(w, h));
                if node.attribute("rx").is_some() || node.attribute("ry").is_some() {
                    let (rx, ry) = rounded_radii(node, w, h, self.width, self.height);
                    props.insert("data".into(), quoted(&rounded_rect_path(x, y, w, h, rx, ry)));
                }
                self.paint(&effective, &mut props, &id);
                "rect"
            }
            "circle" => {
                let cx = self.len(node, "cx", Axis::X, 0.0, &id);
                let cy = self.len(node, "cy", Axis::Y, 0.0, &id);
                let r = self.len(node, "r", Axis::X, 0.0, &id).max(0.0);
                props.insert("position".into(), pair(cx - r, cy - r));
                props.insert("size".into(), pair(r * 2.0, r * 2.0));
                self.paint(&effective, &mut props, &id);
                "circle"
            }
            "ellipse" => {
                let cx = self.len(node, "cx", Axis::X, 0.0, &id);
                let cy = self.len(node, "cy", Axis::Y, 0.0, &id);
                let rx = self.len(node, "rx", Axis::X, 0.0, &id).max(0.0);
                let ry = self.len(node, "ry", Axis::Y, 0.0, &id).max(0.0);
                props.insert("position".into(), pair(cx - rx, cy - ry));
                props.insert("size".into(), pair(rx * 2.0, ry * 2.0));
                self.paint(&effective, &mut props, &id);
                "ellipse"
            }
            "path" => {
                props.insert("position".into(), "0px 0px".into());
                if let Some(d) = node.attribute("d") {
                    props.insert("data".into(), quoted(d));
                } else {
                    self.warn("S220", "path has no `d` attribute", Some(&id));
                }
                self.paint(&effective, &mut props, &id);
                "path"
            }
            "line" => {
                let x1 = self.len(node, "x1", Axis::X, 0.0, &id);
                let y1 = self.len(node, "y1", Axis::Y, 0.0, &id);
                let x2 = self.len(node, "x2", Axis::X, 0.0, &id);
                let y2 = self.len(node, "y2", Axis::Y, 0.0, &id);
                props.insert("position".into(), "0px 0px".into());
                props.insert("data".into(), quoted(&format!("M {} {} L {} {}", fmt(x1), fmt(y1), fmt(x2), fmt(y2))));
                self.paint(&effective, &mut props, &id);
                "path"
            }
            "polyline" | "polygon" => {
                props.insert("position".into(), "0px 0px".into());
                if let Some(mut d) = node.attribute("points").and_then(points_to_path) {
                    if tag == "polygon" { d.push_str(" Z"); }
                    props.insert("data".into(), quoted(&d));
                } else {
                    self.warn("S221", "invalid or missing points", Some(&id));
                }
                self.paint(&effective, &mut props, &id);
                "path"
            }
            "text" | "tspan" => {
                let x = self.len(node, "x", Axis::X, 0.0, &id);
                let y = self.len(node, "y", Axis::Y, 0.0, &id);
                props.insert("position".into(), pair(x, y));
                props.insert("content".into(), quoted(node.text().unwrap_or("")));
                self.paint(&effective, &mut props, &id);
                self.warn("S301", "text is preserved but requires pre-shaped outline path data for rendering", Some(&id));
                "text"
            }
            "image" | "use" => {
                props.insert("position".into(), "0px 0px".into());
                props.insert("size".into(), "0px 0px".into());
                props.insert("fill".into(), "none".into());
                self.warn("S302", &format!("<{tag}> is preserved as metadata but not rendered in v0.1"), Some(&id));
                "group"
            }
            _ => {
                props.insert("position".into(), "0px 0px".into());
                props.insert("size".into(), "0px 0px".into());
                props.insert("fill".into(), "none".into());
                props.insert("svg-source-xml".into(), quoted(&self.node_xml(node)));
                self.warn("S399", &format!("unsupported SVG element <{tag}> preserved as metadata"), Some(&id));
                "group"
            }
        };

        if node.attribute("transform").is_some() {
            self.warn("S130", "transform is preserved as metadata but is not yet baked into geometry", Some(&id));
        }

        self.apply_clip_and_mask(&effective, node, &mut props, &id);
        for key in ["opacity", "fill-opacity", "stroke-opacity", "filter"] {
            if local.contains_key(key) || node.attribute(key).is_some() {
                self.warn("S131", &format!("`{key}` is preserved but not yet represented by the renderer"), Some(&id));
            }
        }

        self.declarations.push(NodeDecl { kind: kind.into(), id: id.clone(), props });
        self.relations.push((parent.into(), id.clone()));
        for child in node.children().filter(|n| n.is_element()) {
            self.visit(child, &id, &effective);
        }
    }

    fn apply_clip_and_mask(
        &mut self,
        effective: &BTreeMap<String, String>,
        node: Node<'_, '_>,
        props: &mut BTreeMap<String, String>,
        id: &str,
    ) {
        if let Some(raw) = effective.get("clip-path").or_else(|| node.attribute("clip-path").map(|v| {
            // This branch only exists to satisfy the shared handling below; presentation attributes
            // are already collected into `effective` in normal SVG input.
            unreachable_value(v)
        })) {
            if let Some(def_id) = url_fragment(raw) {
                if let Some(clip) = self.clip_defs.get(def_id) {
                    props.insert("clip".into(), clip.clone());
                } else {
                    self.warn("S241", &format!("clip-path `{raw}` could not be resolved to renderable clip geometry"), Some(id));
                }
            }
        }

        if let Some(raw) = effective.get("mask") {
            if let Some(def_id) = url_fragment(raw) {
                if let Some(clip) = self.mask_clip_defs.get(def_id) {
                    props.insert("clip".into(), clip.clone());
                    props.insert("svg-mask-mode".into(), quoted("binary-opaque-clip"));
                } else {
                    self.warn("S242", &format!("mask `{raw}` requires alpha/luminance compositing that is not yet supported"), Some(id));
                }
            }
        }
    }

    fn paint(&mut self, style: &BTreeMap<String, String>, props: &mut BTreeMap<String, String>, id: &str) {
        for key in ["fill", "stroke"] {
            if let Some(value) = style.get(key) {
                if let Some(value) = normalize_paint(value) {
                    props.insert(key.into(), value);
                } else if let Some(fragment) = url_fragment(value) {
                    if key == "fill" {
                        if let Some(pattern) = self.patterns.get(fragment) {
                            props.insert("svg-pattern-ref".into(), quoted(fragment));
                            props.insert("svg-pattern-width".into(), format!("{}px", fmt(pattern.width)));
                            props.insert("svg-pattern-height".into(), format!("{}px", fmt(pattern.height)));
                            props.insert("svg-pattern-source-xml".into(), quoted(&pattern.xml));
                            props.insert(format!("svg-{key}"), quoted(value));
                            self.warn("S232", &format!("vector/raster pattern fill `{value}` is preserved with tile metadata; pattern painting is the next renderer stage"), Some(id));
                        } else {
                            props.insert(format!("svg-{key}"), quoted(value));
                            self.warn("S230", &format!("unsupported {key} paint `{value}` preserved but not rendered"), Some(id));
                        }
                    } else {
                        props.insert(format!("svg-{key}"), quoted(value));
                        self.warn("S230", &format!("unsupported {key} paint `{value}` preserved but not rendered"), Some(id));
                    }
                } else {
                    props.insert(format!("svg-{key}"), quoted(value));
                    self.warn("S230", &format!("unsupported {key} paint `{value}` preserved but not rendered"), Some(id));
                }
            }
        }
        for key in ["fill-rule", "stroke-linecap", "stroke-linejoin", "stroke-dasharray", "stroke-dashoffset"] {
            if let Some(value) = style.get(key) { props.insert(key.into(), value.clone()); }
        }
        if let Some(value) = style.get("stroke-width") {
            if let Some(px) = resolve_length(value, Axis::X, self.width, self.height) {
                props.insert("stroke-width".into(), format!("{}px", fmt(px)));
            } else {
                props.insert("svg-stroke-width".into(), quoted(value));
                self.warn("S231", "unsupported stroke-width unit preserved but not rendered", Some(id));
            }
        }
    }

    fn len(&mut self, node: Node<'_, '_>, name: &str, axis: Axis, default: f64, id: &str) -> f64 {
        let Some(raw) = node.attribute(name) else { return default; };
        resolve_length(raw, axis, self.width, self.height).unwrap_or_else(|| {
            self.warn("S201", &format!("unsupported length `{raw}` for `{name}`; using {default}px"), Some(id));
            default
        })
    }

    fn preserve_attributes(&self, node: Node<'_, '_>, props: &mut BTreeMap<String, String>) {
        if !self.options.preserve_source_attributes { return; }
        for attr in node.attributes() {
            props.insert(format!("svg-attr-{}", safe_suffix(attr.name())), quoted(attr.value()));
        }
    }

    fn node_id(&mut self, node: Node<'_, '_>, tag: &str) -> String {
        if let Some(id) = node.attribute("id") { self.unique_id(id) } else {
            self.generated += 1;
            self.unique_id(&format!("{tag}_{}", self.generated))
        }
    }

    fn unique_id(&mut self, raw: &str) -> String {
        let base = safe_ident(raw);
        let count = self.ids.entry(base.clone()).or_insert(0);
        *count += 1;
        if *count == 1 { base } else { format!("{base}_{}", *count) }
    }

    fn node_xml(&self, node: Node<'_, '_>) -> String {
        self.svg.get(node.range()).unwrap_or("").to_string()
    }

    fn warn(&mut self, code: &str, message: &str, element: Option<&str>) {
        self.diagnostics.push(diag("warning", code, message.to_string(), element));
    }
    fn info(&mut self, code: &str, message: &str, element: Option<&str>) {
        self.diagnostics.push(diag("info", code, message.to_string(), element));
    }
}

// `effective` always contains presentation attributes, so this function should never be called.
// Keeping the helper out of the main path avoids duplicating clip-path parsing logic.
fn unreachable_value(_value: &str) -> &String {
    unreachable!("presentation attribute should have been collected into effective style")
}

struct NodeDecl {
    kind: String,
    id: String,
    props: BTreeMap<String, String>,
}

impl NodeDecl {
    fn write_to(&self, out: &mut String) {
        out.push_str(&format!("{} {} {{\n", self.kind, self.id));
        for (key, value) in &self.props {
            out.push_str(&format!("    {key}: {value};\n"));
        }
        out.push_str("}\n\n");
    }
}

#[derive(Clone, Copy)]
enum Axis { X, Y }

fn collect_style(node: Node<'_, '_>) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for key in [
        "fill", "stroke", "fill-rule", "stroke-width", "stroke-linecap", "stroke-linejoin",
        "stroke-dasharray", "stroke-dashoffset", "opacity", "fill-opacity", "stroke-opacity",
        "filter", "mask", "clip-path", "display", "visibility",
    ] {
        if let Some(value) = node.attribute(key) { out.insert(key.into(), value.trim().into()); }
    }
    if let Some(style) = node.attribute("style") {
        for part in style.split(';') {
            if let Some((key, value)) = part.split_once(':') {
                if !key.trim().is_empty() { out.insert(key.trim().into(), value.trim().into()); }
            }
        }
    }
    out
}

fn normalize_paint(value: &str) -> Option<String> {
    let value = value.trim();
    if value.eq_ignore_ascii_case("none") { return Some("none".into()); }
    if value.starts_with('#') && matches!(value.len(), 4 | 5 | 7 | 9) { return Some(value.into()); }
    let named = match value.to_ascii_lowercase().as_str() {
        "black" => Some("#000000"), "white" => Some("#ffffff"), "red" => Some("#ff0000"),
        "green" => Some("#008000"), "blue" => Some("#0000ff"), "yellow" => Some("#ffff00"),
        "gray" | "grey" => Some("#808080"), "transparent" => Some("#00000000"), _ => None,
    };
    if let Some(value) = named { return Some(value.into()); }
    parse_rgb(value)
}

fn parse_rgb(value: &str) -> Option<String> {
    let inner = value.strip_prefix("rgb(")?.strip_suffix(')')?;
    let vals = inner.split(',').map(|v| v.trim().parse::<u8>().ok()).collect::<Option<Vec<_>>>()?;
    if vals.len() != 3 { return None; }
    Some(format!("#{:02x}{:02x}{:02x}", vals[0], vals[1], vals[2]))
}

fn resolve_length(value: &str, axis: Axis, vw: f64, vh: f64) -> Option<f64> {
    let value = value.trim();
    if let Ok(v) = value.parse::<f64>() { return Some(v); }
    for unit in ["px", "pt", "mm", "cm", "in", "%"] {
        if let Some(number) = value.strip_suffix(unit).and_then(|v| v.trim().parse::<f64>().ok()) {
            return Some(match unit {
                "px" => number,
                "pt" => number * 96.0 / 72.0,
                "mm" => number * 96.0 / 25.4,
                "cm" => number * 96.0 / 2.54,
                "in" => number * 96.0,
                "%" => number * if matches!(axis, Axis::X) { vw } else { vh } / 100.0,
                _ => unreachable!(),
            });
        }
    }
    None
}

fn parse_view_box(value: Option<&str>) -> Option<(f64, f64, f64, f64)> {
    let values = value?.split(|c: char| c == ',' || c.is_ascii_whitespace())
        .filter(|s| !s.is_empty()).map(|s| s.parse::<f64>().ok()).collect::<Option<Vec<_>>>()?;
    (values.len() == 4).then(|| (values[0], values[1], values[2], values[3]))
}

fn points_to_path(value: &str) -> Option<String> {
    let values = value.split(|c: char| c == ',' || c.is_ascii_whitespace())
        .filter(|s| !s.is_empty()).map(|s| s.parse::<f64>().ok()).collect::<Option<Vec<_>>>()?;
    if values.len() < 2 || values.len() % 2 != 0 { return None; }
    let mut out = format!("M {} {}", fmt(values[0]), fmt(values[1]));
    for p in values[2..].chunks_exact(2) { out.push_str(&format!(" L {} {}", fmt(p[0]), fmt(p[1]))); }
    Some(out)
}

fn definition_path(node: Node<'_, '_>, vw: f64, vh: f64) -> Option<String> {
    let mut paths = Vec::new();
    for child in node.children().filter(|n| n.is_element()) {
        if child.attribute("transform").is_some() { return None; }
        paths.push(shape_path(child, vw, vh)?);
    }
    if paths.is_empty() { None } else { Some(paths.join(" ")) }
}

fn shape_path(node: Node<'_, '_>, vw: f64, vh: f64) -> Option<String> {
    let len = |name: &str, axis: Axis, default: f64| {
        node.attribute(name)
            .and_then(|v| resolve_length(v, axis, vw, vh))
            .unwrap_or(default)
    };
    match node.tag_name().name() {
        "path" => node.attribute("d").map(str::to_owned),
        "rect" => {
            let x = len("x", Axis::X, 0.0);
            let y = len("y", Axis::Y, 0.0);
            let w = len("width", Axis::X, 0.0).max(0.0);
            let h = len("height", Axis::Y, 0.0).max(0.0);
            let (rx, ry) = rounded_radii(node, w, h, vw, vh);
            Some(rounded_rect_path(x, y, w, h, rx, ry))
        }
        "circle" => {
            let cx = len("cx", Axis::X, 0.0);
            let cy = len("cy", Axis::Y, 0.0);
            let r = len("r", Axis::X, 0.0).max(0.0);
            Some(ellipse_path(cx, cy, r, r))
        }
        "ellipse" => {
            let cx = len("cx", Axis::X, 0.0);
            let cy = len("cy", Axis::Y, 0.0);
            let rx = len("rx", Axis::X, 0.0).max(0.0);
            let ry = len("ry", Axis::Y, 0.0).max(0.0);
            Some(ellipse_path(cx, cy, rx, ry))
        }
        "polygon" => node.attribute("points").and_then(points_to_path).map(|mut d| { d.push_str(" Z"); d }),
        "polyline" => node.attribute("points").and_then(points_to_path),
        "line" => Some(format!(
            "M {} {} L {} {}",
            fmt(len("x1", Axis::X, 0.0)),
            fmt(len("y1", Axis::Y, 0.0)),
            fmt(len("x2", Axis::X, 0.0)),
            fmt(len("y2", Axis::Y, 0.0)),
        )),
        _ => None,
    }
}

fn rounded_radii(node: Node<'_, '_>, width: f64, height: f64, vw: f64, vh: f64) -> (f64, f64) {
    let raw_rx = node.attribute("rx").and_then(|v| resolve_length(v, Axis::X, vw, vh));
    let raw_ry = node.attribute("ry").and_then(|v| resolve_length(v, Axis::Y, vw, vh));
    let mut rx = raw_rx.or(raw_ry).unwrap_or(0.0).max(0.0);
    let mut ry = raw_ry.or(raw_rx).unwrap_or(0.0).max(0.0);
    rx = rx.min(width / 2.0);
    ry = ry.min(height / 2.0);
    (rx, ry)
}

fn rounded_rect_path(x: f64, y: f64, width: f64, height: f64, rx: f64, ry: f64) -> String {
    if rx <= 0.0 || ry <= 0.0 {
        return format!("M {} {} h {} v {} h {} Z", fmt(x), fmt(y), fmt(width), fmt(height), fmt(-width));
    }
    let right = x + width;
    let bottom = y + height;
    format!(
        "M {} {} H {} A {} {} 0 0 1 {} {} V {} A {} {} 0 0 1 {} {} H {} A {} {} 0 0 1 {} {} V {} A {} {} 0 0 1 {} {} Z",
        fmt(x + rx), fmt(y), fmt(right - rx), fmt(rx), fmt(ry), fmt(right), fmt(y + ry),
        fmt(bottom - ry), fmt(rx), fmt(ry), fmt(right - rx), fmt(bottom), fmt(x + rx),
        fmt(rx), fmt(ry), fmt(x), fmt(bottom - ry), fmt(y + ry), fmt(rx), fmt(ry), fmt(x + rx), fmt(y)
    )
}

fn ellipse_path(cx: f64, cy: f64, rx: f64, ry: f64) -> String {
    format!(
        "M {} {} A {} {} 0 1 0 {} {} A {} {} 0 1 0 {} {} Z",
        fmt(cx - rx), fmt(cy), fmt(rx), fmt(ry), fmt(cx + rx), fmt(cy),
        fmt(rx), fmt(ry), fmt(cx - rx), fmt(cy)
    )
}

fn mask_is_binary_opaque(node: Node<'_, '_>) -> bool {
    let children = node.children().filter(|n| n.is_element()).collect::<Vec<_>>();
    if children.is_empty() { return false; }
    children.into_iter().all(|child| {
        if child.attribute("transform").is_some() { return false; }
        let style = collect_style(child);
        let opacity_ok = ["opacity", "fill-opacity"]
            .iter()
            .all(|key| style.get(*key).map(|v| v == "1" || v == "1.0").unwrap_or(true));
        let fill = style.get("fill").map(String::as_str).or_else(|| child.attribute("fill")).unwrap_or("black");
        let fill_ok = matches!(fill.to_ascii_lowercase().as_str(), "white" | "#fff" | "#ffffff" | "#ffffffff");
        opacity_ok && fill_ok && shape_path(child, 1.0, 1.0).is_some()
    })
}

fn url_fragment(value: &str) -> Option<&str> {
    value.trim().strip_prefix("url(#")?.strip_suffix(')')
}

fn placeholder(tag: &str, xml: String) -> BTreeMap<String, String> {
    let mut props = BTreeMap::new();
    props.insert("position".into(), "0px 0px".into());
    props.insert("size".into(), "0px 0px".into());
    props.insert("fill".into(), "none".into());
    props.insert("stroke".into(), "none".into());
    props.insert("svg-source-tag".into(), quoted(tag));
    props.insert("svg-source-xml".into(), quoted(&xml));
    props
}

fn safe_ident(raw: &str) -> String {
    let mut out = String::new();
    for (i, ch) in raw.chars().enumerate() {
        let valid = if i == 0 { ch.is_ascii_alphabetic() || ch == '_' || ch == '%' }
            else { ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | '/' | '%') };
        out.push(if valid { ch } else { '_' });
    }
    if out.is_empty() { "svg_node".into() } else { out }
}

fn safe_suffix(raw: &str) -> String {
    raw.chars().map(|ch| if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.') { ch } else { '-' }).collect()
}

fn quoted(value: &str) -> String {
    let mut out = String::from("\"");
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"), '"' => out.push_str("\\\""), '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"), '\t' => out.push_str("\\t"), other => out.push(other),
        }
    }
    out.push('"');
    out
}

fn pair(x: f64, y: f64) -> String { format!("{}px {}px", fmt(x), fmt(y)) }

fn fmt(value: f64) -> String {
    if value.fract().abs() < 1e-9 { format!("{value:.0}") } else {
        let mut s = format!("{value:.6}");
        while s.ends_with('0') { s.pop(); }
        if s.ends_with('.') { s.pop(); }
        s
    }
}

fn diag(severity: &str, code: &str, message: String, element: Option<&str>) -> ImportDiagnostic {
    ImportDiagnostic { severity: severity.into(), code: code.into(), message, element: element.map(str::to_owned) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn imports_basic_geometry_and_compiles() {
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="80"><rect id="card" x="10" y="20" width="30" height="40" fill="#ff0000"/></svg>"##;
        let imported = import_svg(svg, "basic.svg", &ImportOptions::default());
        assert!(!imported.has_errors());
        assert!(imported.source.contains("rect card"));
        assert!(imported.source.contains("position: 10px 20px;"));
        let document = crate::compile(&imported.source);
        assert!(!document.diagnostics.iter().any(|d| matches!(d.severity, crate::diagnostic::Severity::Error)));
    }

    #[test]
    fn inherits_fill_and_converts_named_color() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg"><g fill="red"><circle id="dot" cx="10" cy="10" r="5"/></g></svg>"#;
        let imported = import_svg(svg, "style.svg", &ImportOptions::default());
        assert!(imported.source.contains("fill: #ff0000;"));
    }

    #[test]
    fn converts_polyline_to_path() {
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg"><polyline id="p" points="0,0 10,5 20,0" stroke="#000" fill="none"/></svg>"##;
        let imported = import_svg(svg, "poly.svg", &ImportOptions::default());
        assert!(imported.source.contains("data: \"M 0 0 L 10 5 L 20 0\";"));
    }

    #[test]
    fn imports_rounded_rect_as_path_geometry() {
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="80"><rect id="round" x="10" y="10" width="60" height="40" rx="8" fill="#fff"/></svg>"##;
        let imported = import_svg(svg, "round.svg", &ImportOptions::default());
        assert!(imported.source.contains("rect round"));
        assert!(imported.source.contains("A 8 8"));
        assert!(!imported.diagnostics.iter().any(|d| d.code == "S210"));
    }

    #[test]
    fn imports_clip_path_into_renderer_clip_reference() {
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100"><defs><clipPath id="cut"><rect x="10" y="10" width="50" height="50" rx="5"/></clipPath></defs><rect id="card" width="100" height="100" clip-path="url(#cut)" fill="#f00"/></svg>"##;
        let imported = import_svg(svg, "clip.svg", &ImportOptions::default());
        assert!(imported.source.contains("path svg_clip_cut"));
        assert!(imported.source.contains("clip: svg_clip_cut;"));
        assert!(!imported.diagnostics.iter().any(|d| d.code == "S131" && d.message.contains("clip-path")));
    }

    #[test]
    fn converts_simple_white_mask_to_clip() {
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100"><defs><mask id="m"><circle cx="50" cy="50" r="25" fill="white"/></mask></defs><rect id="card" width="100" height="100" mask="url(#m)" fill="#f00"/></svg>"##;
        let imported = import_svg(svg, "mask.svg", &ImportOptions::default());
        assert!(imported.source.contains("path svg_maskclip_m"));
        assert!(imported.source.contains("clip: svg_maskclip_m;"));
        assert!(!imported.diagnostics.iter().any(|d| d.code == "S242"));
    }

    #[test]
    fn preserves_complex_mask_with_specific_warning() {
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg"><defs><mask id="m"><rect width="10" height="10" fill="#888"/></mask></defs><rect id="x" width="10" height="10" mask="url(#m)"/></svg>"##;
        let imported = import_svg(svg, "mask-alpha.svg", &ImportOptions::default());
        assert!(imported.diagnostics.iter().any(|d| d.code == "S242"));
    }

    #[test]
    fn records_pattern_tile_metadata() {
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg"><defs><pattern id="p" width="8" height="8"><rect width="4" height="4" fill="#fff"/></pattern></defs><rect id="x" width="20" height="20" fill="url(#p)"/></svg>"##;
        let imported = import_svg(svg, "pattern.svg", &ImportOptions::default());
        assert!(imported.source.contains("svg-pattern-ref: \"p\";"));
        assert!(imported.diagnostics.iter().any(|d| d.code == "S232"));
    }

    #[test]
    fn preserves_unsupported_content_as_metadata() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg"><foreignObject id="x"><div xmlns="http://www.w3.org/1999/xhtml">Hi</div></foreignObject></svg>"#;
        let imported = import_svg(svg, "unsupported.svg", &ImportOptions::default());
        assert!(imported.source.contains("svg-source-xml"));
        assert!(imported.diagnostics.iter().any(|d| d.code == "S399"));
    }
}
