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
                source: "srng 0.1;\n".to_string(),
                diagnostics: vec![ImportDiagnostic {
                    severity: "error".to_string(),
                    code: "S001".to_string(),
                    message: format!("invalid SVG/XML: {error}"),
                    element: None,
                }],
            }
        }
    };

    let root = xml.root_element();
    if root.tag_name().name() != "svg" {
        return ImportResult {
            source: "srng 0.1;\n".to_string(),
            diagnostics: vec![ImportDiagnostic {
                severity: "error".to_string(),
                code: "S002".to_string(),
                message: format!(
                    "expected an <svg> root element, found <{}>",
                    root.tag_name().name()
                ),
                element: None,
            }],
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
        source_name,
        source_svg: svg,
        options,
        width,
        height,
        declarations: Vec::new(),
        relations: Vec::new(),
        diagnostics: Vec::new(),
        ids: HashMap::new(),
        generated: 0,
    };

    let root_id = importer.unique_id(root.attribute("id").unwrap_or("svg_root"));
    let mut root_props = BTreeMap::new();
    root_props.insert("position".to_string(), "0px 0px".to_string());
    root_props.insert(
        "size".to_string(),
        format!("{}px {}px", fmt_num(width), fmt_num(height)),
    );
    root_props.insert("fill".to_string(), "none".to_string());
    root_props.insert("svg-source-tag".to_string(), quote("svg"));
    root_props.insert("svg-source".to_string(), quote(source_name));
    importer.preserve_attributes(root, &mut root_props);
    importer.declarations.push(Declaration::Node {
        kind: "canvas".to_string(),
        id: root_id.clone(),
        properties: root_props,
    });

    if let Some((min_x, min_y, vb_w, vb_h)) = view_box {
        if (min_x != 0.0 || min_y != 0.0 || (vb_w - width).abs() > f64::EPSILON || (vb_h - height).abs() > f64::EPSILON)
            && vb_w > 0.0
            && vb_h > 0.0
        {
            importer.warn(
                "S120",
                "root viewBox transform is preserved as metadata but is not yet baked into geometry",
                Some(&root_id),
            );
        }
    }

    let mut inherited = BTreeMap::new();
    inherited.insert("fill".to_string(), "#000000".to_string());
    inherited.insert("stroke".to_string(), "none".to_string());

    for child in root.children().filter(|n| n.is_element()) {
        importer.visit(child, Some(&root_id), &inherited);
    }

    let mut output = String::from("srng 0.1;\n");
    let file_id = options
        .file_id
        .clone()
        .unwrap_or_else(|| source_name.to_string());
    output.push_str(&format!("file {};\n\n", quote(&file_id)));

    for declaration in &importer.declarations {
        declaration.write_to(&mut output);
        output.push('\n');
    }
    for relation in &importer.relations {
        output.push_str(&format!(
            "relation {} -> {} {{\n    kind: contains;\n}}\n\n",
            relation.0, relation.1
        ));
    }

    ImportResult {
        source: output,
        diagnostics: importer.diagnostics,
    }
}

struct Importer<'a> {
    source_name: &'a str,
    source_svg: &'a str,
    options: &'a ImportOptions,
    width: f64,
    height: f64,
    declarations: Vec<Declaration>,
    relations: Vec<(String, String)>,
    diagnostics: Vec<ImportDiagnostic>,
    ids: HashMap<String, usize>,
    generated: usize,
}

