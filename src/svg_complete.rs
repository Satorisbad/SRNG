use roxmltree::{Document as XmlDocument, Node};
use std::collections::HashMap;

pub use crate::svg_base::{ImportDiagnostic, ImportOptions, ImportResult};
pub use crate::svg_base::strip_svg_provenance;

#[derive(Clone, Debug)]
struct GradientResource {
    kind: &'static str,
    units: String,
    x1: String,
    y1: String,
    x2: String,
    y2: String,
    cx: String,
    cy: String,
    r: String,
    fx: String,
    fy: String,
    stops: String,
}

#[derive(Clone, Debug)]
struct MaskMeta {
    units: String,
    content_units: String,
}

#[derive(Clone, Debug)]
struct UseTarget {
    data: String,
    fill: String,
    stroke: String,
}

/// Import SVG and then promote the remaining high-value SVG semantics into
/// native SRNG properties. The lower-level importer remains the fault-tolerant
/// compatibility parser; this facade is the format-facing contract.
pub fn import_svg(svg: &str, source_name: &str, options: &ImportOptions) -> ImportResult {
    let mut result = crate::svg_base::import_svg(svg, source_name, options);
    let features = promote_advanced(svg, &mut result.source);

    for diagnostic in &mut result.diagnostics {
        match diagnostic.code.as_str() {
            "S120" if features.viewbox => {
                diagnostic.severity = "info".into();
                diagnostic.message = "viewBox is represented by native SRNG viewbox/preserve-aspect-ratio semantics".into();
            }
            "S131" if diagnostic.message.contains("opacity") => {
                diagnostic.severity = "info".into();
                diagnostic.message = diagnostic.message.replace("is preserved but not yet represented by the renderer", "is represented by native SRNG opacity semantics");
            }
            "S230" if features.gradients && diagnostic.message.contains("url(#") => {
                diagnostic.severity = "info".into();
                diagnostic.message = diagnostic.message.replace("unsupported", "native gradient").replace("preserved but not rendered", "mapped to SRNG gradient semantics");
            }
            "S301" if features.text => {
                diagnostic.severity = "info".into();
                diagnostic.message = "text is represented natively and uses the practical text rendering path when no outline data is present".into();
            }
            "S302" if features.images && diagnostic.message.contains("<image>") => {
                diagnostic.severity = "info".into();
                diagnostic.message = "embedded/local image metadata is represented by native SRNG image properties".into();
            }
            "S302" if features.uses && diagnostic.message.contains("<use>") => {
                diagnostic.severity = "info".into();
                diagnostic.message = "simple <use> references are resolved to native SRNG path semantics".into();
            }
            _ => {}
        }
    }
    result
}

#[derive(Default)]
struct PromotedFeatures {
    viewbox: bool,
    gradients: bool,
    text: bool,
    images: bool,
    uses: bool,
}

