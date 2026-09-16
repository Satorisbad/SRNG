use srng::runtime::{Geometry, Scene};
use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Debug, Clone, PartialEq)]
pub struct PreparedScene {
    pub width: u16,
    pub height: u16,
    pub revision: u64,
    pub commands: Vec<Command>,
    pub diagnostics: Vec<RenderDiagnostic>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    PushClip { path: PathData, rule: FillRule },
    PopClip,
    Fill { path: PathData, paint: Paint, rule: FillRule },
    Stroke { path: PathData, paint: Paint, style: StrokeStyle },
}

#[derive(Debug, Clone, PartialEq)]
pub struct PathData {
    pub svg: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Paint {
    Solid(Rgba),
    LinearGradient {
        start: (f64, f64),
        end: (f64, f64),
        stops: Vec<GradientStop>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GradientStop {
    pub offset: f32,
    pub color: Rgba,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FillRule {
    NonZero,
    EvenOdd,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StrokeStyle {
    pub width: f64,
    pub miter_limit: f64,
    pub line_cap: LineCap,
    pub line_join: LineJoin,
    pub dash: Vec<f64>,
    pub dash_offset: f64,
}

impl Default for StrokeStyle {
    fn default() -> Self {
        Self {
            width: 1.0,
            miter_limit: 4.0,
            line_cap: LineCap::Butt,
            line_join: LineJoin::Miter,
            dash: Vec::new(),
            dash_offset: 0.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineCap {
    Butt,
    Round,
    Square,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineJoin {
    Miter,
    Round,
    Bevel,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderDiagnostic {
    pub severity: String,
    pub code: String,
    pub message: String,
    pub declaration: Option<String>,
}

#[derive(Debug, Default)]
pub struct RevisionGate(AtomicU64);

impl RevisionGate {
    pub fn begin(&self) -> u64 {
        self.0.fetch_add(1, Ordering::SeqCst) + 1
    }

    pub fn current(&self) -> u64 {
        self.0.load(Ordering::SeqCst)
    }

    pub fn is_current(&self, revision: u64) -> bool {
        self.current() == revision
    }
}

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

        let (id, kind, geometry, props): (&str, &str, Cow<'_, Geometry>, Cow<'_, BTreeMap<String, String>>) =
            match source {
                Source::Node(node) => (
                    node.id.as_str(),
                    node.kind.as_str(),
                    Cow::Borrowed(&node.geometry),
                    Cow::Borrowed(&node.properties),
                ),
                Source::Reference(reference) => {
                    let geometry = merge_geometry(
                        reference.linked_geometry.as_ref(),
                        &reference.geometry,
                    );
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
                    format!(
                        "text node `{id}` requires pre-shaped outline path data in `data`"
                    ),
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
                if let Some(clip_path) =
                    path_for(&clip_node.kind, &clip_node.geometry, &clip_node.properties)
                {
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

fn path_for(
    kind: &str,
    geometry: &Geometry,
    props: &BTreeMap<String, String>,
) -> Option<PathData> {
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
    parse_color(value).map(Paint::Solid)
}

fn parse_gradient_stops(value: &str) -> Result<Vec<GradientStop>, String> {
    value
        .split(',')
        .map(|part| {
            let mut fields = part.split_whitespace();
            let offset = fields
                .next()
                .ok_or_else(|| "gradient stop is missing offset".to_string())?;
            let color = fields
                .next()
                .ok_or_else(|| "gradient stop is missing color".to_string())?;
            let offset = if let Some(value) = offset.strip_suffix('%') {
                value
                    .parse::<f32>()
                    .map_err(|_| "invalid gradient stop offset".to_string())?
                    / 100.0
            } else {
                offset
                    .parse::<f32>()
                    .map_err(|_| "invalid gradient stop offset".to_string())?
            };
            if !(0.0..=1.0).contains(&offset) {
                return Err("gradient stop offset must be in 0..=1".to_string());
            }
            Ok(GradientStop {
                offset,
                color: parse_color(color)?,
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
    value.trim().trim_end_matches("px").parse().ok()
}

fn unquote(value: &str) -> String {
    value
        .trim()
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .unwrap_or(value.trim())
        .to_string()
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

#[cfg(feature = "cpu")]
pub mod cpu {
    use super::*;
    use vello_cpu::{
        color::{AlphaColor, Srgb},
        kurbo::{BezPath, Cap, Join, Stroke as KurboStroke},
        peniko::{ColorStop, Fill, Gradient},
        Pixmap, RenderContext, Resources,
    };

    #[derive(Debug)]
    pub struct CpuOutput {
        pub width: u16,
        pub height: u16,
        pub pixels: Vec<u8>,
        pub diagnostics: Vec<RenderDiagnostic>,
    }

    pub fn render(scene: &PreparedScene) -> CpuOutput {
        let mut context = RenderContext::new(scene.width, scene.height);
        let mut resources = Resources::new();
        let mut diagnostics = scene.diagnostics.clone();
        for command in &scene.commands {
            if let Err(message) = apply(&mut context, command) {
                diagnostics.push(RenderDiagnostic {
                    severity: "error".to_string(),
                    code: "G500".to_string(),
                    message,
                    declaration: None,
                });
            }
        }
        context.flush();
        let mut pixmap = Pixmap::new(scene.width, scene.height);
        context.render(&mut pixmap, &mut resources);
        CpuOutput {
            width: scene.width,
            height: scene.height,
            pixels: pixmap.data_as_u8_slice().to_vec(),
            diagnostics,
        }
    }

    fn apply(context: &mut RenderContext, command: &Command) -> Result<(), String> {
        match command {
            Command::PushClip { path, rule } => {
                context.set_fill_rule(to_fill(*rule));
                context.push_clip_path(&parse_path(path)?);
            }
            Command::PopClip => context.pop_clip_path(),
            Command::Fill { path, paint, rule } => {
                context.set_fill_rule(to_fill(*rule));
                set_paint(context, paint);
                context.fill_path(&parse_path(path)?);
            }
            Command::Stroke { path, paint, style } => {
                set_paint(context, paint);
                let stroke = KurboStroke::new(style.width)
                    .with_miter_limit(style.miter_limit)
                    .with_caps(cap(style.line_cap))
                    .with_join(join(style.line_join))
                    .with_dashes(style.dash_offset, style.dash.iter());
                context.set_stroke(stroke);
                context.stroke_path(&parse_path(path)?);
            }
        }
        Ok(())
    }

    fn to_fill(rule: FillRule) -> Fill {
        match rule {
            FillRule::NonZero => Fill::NonZero,
            FillRule::EvenOdd => Fill::EvenOdd,
        }
    }

    fn cap(value: LineCap) -> Cap {
        match value {
            LineCap::Butt => Cap::Butt,
            LineCap::Round => Cap::Round,
            LineCap::Square => Cap::Square,
        }
    }

    fn join(value: LineJoin) -> Join {
        match value {
            LineJoin::Miter => Join::Miter,
            LineJoin::Round => Join::Round,
            LineJoin::Bevel => Join::Bevel,
        }
    }

    fn parse_path(path: &PathData) -> Result<BezPath, String> {
        BezPath::from_svg(&path.svg).map_err(|error| format!("invalid path: {error:?}"))
    }

    fn color(value: Rgba) -> AlphaColor<Srgb> {
        AlphaColor::<Srgb>::from_rgba8(value.r, value.g, value.b, value.a)
    }

    fn set_paint(context: &mut RenderContext, paint: &Paint) {
        match paint {
            Paint::Solid(value) => context.set_paint(color(*value)),
            Paint::LinearGradient { start, end, stops } => {
                let stops = stops
                    .iter()
                    .map(|stop| ColorStop {
                        offset: stop.offset,
                        color: color(stop.color),
                    })
                    .collect::<Vec<_>>();
                context.set_paint(Gradient::new_linear(*start, *end).with_stops(stops));
            }
        }
    }
}

#[cfg(feature = "gpu")]
pub mod gpu {
    use super::*;
    use vello_hybrid::{
        color::{AlphaColor, Srgb},
        kurbo::{BezPath, Cap, Join, Stroke as KurboStroke},
        peniko::{ColorStop, Fill, Gradient},
        Scene as HybridScene,
    };

    pub fn build_scene(scene: &PreparedScene) -> Result<HybridScene, Vec<RenderDiagnostic>> {
        let mut output = HybridScene::new(scene.width, scene.height);
        let mut diagnostics = scene.diagnostics.clone();
        for command in &scene.commands {
            if let Err(message) = apply(&mut output, command) {
                diagnostics.push(RenderDiagnostic {
                    severity: "error".to_string(),
                    code: "G600".to_string(),
                    message,
                    declaration: None,
                });
            }
        }
        if diagnostics.iter().any(|diagnostic| diagnostic.severity == "error") {
            Err(diagnostics)
        } else {
            Ok(output)
        }
    }

    fn apply(context: &mut HybridScene, command: &Command) -> Result<(), String> {
        match command {
            Command::PushClip { path, rule } => {
                context.set_fill_rule(to_fill(*rule));
                context.push_clip_path(&parse_path(path)?);
            }
            Command::PopClip => context.pop_clip_path(),
            Command::Fill { path, paint, rule } => {
                context.set_fill_rule(to_fill(*rule));
                set_paint(context, paint);
                context.fill_path(&parse_path(path)?);
            }
            Command::Stroke { path, paint, style } => {
                set_paint(context, paint);
                let stroke = KurboStroke::new(style.width)
                    .with_miter_limit(style.miter_limit)
                    .with_caps(cap(style.line_cap))
                    .with_join(join(style.line_join))
                    .with_dashes(style.dash_offset, style.dash.iter());
                context.set_stroke(stroke);
                context.stroke_path(&parse_path(path)?);
            }
        }
        Ok(())
    }

    fn to_fill(rule: FillRule) -> Fill {
        match rule {
            FillRule::NonZero => Fill::NonZero,
            FillRule::EvenOdd => Fill::EvenOdd,
        }
    }

    fn cap(value: LineCap) -> Cap {
        match value {
            LineCap::Butt => Cap::Butt,
            LineCap::Round => Cap::Round,
            LineCap::Square => Cap::Square,
        }
    }

    fn join(value: LineJoin) -> Join {
        match value {
            LineJoin::Miter => Join::Miter,
            LineJoin::Round => Join::Round,
            LineJoin::Bevel => Join::Bevel,
        }
    }

    fn parse_path(path: &PathData) -> Result<BezPath, String> {
        BezPath::from_svg(&path.svg).map_err(|error| format!("invalid path: {error:?}"))
    }

    fn color(value: Rgba) -> AlphaColor<Srgb> {
        AlphaColor::<Srgb>::from_rgba8(value.r, value.g, value.b, value.a)
    }

    fn set_paint(context: &mut HybridScene, paint: &Paint) {
        match paint {
            Paint::Solid(value) => context.set_paint(color(*value)),
            Paint::LinearGradient { start, end, stops } => {
                let stops = stops
                    .iter()
                    .map(|stop| ColorStop {
                        offset: stop.offset,
                        color: color(stop.color),
                    })
                    .collect::<Vec<_>>();
                context.set_paint(Gradient::new_linear(*start, *end).with_stops(stops));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use srng::runtime::{execute_json, RuntimeOptions};

    fn prepared(source: &str) -> PreparedScene {
        let ir = srng::compile_to_json(source, "renderer-test.srng");
        let scene = execute_json(&ir, &RuntimeOptions::default()).unwrap();
        let gate = RevisionGate::default();
        let revision = gate.begin();
        prepare_scene(&scene, revision, &gate)
    }

    #[test]
    fn preparation_preserves_paint_order() {
        let scene = prepared(
            "srng 0.1; rect a { position: 0px 0px; size: 10px 10px; fill: #000; } rect b { position: 1px 1px; size: 5px 5px; fill: #fff; }",
        );
        assert_eq!(scene.commands.len(), 2);
    }

    #[test]
    fn none_paint_is_not_an_error() {
        let scene = prepared(
            "srng 0.1; rect a { position: 0px 0px; size: 10px 10px; fill: none; stroke: #fff; }",
        );
        assert_eq!(scene.commands.len(), 1);
        assert!(scene.diagnostics.is_empty());
    }

    #[test]
    fn stale_revision_stops_preparation() {
        let ir = srng::compile_to_json(
            "srng 0.1; rect a { position: 0px 0px; size: 10px 10px; fill: #fff; }",
            "renderer-test.srng",
        );
        let scene = execute_json(&ir, &RuntimeOptions::default()).unwrap();
        let gate = RevisionGate::default();
        let stale = gate.begin();
        gate.begin();
        let prepared = prepare_scene(&scene, stale, &gate);
        assert!(prepared.commands.is_empty());
    }
}