impl Importer<'_> {
    fn visit(&mut self, node: Node<'_, '_>, parent: Option<&str>, inherited: &BTreeMap<String, String>) {
        let tag = node.tag_name().name();
        let local_style = collect_style(node);
        let mut effective_style = inherited.clone();
        for (key, value) in &local_style {
            effective_style.insert(key.clone(), value.clone());
        }

        if local_style.get("display").is_some_and(|v| v.trim() == "none") {
            let id = self.node_id(node, tag);
            let mut props = placeholder_props(tag, self.node_xml(node));
            self.preserve_attributes(node, &mut props);
            self.declarations.push(Declaration::Node {
                kind: "group".to_string(),
                id: id.clone(),
                properties: props,
            });
            if let Some(parent) = parent {
                self.relations.push((parent.to_string(), id.clone()));
            }
            self.info("S140", "display:none subtree preserved as non-rendering metadata", Some(&id));
            return;
        }

        if tag == "defs" || tag == "metadata" || tag == "title" || tag == "desc" {
            let id = self.node_id(node, tag);
            let mut props = placeholder_props(tag, self.node_xml(node));
            self.preserve_attributes(node, &mut props);
            self.declarations.push(Declaration::Node {
                kind: "group".to_string(),
                id: id.clone(),
                properties: props,
            });
            if let Some(parent) = parent {
                self.relations.push((parent.to_string(), id.clone()));
            }
            self.info("S141", "non-rendering SVG content preserved as metadata", Some(&id));
            return;
        }

        let id = self.node_id(node, tag);
        let mut props = BTreeMap::new();
        props.insert("svg-source-tag".to_string(), quote(tag));
        self.preserve_attributes(node, &mut props);

        let kind = match tag {
            "g" | "symbol" => {
                props.insert("position".to_string(), "0px 0px".to_string());
                props.insert("size".to_string(), "0px 0px".to_string());
                props.insert("fill".to_string(), "none".to_string());
                props.insert("stroke".to_string(), "none".to_string());
                "group"
            }
            "rect" => {
                let x = self.length_attr(node, "x", Axis::X, 0.0, &id);
                let y = self.length_attr(node, "y", Axis::Y, 0.0, &id);
                let width = self.length_attr(node, "width", Axis::X, 0.0, &id);
                let height = self.length_attr(node, "height", Axis::Y, 0.0, &id);
                props.insert("position".to_string(), pair_px(x, y));
                props.insert("size".to_string(), pair_px(width, height));
                if node.attribute("rx").is_some() || node.attribute("ry").is_some() {
                    self.warn("S210", "rounded rectangle radii are preserved but not rendered yet", Some(&id));
                }
                self.apply_paint(&effective_style, &mut props, &id);
                "rect"
            }
            "circle" => {
                let cx = self.length_attr(node, "cx", Axis::X, 0.0, &id);
                let cy = self.length_attr(node, "cy", Axis::Y, 0.0, &id);
                let r = self.length_attr(node, "r", Axis::X, 0.0, &id).max(0.0);
                props.insert("position".to_string(), pair_px(cx - r, cy - r));
                props.insert("size".to_string(), pair_px(r * 2.0, r * 2.0));
                self.apply_paint(&effective_style, &mut props, &id);
                "circle"
            }
            "ellipse" => {
                let cx = self.length_attr(node, "cx", Axis::X, 0.0, &id);
                let cy = self.length_attr(node, "cy", Axis::Y, 0.0, &id);
                let rx = self.length_attr(node, "rx", Axis::X, 0.0, &id).max(0.0);
                let ry = self.length_attr(node, "ry", Axis::Y, 0.0, &id).max(0.0);
                props.insert("position".to_string(), pair_px(cx - rx, cy - ry));
                props.insert("size".to_string(), pair_px(rx * 2.0, ry * 2.0));
                self.apply_paint(&effective_style, &mut props, &id);
                "ellipse"
            }
            "path" => {
                props.insert("position".to_string(), "0px 0px".to_string());
                if let Some(data) = node.attribute("d") {
                    props.insert("data".to_string(), quote(data));
                } else {
                    self.warn("S220", "path has no `d` data", Some(&id));
                }
                self.apply_paint(&effective_style, &mut props, &id);
                "path"
            }
            "line" => {
                let x1 = self.length_attr(node, "x1", Axis::X, 0.0, &id);
                let y1 = self.length_attr(node, "y1", Axis::Y, 0.0, &id);
                let x2 = self.length_attr(node, "x2", Axis::X, 0.0, &id);
                let y2 = self.length_attr(node, "y2", Axis::Y, 0.0, &id);
                props.insert("position".to_string(), "0px 0px".to_string());
                props.insert(
                    "data".to_string(),
                    quote(&format!(
                        "M {} {} L {} {}",
                        fmt_num(x1), fmt_num(y1), fmt_num(x2), fmt_num(y2)
                    )),
                );
                self.apply_paint(&effective_style, &mut props, &id);
                "path"
            }
            "polyline" | "polygon" => {
                props.insert("position".to_string(), "0px 0px".to_string());
                match node.attribute("points").and_then(points_to_path) {
                    Some(mut data) => {
                        if tag == "polygon" {
                            data.push_str(" Z");
                        }
                        props.insert("data".to_string(), quote(&data));
                    }
                    None => self.warn("S221", "invalid or missing polygon/polyline points", Some(&id)),
                }
                self.apply_paint(&effective_style, &mut props, &id);
                "path"
            }
            "text" | "tspan" => {
                let x = self.length_attr(node, "x", Axis::X, 0.0, &id);
                let y = self.length_attr(node, "y", Axis::Y, 0.0, &id);
                props.insert("position".to_string(), pair_px(x, y));
                props.insert("content".to_string(), quote(node.text().unwrap_or("")));
                self.apply_paint(&effective_style, &mut props, &id);
                self.warn(
                    "S301",
                    "text is preserved, but SRNG v0.1 renderer requires pre-shaped outline path data for visual rendering",
                    Some(&id),
                );
                "text"
            }
            "image" => {
                let x = self.length_attr(node, "x", Axis::X, 0.0, &id);
                let y = self.length_attr(node, "y", Axis::Y, 0.0, &id);
                let width = self.length_attr(node, "width", Axis::X, 0.0, &id);
                let height = self.length_attr(node, "height", Axis::Y, 0.0, &id);
                props.insert("position".to_string(), pair_px(x, y));
                props.insert("size".to_string(), pair_px(width, height));
                props.insert("fill".to_string(), "none".to_string());
                self.warn("S302", "SVG raster/image content is preserved as metadata but is not rendered in SRNG v0.1", Some(&id));
                "group"
            }
            "use" => {
                props.insert("position".to_string(), "0px 0px".to_string());
                props.insert("size".to_string(), "0px 0px".to_string());
                props.insert("fill".to_string(), "none".to_string());
                self.warn("S303", "SVG <use> is preserved as metadata; reference conversion is deferred until definition visibility can be preserved exactly", Some(&id));
                "group"
            }
            _ => {
                props.insert("position".to_string(), "0px 0px".to_string());
                props.insert("size".to_string(), "0px 0px".to_string());
                props.insert("fill".to_string(), "none".to_string());
                props.insert("svg-source-xml".to_string(), quote(&self.node_xml(node)));
                self.warn(
                    "S399",
                    &format!("unsupported SVG element <{tag}> preserved as metadata"),
                    Some(&id),
                );
                "group"
            }
        };

        if node.attribute("transform").is_some() {
            self.warn(
                "S130",
                "transform is preserved as source metadata but is not yet baked into SRNG geometry",
                Some(&id),
            );
        }
        for key in ["opacity", "fill-opacity", "stroke-opacity", "filter", "mask", "clip-path"] {
            if local_style.contains_key(key) || node.attribute(key).is_some() {
                self.warn(
                    "S131",
                    &format!("`{key}` is preserved as metadata but is not yet represented by the v0.1 renderer"),
                    Some(&id),
                );
            }
        }

        self.declarations.push(Declaration::Node {
            kind: kind.to_string(),
            id: id.clone(),
            properties: props,
        });
        if let Some(parent) = parent {
            self.relations.push((parent.to_string(), id.clone()));
        }

        for child in node.children().filter(|n| n.is_element()) {
            self.visit(child, Some(&id), &effective_style);
        }
    }

    fn apply_paint(&mut self, style: &BTreeMap<String, String>, props: &mut BTreeMap<String, String>, id: &str) {
        for key in ["fill", "stroke"] {
            if let Some(value) = style.get(key) {
                match normalize_paint(value) {
                    Some(value) => {
                        props.insert(key.to_string(), value);
                    }
                    None => {
                        props.insert(format!("svg-{key}"), quote(value));
                        self.warn(
                            "S230",
                            &format!("unsupported SVG {key} paint `{value}` preserved but not rendered"),
                            Some(id),
                        );
                    }
                }
            }
        }
        for key in [
            "fill-rule",
            "stroke-linecap",
            "stroke-linejoin",
            "stroke-dasharray",
            "stroke-dashoffset",
        ] {
            if let Some(value) = style.get(key) {
                props.insert(key.to_string(), value.clone());
            }
        }
        if let Some(value) = style.get("stroke-width") {
            if let Some(px) = resolve_length(value, Axis::X, self.width, self.height) {
                props.insert("stroke-width".to_string(), format!("{}px", fmt_num(px)));
            } else {
                props.insert("svg-stroke-width".to_string(), quote(value));
                self.warn("S231", "unsupported stroke-width unit preserved but not rendered", Some(id));
            }
        }
    }

    fn length_attr(&mut self, node: Node<'_, '_>, name: &str, axis: Axis, default: f64, id: &str) -> f64 {
        match node.attribute(name) {
            Some(value) => match resolve_length(value, axis, self.width, self.height) {
                Some(value) => value,
                None => {
                    self.warn(
                        "S201",
                        &format!("unsupported length `{value}` for `{name}`; using {default}px for rendering and preserving the original attribute"),
                        Some(id),
                    );
                    default
                }
            },
            None => default,
        }
    }

    fn preserve_attributes(&self, node: Node<'_, '_>, props: &mut BTreeMap<String, String>) {
        if !self.options.preserve_source_attributes {
            return;
        }
        for attribute in node.attributes() {
            let key = format!("svg-attr-{}", safe_property_suffix(attribute.name()));
            props.insert(key, quote(attribute.value()));
        }
    }

    fn node_id(&mut self, node: Node<'_, '_>, tag: &str) -> String {
        if let Some(id) = node.attribute("id") {
            self.unique_id(id)
        } else {
            self.generated += 1;
            self.unique_id(&format!("{tag}_{}", self.generated))
        }
    }

    fn unique_id(&mut self, raw: &str) -> String {
        let base = safe_ident(raw);
        let count = self.ids.entry(base.clone()).or_insert(0);
        *count += 1;
        if *count == 1 {
            base
        } else {
            format!("{base}_{}", *count)
        }
    }

    fn node_xml(&self, node: Node<'_, '_>) -> String {
        let range = node.range();
        self.source_svg.get(range).unwrap_or("").to_string()
    }

    fn warn(&mut self, code: &str, message: &str, element: Option<&str>) {
        self.diagnostics.push(ImportDiagnostic {
            severity: "warning".to_string(),
            code: code.to_string(),
            message: message.to_string(),
            element: element.map(ToOwned::to_owned),
        });
    }

    fn info(&mut self, code: &str, message: &str, element: Option<&str>) {
        self.diagnostics.push(ImportDiagnostic {
            severity: "info".to_string(),
            code: code.to_string(),
            message: message.to_string(),
            element: element.map(ToOwned::to_owned),
        });
    }
}