fn promote_advanced(svg: &str, source: &mut String) -> PromotedFeatures {
    let Ok(document) = XmlDocument::parse(svg) else { return PromotedFeatures::default(); };
    let gradients = collect_gradients(&document);
    let masks = collect_masks(&document);
    let uses = collect_use_targets(&document);
    let mut features = PromotedFeatures {
        viewbox: document.root_element().attribute("viewBox").is_some(),
        gradients: !gradients.is_empty(),
        text: document.descendants().any(|n| n.is_element() && matches!(n.tag_name().name(), "text" | "tspan")),
        images: document.descendants().any(|n| n.is_element() && n.tag_name().name() == "image"),
        uses: document.descendants().any(|n| n.is_element() && n.tag_name().name() == "use"),
    };

    let mut out = String::with_capacity(source.len() + source.len() / 5);
    for line in source.lines() {
        out.push_str(line);
        out.push('\n');
        let trimmed = line.trim_start();
        let indent = &line[..line.len() - trimmed.len()];

        let alias = [
            ("svg-attr-transform:", "transform:"),
            ("svg-attr-opacity:", "opacity:"),
            ("svg-attr-fill-opacity:", "fill-opacity:"),
            ("svg-attr-stroke-opacity:", "stroke-opacity:"),
            ("svg-attr-viewBox:", "viewbox:"),
            ("svg-attr-preserveAspectRatio:", "preserve-aspect-ratio:"),
            ("svg-attr-clipPathUnits:", "clip-units:"),
            ("svg-attr-maskUnits:", "mask-units:"),
            ("svg-attr-maskContentUnits:", "mask-content-units:"),
            ("svg-attr-font-size:", "font-size:"),
            ("svg-attr-font-family:", "font-family:"),
            ("svg-attr-font-weight:", "font-weight:"),
            ("svg-attr-font-style:", "font-style:"),
            ("svg-attr-text-anchor:", "text-anchor:"),
            ("svg-attr-preserveAspectRatio:", "image-preserve-aspect-ratio:"),
            ("svg-attr-x:", "source-x:"),
            ("svg-attr-y:", "source-y:"),
            ("svg-attr-width:", "source-width:"),
            ("svg-attr-height:", "source-height:"),
        ];
        for (from, to) in alias {
            if let Some(rest) = trimmed.strip_prefix(from) {
                out.push_str(indent);
                out.push_str(to);
                out.push_str(rest);
                out.push('\n');
            }
        }

        if let Some(rest) = trimmed.strip_prefix("svg-attr-href:").or_else(|| trimmed.strip_prefix("svg-attr-xlink-href:")) {
            out.push_str(indent);
            out.push_str("href:");
            out.push_str(rest);
            out.push('\n');
            if let Some(raw) = property_string_from_rest(rest) {
                if let Some(id) = raw.strip_prefix('#') {
                    if let Some(target) = uses.get(id) {
                        emit_prop(&mut out, indent, "use-data", &quote(&target.data));
                        emit_prop(&mut out, indent, "use-fill", &target.fill);
                        emit_prop(&mut out, indent, "use-stroke", &target.stroke);
                    }
                }
            }
        }

        if let Some(raw) = property_string(trimmed, "svg-fill:") {
            if let Some(id) = url_fragment(&raw) {
                if let Some(gradient) = gradients.get(id) {
                    emit_gradient(&mut out, indent, gradient);
                }
            }
        }

        if let Some(raw) = property_string(trimmed, "mask-ref:").or_else(|| property_string(trimmed, "svg-attr-mask:")) {
            if let Some(id) = url_fragment(&raw) {
                if let Some(mask) = masks.get(id) {
                    emit_prop(&mut out, indent, "mask-units", &quote(&mask.units));
                    emit_prop(&mut out, indent, "mask-content-units", &quote(&mask.content_units));
                }
            }
        }
    }

    // Keep the feature flags honest: a gradient only counts as promoted if at
    // least one native gradient declaration was actually emitted.
    features.gradients &= out.contains("gradient-kind:");
    *source = out;
    features
}

fn emit_gradient(out: &mut String, indent: &str, gradient: &GradientResource) {
    emit_prop(out, indent, "fill", gradient.kind);
    emit_prop(out, indent, "gradient-kind", &quote(gradient.kind));
    emit_prop(out, indent, "gradient-units", &quote(&gradient.units));
    emit_prop(out, indent, "gradient-stops", &gradient.stops);
    if gradient.kind == "linear-gradient" {
        emit_prop(out, indent, "gradient-x1", &quote(&gradient.x1));
        emit_prop(out, indent, "gradient-y1", &quote(&gradient.y1));
        emit_prop(out, indent, "gradient-x2", &quote(&gradient.x2));
        emit_prop(out, indent, "gradient-y2", &quote(&gradient.y2));
    } else {
        emit_prop(out, indent, "gradient-cx", &quote(&gradient.cx));
        emit_prop(out, indent, "gradient-cy", &quote(&gradient.cy));
        emit_prop(out, indent, "gradient-r", &quote(&gradient.r));
        emit_prop(out, indent, "gradient-fx", &quote(&gradient.fx));
        emit_prop(out, indent, "gradient-fy", &quote(&gradient.fy));
    }
}

fn emit_prop(out: &mut String, indent: &str, key: &str, value: &str) {
    out.push_str(indent);
    out.push_str(key);
    out.push_str(": ");
    out.push_str(value);
    out.push_str(";\n");
}

fn collect_gradients(document: &XmlDocument<'_>) -> HashMap<String, GradientResource> {
    let mut out = HashMap::new();
    for node in document.descendants().filter(|n| n.is_element()) {
        let kind = match node.tag_name().name() {
            "linearGradient" => "linear-gradient",
            "radialGradient" => "radial-gradient",
            _ => continue,
        };
        let Some(id) = node.attribute("id") else { continue; };
        let units = node.attribute("gradientUnits").unwrap_or("objectBoundingBox").to_string();
        let stops = gradient_stops(node);
        if stops.is_empty() { continue; }
        out.insert(id.to_string(), GradientResource {
            kind,
            units,
            x1: node.attribute("x1").unwrap_or("0%").to_string(),
            y1: node.attribute("y1").unwrap_or("0%").to_string(),
            x2: node.attribute("x2").unwrap_or("100%").to_string(),
            y2: node.attribute("y2").unwrap_or("0%").to_string(),
            cx: node.attribute("cx").unwrap_or("50%").to_string(),
            cy: node.attribute("cy").unwrap_or("50%").to_string(),
            r: node.attribute("r").unwrap_or("50%").to_string(),
            fx: node.attribute("fx").or_else(|| node.attribute("cx")).unwrap_or("50%").to_string(),
            fy: node.attribute("fy").or_else(|| node.attribute("cy")).unwrap_or("50%").to_string(),
            stops,
        });
    }
    out
}

