use crate::{Command, FillRule, LineCap, LineJoin, Paint, PreparedScene, RenderDiagnostic, Rgba};
use std::sync::Arc;
use vello_cpu::{
    color::{AlphaColor, Srgb},
    kurbo::{Affine, BezPath, Cap, Join, Stroke as KurboStroke},
    peniko::{ColorStop, ColorStops, Extend, Fill, Gradient, ImageSampler},
    Image, ImageSource, Pixmap, RenderContext, Resources,
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
    let mut diagnostics = scene.diagnostics.clone();
    if let Err(message) = render_commands(&mut context, &scene.commands, scene.width, scene.height) {
        diagnostics.push(RenderDiagnostic {
            severity: "error".to_string(),
            code: "G500".to_string(),
            message,
            declaration: None,
        });
    }

    context.flush();
    let mut resources = Resources::new();
    let mut pixmap = Pixmap::new(scene.width, scene.height);
    context.render(&mut pixmap, &mut resources);
    CpuOutput {
        width: scene.width,
        height: scene.height,
        pixels: pixmap.data_as_u8_slice().to_vec(),
        diagnostics,
    }
}

fn render_commands(
    context: &mut RenderContext,
    commands: &[Command],
    width: u16,
    height: u16,
) -> Result<(), String> {
    let mut index = 0;
    while index < commands.len() {
        match &commands[index] {
            Command::PushMaskSvg { svg } => {
                let end = matching_mask_end(commands, index)?;
                let mut layer_context = RenderContext::new(width, height);
                render_commands(&mut layer_context, &commands[index + 1..end], width, height)?;
                layer_context.flush();
                let mut layer_resources = Resources::new();
                let mut layer = Pixmap::new(width, height);
                layer_context.render(&mut layer, &mut layer_resources);

                let mask = rasterize_svg_exact(svg, width, height)?;
                apply_alpha_mask(&mut layer, &mask)?;
                composite_pixmap(context, layer, width, height)?;
                index = end + 1;
            }
            Command::PopMask => return Err("encountered unmatched mask terminator".to_string()),
            command => {
                apply_simple(context, command)?;
                index += 1;
            }
        }
    }
    Ok(())
}

fn matching_mask_end(commands: &[Command], start: usize) -> Result<usize, String> {
    let mut depth = 0usize;
    for (index, command) in commands.iter().enumerate().skip(start) {
        match command {
            Command::PushMaskSvg { .. } => depth += 1,
            Command::PopMask => {
                if depth == 0 {
                    return Err("encountered unmatched mask terminator".to_string());
                }
                depth -= 1;
                if depth == 0 {
                    return Ok(index);
                }
            }
            _ => {}
        }
    }
    Err("mask block is missing its terminator".to_string())
}