enum Declaration {
    Node {
        kind: String,
        id: String,
        properties: BTreeMap<String, String>,
    },
}

impl Declaration {
    fn write_to(&self, out: &mut String) {
        match self {
            Declaration::Node { kind, id, properties } => {
                out.push_str(&format!("{kind} {id} {{\n"));
                for (key, value) in properties {
                    out.push_str(&format!("    {key}: {value};\n"));
                }
                out.push_str("}\n");
            }
        }
    }
}

#[derive(Clone, Copy)]
enum Axis {
    X,
    Y,
}

fn collect_style(node: Node<'_, '_>) -> BTreeMap<String, String> {
    let mut style = BTreeMap::new();
    for key in [
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
        "filter",
        "mask",
        "clip-path",
        "display",
        "visibility",
    ] {
        if let Some(value) = node.attribute(key) {
            style.insert(key.to_string(), value.trim().to_string());
        }
    }
    if let Some(inline) = node.attribute("style") {
        for declaration in inline.split(';') {
            let Some((key, value)) = declaration.split_once(':') else {
                continue;
            };
            let key = key.trim();
            if !key.is_empty() {
                style.insert(key.to_string(), value.trim().to_string());
            }
        }
    }
    style
}

fn normalize_paint(value: &str) -> Option<String> {
    let value = value.trim();
    if value.eq_ignore_ascii_case("none") {
        return Some("none".to_string());
    }
    if value.starts_with('#') && matches!(value.len(), 4 | 5 | 7 | 9) {
        return Some(value.to_string());
    }
    let named = match value.to_ascii_lowercase().as_str() {
        "black" => "#000000",
        "white" => "#ffffff",
        "red" => "#ff0000",
        "green" => "#008000",
        "blue" => "#0000ff",
        "yellow" => "#ffff00",
        "gray" | "grey" => "#808080",
        "transparent" => "#00000000",
        _ => "",
    };
    if !named.is_empty() {
        return Some(named.to_string());
    }
    parse_rgb(value)
}

