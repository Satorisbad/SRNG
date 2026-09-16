use crate::{Command, FillRule, LineCap, LineJoin, Paint, PreparedScene, RenderDiagnostic, Rgba};
use vello_hybrid::{
    color::{AlphaColor, Srgb},
    kurbo::{BezPath, Cap, Join, Stroke as KurboStroke},
    peniko::{ColorStop, ColorStops, Fill, Gradient},
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

fn set_paint(context: &mut HybridScene, paint: &Paint) {
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