fn apply_simple(context: &mut RenderContext, command: &Command) -> Result<(), String> {
    match command {
        Command::PushClip { path, rule } => {
            context.set_fill_rule(to_fill(*rule));
            context.push_clip_path(&parse_path(&path.svg)?);
        }
        Command::PopClip => context.pop_clip_path(),
        Command::PushMaskSvg { .. } | Command::PopMask => {
            return Err("mask commands must be handled as a block".to_string())
        }
        Command::Fill { path, paint, rule } => {
            context.set_fill_rule(to_fill(*rule));
            set_paint(context, paint)?;
            context.fill_path(&parse_path(&path.svg)?);
        }
        Command::Stroke { path, paint, style } => {
            set_paint(context, paint)?;
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

fn apply_alpha_mask(layer: &mut Pixmap, mask: &Pixmap) -> Result<(), String> {
    let mask_bytes = mask.data_as_u8_slice();
    let layer_bytes = layer.data_as_u8_slice_mut();
    if mask_bytes.len() != layer_bytes.len() {
        return Err("SVG mask raster dimensions do not match the isolated layer".to_string());
    }
    for (pixel, mask_pixel) in layer_bytes.chunks_exact_mut(4).zip(mask_bytes.chunks_exact(4)) {
        let alpha = u16::from(mask_pixel[3]);
        for channel in pixel {
            *channel = ((u16::from(*channel) * alpha + 127) / 255) as u8;
        }
    }
    layer.recompute_may_have_transparency();
    Ok(())
}

fn composite_pixmap(context: &mut RenderContext, pixmap: Pixmap, width: u16, height: u16) -> Result<(), String> {
    let image = Image {
        image: ImageSource::Pixmap(Arc::new(pixmap)),
        sampler: ImageSampler::default(),
    };
    context.reset_paint_transform();
    context.set_paint(image);
    context.set_fill_rule(Fill::NonZero);
    context.fill_path(&parse_path(&format!("M 0 0 H {width} V {height} H 0 Z"))?);
    Ok(())
}

fn to_fill(rule: FillRule) -> Fill {
    match rule { FillRule::NonZero => Fill::NonZero, FillRule::EvenOdd => Fill::EvenOdd }
}
fn cap(value: LineCap) -> Cap {
    match value { LineCap::Butt => Cap::Butt, LineCap::Round => Cap::Round, LineCap::Square => Cap::Square }
}
fn join(value: LineJoin) -> Join {
    match value { LineJoin::Miter => Join::Miter, LineJoin::Round => Join::Round, LineJoin::Bevel => Join::Bevel }
}
fn parse_path(value: &str) -> Result<BezPath, String> {
    BezPath::from_svg(value).map_err(|error| format!("invalid path: {error:?}"))
}
fn color(value: Rgba) -> AlphaColor<Srgb> {
    AlphaColor::<Srgb>::from_rgba8(value.r, value.g, value.b, value.a)
}

fn set_paint(context: &mut RenderContext, paint: &Paint) -> Result<(), String> {
    context.reset_paint_transform();
    match paint {
        Paint::Solid(value) => context.set_paint(color(*value)),
        Paint::LinearGradient { start, end, stops } => {
            let stops = ColorStops(stops.iter().map(|stop| ColorStop {
                offset: stop.offset,
                color: color(stop.color).into(),
            }).collect());
            context.set_paint(Gradient::new_linear(*start, *end).with_stops(stops));
        }
        Paint::RadialGradient { center, focal, radius, stops } => {
            let stops = ColorStops(stops.iter().map(|stop| ColorStop {
                offset: stop.offset,
                color: color(stop.color).into(),
            }).collect());
            context.set_paint(
                Gradient::new_two_point_radial(*focal, 0.0, *center, *radius as f32)
                    .with_stops(stops),
            );
        }
        Paint::SvgPattern { svg, tile_width, tile_height } => {
            let (pixmap, raster_width, raster_height) = rasterize_pattern(svg, *tile_width, *tile_height)?;
            let image = Image {
                image: ImageSource::Pixmap(Arc::new(pixmap)),
                sampler: ImageSampler { x_extend: Extend::Repeat, y_extend: Extend::Repeat, ..Default::default() },
            };
            context.set_paint(image);
            context.set_paint_transform(Affine::scale_non_uniform(
                *tile_width / f64::from(raster_width),
                *tile_height / f64::from(raster_height),
            ));
        }
    }
    Ok(())
}

fn svg_options(svg: &str) -> resvg::usvg::Options<'static> {
    let mut options = resvg::usvg::Options::default();
    if svg.contains("<text") || svg.contains("<tspan") {
        options.fontdb_mut().load_system_fonts();
    }
    options
}

fn rasterize_pattern(svg: &str, tile_width: f64, tile_height: f64) -> Result<(Pixmap, u16, u16), String> {
    use resvg::{tiny_skia, usvg};
    if !tile_width.is_finite() || !tile_height.is_finite() || tile_width <= 0.0 || tile_height <= 0.0 {
        return Err("SVG pattern tile dimensions must be positive finite numbers".to_string());
    }

    let options = svg_options(svg);
    let tree = usvg::Tree::from_str(svg, &options)
        .map_err(|error| format!("could not parse preserved SVG pattern: {error}"))?;
    let size = tree.size();
    if size.width() <= 0.0 || size.height() <= 0.0 {
        return Err("SVG pattern has an empty tile viewport".to_string());
    }

    const TARGET_SIDE: f32 = 256.0;
    const MAX_SIDE: f32 = 512.0;
    let longest = size.width().max(size.height()).max(0.001);
    let scale = (TARGET_SIDE / longest).max(1.0);
    let width = (size.width() * scale).ceil().clamp(1.0, MAX_SIDE) as u32;
    let height = (size.height() * scale).ceil().clamp(1.0, MAX_SIDE) as u32;
    let raster_width = u16::try_from(width).map_err(|_| "SVG pattern raster width is too large".to_string())?;
    let raster_height = u16::try_from(height).map_err(|_| "SVG pattern raster height is too large".to_string())?;

    let mut source = tiny_skia::Pixmap::new(width, height)
        .ok_or_else(|| "could not allocate SVG pattern tile".to_string())?;
    let sx = width as f32 / size.width();
    let sy = height as f32 / size.height();
    resvg::render(&tree, tiny_skia::Transform::from_scale(sx, sy), &mut source.as_mut());

    let mut pixmap = Pixmap::new(raster_width, raster_height);
    pixmap.data_as_u8_slice_mut().copy_from_slice(source.data());
    pixmap.recompute_may_have_transparency();
    Ok((pixmap, raster_width, raster_height))
}

fn rasterize_svg_exact(svg: &str, width: u16, height: u16) -> Result<Pixmap, String> {
    use resvg::{tiny_skia, usvg};
    let options = svg_options(svg);
    let tree = usvg::Tree::from_str(svg, &options)
        .map_err(|error| format!("could not parse preserved SVG mask: {error}"))?;
    let size = tree.size();
    if size.width() <= 0.0 || size.height() <= 0.0 {
        return Err("SVG mask has an empty viewport".to_string());
    }

    let mut source = tiny_skia::Pixmap::new(u32::from(width), u32::from(height))
        .ok_or_else(|| "could not allocate SVG mask pixmap".to_string())?;
    let sx = f32::from(width) / size.width();
    let sy = f32::from(height) / size.height();
    resvg::render(&tree, tiny_skia::Transform::from_scale(sx, sy), &mut source.as_mut());

    let mut pixmap = Pixmap::new(width, height);
    pixmap.data_as_u8_slice_mut().copy_from_slice(source.data());
    pixmap.recompute_may_have_transparency();
    Ok(pixmap)
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
        let output = render(&prepare_scene(&scene, revision, &gate));
        assert_eq!(output.pixels.len(), 8 * 8 * 4);
        assert!(output.pixels.iter().any(|byte| *byte != 0));
        assert!(!output.diagnostics.iter().any(|d| d.severity == "error"));
    }

    #[test]
    fn renders_native_radial_gradient() {
        let scene = PreparedScene {
            width: 16,
            height: 16,
            revision: 1,
            diagnostics: vec![],
            commands: vec![Command::Fill {
                path: crate::PathData { svg: "M 0 0 H 16 V 16 H 0 Z".to_string() },
                paint: Paint::RadialGradient {
                    center: (8.0, 8.0),
                    focal: (8.0, 8.0),
                    radius: 8.0,
                    stops: vec![
                        crate::GradientStop { offset: 0.0, color: Rgba { r: 255, g: 0, b: 0, a: 255 } },
                        crate::GradientStop { offset: 1.0, color: Rgba { r: 0, g: 0, b: 255, a: 255 } },
                    ],
                },
                rule: FillRule::NonZero,
            }],
        };
        let output = render(&scene);
        assert!(!output.diagnostics.iter().any(|d| d.severity == "error"), "{:?}", output.diagnostics);
        assert!(output.pixels.iter().any(|byte| *byte != 0));
    }

    #[test]
    fn renders_repeating_svg_pattern_paint() {
        let pattern_svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="4" height="4" viewBox="0 0 4 4"><rect width="2" height="4" fill="#ff0000"/><rect x="2" width="2" height="4" fill="#0000ff"/></svg>"##;
        let scene = PreparedScene {
            width: 12,
            height: 4,
            revision: 1,
            diagnostics: vec![],
            commands: vec![Command::Fill {
                path: crate::PathData { svg: "M 0 0 H 12 V 4 H 0 Z".to_string() },
                paint: Paint::SvgPattern { svg: pattern_svg.to_string(), tile_width: 4.0, tile_height: 4.0 },
                rule: FillRule::NonZero,
            }],
        };
        let output = render(&scene);
        assert!(!output.diagnostics.iter().any(|d| d.severity == "error"), "{:?}", output.diagnostics);
        let pixel = |x: usize| &output.pixels[x * 4..x * 4 + 4];
        assert!(pixel(0)[0] > pixel(0)[2]);
        assert!(pixel(2)[2] > pixel(2)[0]);
        assert!(pixel(4)[0] > pixel(4)[2]);
    }
}