fn parse_rgb(value: &str) -> Option<String> {
    let inner = value.strip_prefix("rgb(")?.strip_suffix(')')?;
    let values = inner
        .split(',')
        .map(|part| part.trim().parse::<u8>().ok())
        .collect::<Option<Vec<_>>>()?;
    if values.len() != 3 {
        return None;
    }
    Some(format!("#{:02x}{:02x}{:02x}", values[0], values[1], values[2]))
}

fn resolve_length(value: &str, axis: Axis, viewport_width: f64, viewport_height: f64) -> Option<f64> {
    let value = value.trim();
    if let Ok(number) = value.parse::<f64>() {
        return Some(number);
    }
    let units = ["px", "pt", "mm", "cm", "in", "%"];
    let unit = units.iter().find(|unit| value.ends_with(**unit))?;
    let number = value[..value.len() - unit.len()].trim().parse::<f64>().ok()?;
    Some(match *unit {
        "px" => number,
        "pt" => number * 96.0 / 72.0,
        "in" => number * 96.0,
        "cm" => number * 96.0 / 2.54,
        "mm" => number * 96.0 / 25.4,
        "%" => {
            let basis = match axis {
                Axis::X => viewport_width,
                Axis::Y => viewport_height,
            };
            basis * number / 100.0
        }
        _ => return None,
    })
}

