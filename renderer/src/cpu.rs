use crate::{Command, FillRule, LineCap, LineJoin, Paint, PreparedScene, RenderDiagnostic, Rgba};
use vello_cpu::{
    color::{AlphaColor, Srgb},
    kurbo::{BezPath, Cap, Join, Stroke as KurboStroke},
    peniko::{ColorStop, ColorStops, Fill, Gradient},
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
            context.push_clip_path(&parse_path(&path.svg)?);
        }
        Command::PopClip => context.pop_clip_path(),
        Command::Fill { path, paint, rule } => {
            context.set_fill_rule(to_fill(*rule));
            set_paint(context, paint);
            context.fill_path(&parse_path(&path.svg)?);
        }
        Command::Stroke { path, paint, style } => {
            set_paint(context, paint);
            let stroke = KurboStroke::new(style.width)
                .with_miter_limit(style.miter_limit)
                .with_caps(cap(style.line_cap))
                .with_join(join(style.line_join))
                .with_dashes(style.dash_offset, style.dash.iter());
            context.set_stroke(stroke);
            context.stroke_path(&parse_path(&path.svg)?);
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

fn parse_path(value: &str) -> Result<BezPath, String> {
    BezPath::from_svg(value).map_err(|error| format!("invalid path: {error:?}"))
}

fn color(value: Rgba) -> AlphaColor<Srgb> {
    AlphaColor::<Srgb>::from_rgba8(value.r, value.g, value.b, value.a)
}

fn set_paint(context: &mut RenderContext, paint: &Paint) {
    match paint {
        Paint::Solid(value) => context.set_paint(color(*value)),
        Paint::LinearGradient { start, end, stops } => {
            let stops = ColorStops(
                stops
                    .iter()
                    .map(|stop| ColorStop {
                        offset: stop.offset,
                        color: color(stop.color).into(),
                    })
                    .collect(),
            );
            context.set_paint(Gradient::new_linear(*start, *end).with_stops(stops));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{prepare_scene, RevisionGate};
    use srng::runtime::{execute_json, RuntimeOptions};

    #[test]
    fn renders_non_empty_rgba_pixmap() {
        let ir = srng::compile_to_json(
            "srng 0.1; rect box { position: 0px 0px; size: 8px 8px; fill: #ff0000; }",
            "cpu-test.srng",
        );
        let mut options = RuntimeOptions::default();
        options.viewport_width = 8.0;
        options.viewport_height = 8.0;
        let scene = execute_json(&ir, &options).unwrap();
        let gate = RevisionGate::default();
        let revision = gate.begin();
        let prepared = prepare_scene(&scene, revision, &gate);
        let output = render(&prepared);
        assert_eq!(output.pixels.len(), 8 * 8 * 4);
        assert!(output.pixels.iter().any(|byte| *byte != 0));
        assert!(!output.diagnostics.iter().any(|d| d.severity == "error"));
    }
}
