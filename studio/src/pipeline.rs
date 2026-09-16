use srng::runtime::{execute_json, RuntimeOptions, Scene};
use srng::svg::{import_svg, ImportOptions};
use srng_renderer::{cpu, prepare_scene, RevisionGate};
use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct RgbaImage {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct StudioDiagnostic {
    pub severity: String,
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct ConversionOutcome {
    pub srng: String,
    pub diagnostics: Vec<StudioDiagnostic>,
    pub original: Option<RgbaImage>,
    pub rendered: Option<RgbaImage>,
}

impl ConversionOutcome {
    pub fn has_errors(&self) -> bool {
        self.diagnostics.iter().any(|d| d.severity == "error")
    }
}

pub fn convert_svg(svg: &str, source_name: &str) -> ConversionOutcome {
    let mut diagnostics = Vec::new();
    let original = match rasterize_svg(svg) {
        Ok(image) => Some(image),
        Err(message) => {
            diagnostics.push(diag("error", "UI100", message));
            None
        }
    };

    let imported = import_svg(svg, source_name, &ImportOptions::default());
    diagnostics.extend(imported.diagnostics.iter().map(|d| StudioDiagnostic {
        severity: d.severity.clone(),
        code: d.code.clone(),
        message: d.message.clone(),
    }));

    let srng = imported.source;
    let rendered = if diagnostics.iter().any(|d| d.severity == "error") {
        None
    } else {
        render_srng_internal(&srng, source_name, &mut diagnostics)
    };

    ConversionOutcome {
        srng,
        diagnostics,
        original,
        rendered,
    }
}

pub fn render_srng(source: &str, source_name: &str) -> (Option<RgbaImage>, Vec<StudioDiagnostic>) {
    let mut diagnostics = Vec::new();
    let image = render_srng_internal(source, source_name, &mut diagnostics);
    (image, diagnostics)
}

fn render_srng_internal(
    source: &str,
    source_name: &str,
    diagnostics: &mut Vec<StudioDiagnostic>,
) -> Option<RgbaImage> {
    let ir = srng::compile_to_json(source, source_name);
    let mut options = RuntimeOptions::default();
    let mut scene = match execute_json(&ir, &options) {
        Ok(scene) => scene,
        Err(error) => {
            diagnostics.push(diag("error", "UI200", error.to_string()));
            return None;
        }
    };

    if let Some((width, height)) = canvas_size(&scene) {
        if width > 0.0 && height > 0.0 {
            options.viewport_width = width;
            options.viewport_height = height;
            match execute_json(&ir, &options) {
                Ok(updated) => scene = updated,
                Err(error) => {
                    diagnostics.push(diag("error", "UI201", error.to_string()));
                    return None;
                }
            }
        }
    }

    diagnostics.extend(scene.diagnostics.iter().map(|d| StudioDiagnostic {
        severity: d.severity.clone(),
        code: d.code.clone(),
        message: d.message.clone(),
    }));
    if scene.has_errors() {
        return None;
    }

    let gate = RevisionGate::default();
    let revision = gate.begin();
    let prepared = prepare_scene(&scene, revision, &gate);
    let output = cpu::render(&prepared);
    diagnostics.extend(output.diagnostics.iter().map(|d| StudioDiagnostic {
        severity: d.severity.clone(),
        code: d.code.clone(),
        message: d.message.clone(),
    }));
    if output.diagnostics.iter().any(|d| d.severity == "error") {
        return None;
    }

    Some(RgbaImage {
        width: output.width as usize,
        height: output.height as usize,
        pixels: output.pixels,
    })
}

fn canvas_size(scene: &Scene) -> Option<(f64, f64)> {
    scene
        .nodes
        .iter()
        .find(|node| node.active && node.kind == "canvas")
        .and_then(|node| Some((node.geometry.width?, node.geometry.height?)))
}

pub fn rasterize_svg(svg: &str) -> Result<RgbaImage, String> {
    use resvg::{tiny_skia, usvg};

    let options = usvg::Options::default();
    let tree = usvg::Tree::from_str(svg, &options).map_err(|error| error.to_string())?;
    let size = tree.size();
    if size.width() <= 0.0 || size.height() <= 0.0 {
        return Err("SVG has an empty viewport".to_string());
    }

    const MAX_SIDE: f32 = 1600.0;
    let scale = (MAX_SIDE / size.width())
        .min(MAX_SIDE / size.height())
        .min(1.0)
        .max(0.001);
    let width = (size.width() * scale).ceil().max(1.0) as u32;
    let height = (size.height() * scale).ceil().max(1.0) as u32;
    let mut pixmap = tiny_skia::Pixmap::new(width, height)
        .ok_or_else(|| "could not allocate SVG preview pixmap".to_string())?;
    let transform = tiny_skia::Transform::from_scale(scale, scale);
    resvg::render(&tree, transform, &mut pixmap.as_mut());

    Ok(RgbaImage {
        width: width as usize,
        height: height as usize,
        pixels: unpremultiply_rgba(pixmap.data()),
    })
}

fn unpremultiply_rgba(source: &[u8]) -> Vec<u8> {
    let mut pixels = source.to_vec();
    for rgba in pixels.chunks_exact_mut(4) {
        let a = rgba[3] as u32;
        if a == 0 || a == 255 {
            continue;
        }
        for channel in &mut rgba[..3] {
            *channel = (((*channel as u32) * 255 + a / 2) / a).min(255) as u8;
        }
    }
    pixels
}

pub fn write_png(path: &Path, image: &RgbaImage) -> Result<(), String> {
    let expected = image.width * image.height * 4;
    if image.pixels.len() != expected {
        return Err(format!(
            "image has {} bytes, expected {expected}",
            image.pixels.len()
        ));
    }

    let file = File::create(path).map_err(|error| error.to_string())?;
    let writer = BufWriter::new(file);
    let mut encoder = png::Encoder::new(writer, image.width as u32, image.height as u32);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(|error| error.to_string())?;
    writer
        .write_image_data(&image.pixels)
        .map_err(|error| error.to_string())
}

fn diag(severity: &str, code: &str, message: impl Into<String>) -> StudioDiagnostic {
    StudioDiagnostic {
        severity: severity.to_string(),
        code: code.to_string(),
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn alpha_at(image: &RgbaImage, x: usize, y: usize) -> u8 {
        image.pixels[(y * image.width + x) * 4 + 3]
    }

    fn rgba_at(image: &RgbaImage, x: usize, y: usize) -> &[u8] {
        let offset = (y * image.width + x) * 4;
        &image.pixels[offset..offset + 4]
    }

    #[test]
    fn svg_runs_through_importer_and_renderer() {
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="64" height="48"><rect id="box" x="4" y="5" width="40" height="30" fill="#ff0000"/></svg>"##;
        let outcome = convert_svg(svg, "test.svg");
        assert!(!outcome.has_errors(), "{:?}", outcome.diagnostics);
        assert!(outcome.srng.contains("rect box"));
        assert!(outcome.original.as_ref().is_some_and(|image| !image.pixels.is_empty()));
        assert!(outcome.rendered.as_ref().is_some_and(|image| {
            image.width == 64 && image.height == 48 && image.pixels.iter().any(|b| *b != 0)
        }));
    }

    #[test]
    fn rounded_rect_changes_corner_coverage() {
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="40"><rect x="4" y="4" width="32" height="32" rx="10" fill="#ff0000"/></svg>"##;
        let outcome = convert_svg(svg, "rounded.svg");
        assert!(!outcome.has_errors(), "{:?}", outcome.diagnostics);
        let image = outcome.rendered.unwrap();
        assert_eq!(alpha_at(&image, 4, 4), 0);
        assert!(alpha_at(&image, 20, 20) > 0);
        assert!(!outcome.diagnostics.iter().any(|d| d.code == "S210"));
    }

    #[test]
    fn clip_path_limits_rendered_pixels() {
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="40"><defs><clipPath id="cut"><rect x="10" y="10" width="20" height="20"/></clipPath></defs><rect width="40" height="40" fill="#ff0000" clip-path="url(#cut)"/></svg>"##;
        let outcome = convert_svg(svg, "clip.svg");
        assert!(!outcome.has_errors(), "{:?}", outcome.diagnostics);
        let image = outcome.rendered.unwrap();
        assert_eq!(alpha_at(&image, 5, 5), 0);
        assert!(alpha_at(&image, 20, 20) > 0);
        assert_eq!(alpha_at(&image, 35, 35), 0);
    }

    #[test]
    fn simple_white_mask_behaves_as_binary_clip() {
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="40"><defs><mask id="m"><circle cx="20" cy="20" r="10" fill="white"/></mask></defs><rect width="40" height="40" fill="#00ff00" mask="url(#m)"/></svg>"##;
        let outcome = convert_svg(svg, "mask.svg");
        assert!(!outcome.has_errors(), "{:?}", outcome.diagnostics);
        let image = outcome.rendered.unwrap();
        assert_eq!(alpha_at(&image, 2, 2), 0);
        assert!(alpha_at(&image, 20, 20) > 0);
        assert!(!outcome.diagnostics.iter().any(|d| d.code == "S242"));
    }

    #[test]
    fn svg_pattern_fill_repeats_through_full_pipeline() {
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="12" height="4"><defs><pattern id="p" patternUnits="userSpaceOnUse" width="4" height="4"><rect width="2" height="4" fill="#ff0000"/><rect x="2" width="2" height="4" fill="#0000ff"/></pattern></defs><rect id="box" width="12" height="4" fill="url(#p)"/></svg>"##;
        let outcome = convert_svg(svg, "pattern.svg");
        assert!(!outcome.has_errors(), "{:?}", outcome.diagnostics);
        assert!(!outcome.diagnostics.iter().any(|d| d.code == "S232" && d.severity == "warning"));
        let image = outcome.rendered.expect("pattern should render");
        let a = rgba_at(&image, 0, 1);
        let b = rgba_at(&image, 2, 1);
        let c = rgba_at(&image, 4, 1);
        assert!(a[0] > a[2], "first half of tile should be red: {a:?}");
        assert!(b[2] > b[0], "second half of tile should be blue: {b:?}");
        assert!(c[0] > c[2], "pattern should repeat: {c:?}");
    }

    #[test]
    fn object_bounding_box_pattern_scales_to_target_bounds() {
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="12" height="4"><defs><pattern id="p" patternContentUnits="objectBoundingBox" width="1" height="1"><rect width="0.5" height="1" fill="#ff0000"/><rect x="0.5" width="0.5" height="1" fill="#0000ff"/></pattern></defs><rect id="box" width="12" height="4" fill="url(#p)"/></svg>"##;
        let outcome = convert_svg(svg, "bbox-pattern.svg");
        assert!(!outcome.has_errors(), "{:?}", outcome.diagnostics);
        let image = outcome.rendered.expect("objectBoundingBox pattern should render");
        let left = rgba_at(&image, 2, 1);
        let right = rgba_at(&image, 9, 1);
        assert!(left[0] > left[2], "left half should be red: {left:?}");
        assert!(right[2] > right[0], "right half should be blue: {right:?}");
    }

    #[test]
    fn figma_style_embedded_image_pattern_survives_defs_resolution() {
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="8" height="4"><defs><pattern id="p" patternContentUnits="objectBoundingBox" width="1" height="1"><use xlink:href="#img" transform="scale(0.5 1)"/></pattern><image id="img" width="2" height="1" xlink:href="data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAIAAAABCAYAAAD0In+KAAAADklEQVR4nGP4z8AAQv8BD/kD/YURmXYAAAAASUVORK5CYII="/></defs><rect id="box" width="8" height="4" fill="url(#p)"/></svg>"##;
        let outcome = convert_svg(svg, "embedded-pattern.svg");
        assert!(!outcome.has_errors(), "{:?}", outcome.diagnostics);
        assert!(!outcome.diagnostics.iter().any(|d| d.code == "S232" && d.severity == "warning"));
        let image = outcome.rendered.expect("embedded image pattern should render");
        assert!(image.pixels.iter().any(|b| *b != 0));
    }

    #[test]
    fn complex_svg_mask_is_applied_by_cpu_renderer() {
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="8" height="8"><defs><mask id="m"><rect width="4" height="8" fill="#808080"/></mask></defs><rect id="box" width="8" height="8" fill="#ff0000" mask="url(#m)"/></svg>"##;
        let outcome = convert_svg(svg, "mask-complex.svg");
        assert!(!outcome.has_errors(), "{:?}", outcome.diagnostics);
        assert!(!outcome.diagnostics.iter().any(|d| d.code == "S242" && d.severity == "warning"));
        let image = outcome.rendered.expect("complex mask should render");
        let inside = rgba_at(&image, 2, 2);
        let outside = rgba_at(&image, 6, 2);
        assert!(inside[3] > 20, "masked area should retain alpha: {inside:?}");
        assert!(outside[3] < inside[3], "outside mask should be more transparent: {outside:?}");
    }

    #[test]
    fn png_writer_emits_png_signature() {
        let image = RgbaImage {
            width: 1,
            height: 1,
            pixels: vec![255, 0, 0, 255],
        };
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("srng-studio-{stamp}.png"));
        write_png(&path, &image).unwrap();
        let bytes = fs::read(&path).unwrap();
        let _ = fs::remove_file(&path);
        assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
    }
}