fn parse_view_box(value: Option<&str>) -> Option<(f64, f64, f64, f64)> {
    let values = value?
        .split(|c: char| c == ',' || c.is_ascii_whitespace())
        .filter(|part| !part.is_empty())
        .map(|part| part.parse::<f64>().ok())
        .collect::<Option<Vec<_>>>()?;
    if values.len() == 4 && values[2] >= 0.0 && values[3] >= 0.0 {
        Some((values[0], values[1], values[2], values[3]))
    } else {
        None
    }
}

fn points_to_path(value: &str) -> Option<String> {
    let values = value
        .split(|c: char| c == ',' || c.is_ascii_whitespace())
        .filter(|part| !part.is_empty())
        .map(|part| part.parse::<f64>().ok())
        .collect::<Option<Vec<_>>>()?;
    if values.len() < 2 || values.len() % 2 != 0 {
        return None;
    }
    let mut out = format!("M {} {}", fmt_num(values[0]), fmt_num(values[1]));
    for pair in values[2..].chunks_exact(2) {
        out.push_str(&format!(" L {} {}", fmt_num(pair[0]), fmt_num(pair[1])));
    }
    Some(out)
}

fn placeholder_props(tag: &str, xml: String) -> BTreeMap<String, String> {
    let mut props = BTreeMap::new();
    props.insert("position".to_string(), "0px 0px".to_string());
    props.insert("size".to_string(), "0px 0px".to_string());
    props.insert("fill".to_string(), "none".to_string());
    props.insert("stroke".to_string(), "none".to_string());
    props.insert("svg-source-tag".to_string(), quote(tag));
    props.insert("svg-source-xml".to_string(), quote(&xml));
    props
}