fn gradient_stops(gradient: Node<'_, '_>) -> String {
    let mut result = Vec::new();
    for stop in gradient.children().filter(|n| n.is_element() && n.tag_name().name() == "stop") {
        let mut color = stop.attribute("stop-color").unwrap_or("#000000").to_string();
        let mut opacity = stop.attribute("stop-opacity").and_then(parse_f64).unwrap_or(1.0);
        if let Some(style) = stop.attribute("style") {
            for field in style.split(';') {
                if let Some((key, value)) = field.split_once(':') {
                    match key.trim() {
                        "stop-color" => color = value.trim().to_string(),
                        "stop-opacity" => opacity = parse_f64(value).unwrap_or(opacity),
                        _ => {}
                    }
                }
            }
        }
        let Some(mut rgba) = normalize_color(&color) else { continue; };
        rgba.3 = ((f64::from(rgba.3) * opacity.clamp(0.0, 1.0)).round()) as u8;
        let offset = stop.attribute("offset").unwrap_or("0");
        result.push(format!("{} {}", normalize_offset(offset), rgba_hex(rgba)));
    }
    result.join(", ")
}

fn collect_masks(document: &XmlDocument<'_>) -> HashMap<String, MaskMeta> {
    let mut out = HashMap::new();
    for node in document.descendants().filter(|n| n.is_element() && n.tag_name().name() == "mask") {
        let Some(id) = node.attribute("id") else { continue; };
        out.insert(id.to_string(), MaskMeta {
            units: node.attribute("maskUnits").unwrap_or("objectBoundingBox").to_string(),
            content_units: node.attribute("maskContentUnits").unwrap_or("userSpaceOnUse").to_string(),
        });
    }
    out
}

fn collect_use_targets(document: &XmlDocument<'_>) -> HashMap<String, UseTarget> {
    let mut out = HashMap::new();
    for node in document.descendants().filter(|n| n.is_element()) {
        let Some(id) = node.attribute("id") else { continue; };
        let Some(data) = shape_path(node) else { continue; };
        let fill = node.attribute("fill").and_then(normalize_color_string).unwrap_or_else(|| "#000000".into());
        let stroke = node.attribute("stroke").and_then(normalize_color_string).unwrap_or_else(|| "none".into());
        out.insert(id.to_string(), UseTarget { data, fill, stroke });
    }
    out
}

fn shape_path(node: Node<'_, '_>) -> Option<String> {
    let n = |name: &str, default: f64| node.attribute(name).and_then(parse_f64).unwrap_or(default);
    match node.tag_name().name() {
        "path" => node.attribute("d").map(str::to_string),
        "rect" => {
            let x = n("x", 0.0); let y = n("y", 0.0); let w = n("width", 0.0); let h = n("height", 0.0);
            Some(format!("M {x} {y} H {} V {} H {x} Z", x + w, y + h))
        }
        "circle" => {
            let cx = n("cx", 0.0); let cy = n("cy", 0.0); let r = n("r", 0.0);
            Some(format!("M {} {cy} A {r} {r} 0 1 0 {} {cy} A {r} {r} 0 1 0 {} {cy} Z", cx-r, cx+r, cx-r))
        }
        "ellipse" => {
            let cx=n("cx",0.0); let cy=n("cy",0.0); let rx=n("rx",0.0); let ry=n("ry",0.0);
            Some(format!("M {} {cy} A {rx} {ry} 0 1 0 {} {cy} A {rx} {ry} 0 1 0 {} {cy} Z", cx-rx,cx+rx,cx-rx))
        }
        "line" => Some(format!("M {} {} L {} {}",n("x1",0.0),n("y1",0.0),n("x2",0.0),n("y2",0.0))),
        "polygon" | "polyline" => {
            let vals = node.attribute("points")?.split(|c: char| c==',' || c.is_whitespace()).filter(|s| !s.is_empty()).collect::<Vec<_>>();
            if vals.len() < 2 || vals.len()%2!=0 { return None; }
            let mut d=format!("M {} {}", vals[0], vals[1]);
            for p in vals[2..].chunks_exact(2) { d.push_str(&format!(" L {} {}",p[0],p[1])); }
            if node.tag_name().name()=="polygon" { d.push_str(" Z"); }
            Some(d)
        }
        _ => None,
    }
}