fn safe_ident(value: &str) -> String {
    let mut out = String::new();
    for (index, ch) in value.chars().enumerate() {
        let valid = if index == 0 {
            ch.is_ascii_alphabetic() || ch == '_' || ch == '%'
        } else {
            ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | '/' | '%')
        };
        out.push(if valid { ch } else { '_' });
    }
    if out.is_empty() {
        "svg_node".to_string()
    } else if out.chars().next().is_some_and(|ch| ch.is_ascii_digit()) {
        format!("svg_{out}")
    } else {
        out
    }
}

fn safe_property_suffix(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.') {
                ch
            } else {
                '-'
            }
        })
        .collect()
}

fn quote(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

fn pair_px(x: f64, y: f64) -> String {
    format!("{}px {}px", fmt_num(x), fmt_num(y))
}

fn fmt_num(value: f64) -> String {
    if value.fract().abs() < 1e-9 {
        format!("{:.0}", value)
    } else {
        let mut out = format!("{value:.6}");
        while out.ends_with('0') {
            out.pop();
        }
        if out.ends_with('.') {
            out.pop();
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn imports_basic_geometry_and_compiles() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="80"><rect id="card" x="10" y="20" width="30" height="40" fill="#ff0000"/></svg>"#;
        let imported = import_svg(svg, "basic.svg", &ImportOptions::default());
        assert!(!imported.has_errors());
        assert!(imported.source.contains("rect card"));
        assert!(imported.source.contains("position: 10px 20px;"));
        assert!(imported.source.contains("size: 30px 40px;"));
        let document = crate::compile(&imported.source);
        assert!(!document.diagnostics.iter().any(|d| matches!(d.severity, crate::diagnostic::Severity::Error)));
    }

    #[test]
    fn inherits_svg_fill() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg"><g fill="red"><circle id="dot" cx="10" cy="10" r="5"/></g></svg>"#;
        let imported = import_svg(svg, "style.svg", &ImportOptions::default());
        assert!(imported.source.contains("circle dot"));
        assert!(imported.source.contains("fill: #ff0000;"));
    }

    #[test]
    fn converts_polyline_to_path() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg"><polyline id="p" points="0,0 10,5 20,0" stroke="#000" fill="none"/></svg>"#;
        let imported = import_svg(svg, "poly.svg", &ImportOptions::default());
        assert!(imported.source.contains("data: \"M 0 0 L 10 5 L 20 0\";"));
    }

    #[test]
    fn preserves_unsupported_content_in_metadata() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg"><foreignObject id="x" width="10" height="10"><div xmlns="http://www.w3.org/1999/xhtml">Hi</div></foreignObject></svg>"#;
        let imported = import_svg(svg, "unsupported.svg", &ImportOptions::default());
        assert!(imported.source.contains("svg-source-xml"));
        assert!(imported.diagnostics.iter().any(|d| d.code == "S399"));
    }

    #[test]
    fn duplicate_and_invalid_ids_are_made_safe() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg"><rect id="1 bad"/><rect id="1 bad"/></svg>"#;
        let imported = import_svg(svg, "ids.svg", &ImportOptions::default());
        assert!(imported.source.contains("rect svg__bad"));
        assert!(imported.source.contains("rect svg__bad_2"));
    }
}