fn normalize_offset(value: &str) -> String {
    let value = value.trim();
    if let Some(p) = value.strip_suffix('%').and_then(parse_f64) {
        format!("{}", (p / 100.0).clamp(0.0, 1.0))
    } else {
        format!("{}", parse_f64(value).unwrap_or(0.0).clamp(0.0, 1.0))
    }
}

fn normalize_color_string(value: &str) -> Option<String> { normalize_color(value).map(rgba_hex) }
fn normalize_color(value: &str) -> Option<(u8,u8,u8,u8)> {
    let v=value.trim().to_ascii_lowercase();
    let named = match v.as_str() {
        "black"=>Some("#000000"),"white"=>Some("#ffffff"),"red"=>Some("#ff0000"),"green"=>Some("#008000"),"blue"=>Some("#0000ff"),"yellow"=>Some("#ffff00"),"gray"|"grey"=>Some("#808080"),"transparent"=>Some("#00000000"),_=>None
    };
    let v=named.unwrap_or(&v);
    let h=v.strip_prefix('#')?;
    let byte=|s:&str| u8::from_str_radix(s,16).ok();
    match h.len() {
        3=>Some((byte(&h[0..1].repeat(2))?,byte(&h[1..2].repeat(2))?,byte(&h[2..3].repeat(2))?,255)),
        4=>Some((byte(&h[0..1].repeat(2))?,byte(&h[1..2].repeat(2))?,byte(&h[2..3].repeat(2))?,byte(&h[3..4].repeat(2))?)),
        6=>Some((byte(&h[0..2])?,byte(&h[2..4])?,byte(&h[4..6])?,255)),
        8=>Some((byte(&h[0..2])?,byte(&h[2..4])?,byte(&h[4..6])?,byte(&h[6..8])?)),
        _=>None,
    }
}
fn rgba_hex(c:(u8,u8,u8,u8))->String { format!("#{:02x}{:02x}{:02x}{:02x}",c.0,c.1,c.2,c.3) }
fn parse_f64(value:&str)->Option<f64>{ value.trim().trim_end_matches("px").parse().ok() }
fn url_fragment(value:&str)->Option<&str>{ value.trim().strip_prefix("url(#")?.strip_suffix(')') }
fn property_string(line:&str,prefix:&str)->Option<String>{ let v=line.strip_prefix(prefix)?.trim().strip_suffix(';')?.trim(); Some(unquote(v)) }
fn property_string_from_rest(rest:&str)->Option<String>{ Some(unquote(rest.trim().strip_suffix(';')?.trim())) }
fn unquote(v:&str)->String{ let v=v.trim(); let Some(i)=v.strip_prefix('"').and_then(|x|x.strip_suffix('"')) else{return v.to_string()}; i.replace("\\\"","\"").replace("\\n","\n").replace("\\\\","\\") }
fn quote(v:&str)->String{ format!("\"{}\"",v.replace('\\',"\\\\").replace('"',"\\\"").replace('\n',"\\n")) }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn promotes_viewbox_transform_opacity_and_linear_gradient() {
        let svg=r##"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="100" viewBox="0 0 100 50"><defs><linearGradient id="g"><stop offset="0" stop-color="#ff0000"/><stop offset="100%" stop-color="#0000ff"/></linearGradient></defs><rect id="r" transform="translate(2 3)" opacity="0.5" width="100" height="50" fill="url(#g)"/></svg>"##;
        let result=import_svg(svg,"a.svg",&ImportOptions::default());
        assert!(result.source.contains("viewbox: \"0 0 100 50\";"));
        assert!(result.source.contains("transform: \"translate(2 3)\";"));
        assert!(result.source.contains("opacity: \"0.5\";"));
        assert!(result.source.contains("fill: linear-gradient;"));
        assert!(result.source.contains("gradient-stops: 0 #ff0000ff, 1 #0000ffff;"));
    }

    #[test]
    fn promotes_simple_use_target() {
        let svg=r##"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20"><defs><path id="p" d="M0 0 L10 0 L10 10 Z" fill="#ff0000"/></defs><use href="#p" x="5" y="6"/></svg>"##;
        let result=import_svg(svg,"a.svg",&ImportOptions::default());
        assert!(result.source.contains("use-data: \"M0 0 L10 0 L10 10 Z\";"));
        assert!(result.source.contains("use-fill: #ff0000;"));
    }
}
