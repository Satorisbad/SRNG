pub mod pipeline;

pub use pipeline::{rasterize_svg, write_png, ConversionOutcome, RgbaImage, StudioDiagnostic};

const MAX_SOURCE_BYTES: usize = 32 * 1024 * 1024;
const MAX_RENDER_SIDE: f64 = 8192.0;
const MAX_RENDER_PIXELS: f64 = 16_777_216.0;

pub fn convert_svg(svg: &str, source_name: &str) -> ConversionOutcome {
    if svg.len() > MAX_SOURCE_BYTES {
        return ConversionOutcome {
            srng: String::new(),
            diagnostics: vec![diagnostic(
                "error",
                "UI300",
                format!(
                    "source is {} bytes; Studio limit is {} bytes",
                    svg.len(),
                    MAX_SOURCE_BYTES
                ),
            )],
            original: None,
            rendered: None,
        };
    }

    use srng::svg::{import_svg, ImportOptions};

    let mut diagnostics = Vec::new();
    let original = match pipeline::rasterize_svg(svg) {
        Ok(image) => Some(image),
        Err(message) => {
            diagnostics.push(diagnostic("error", "UI100", message));
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
        let (image, mut render_diagnostics) = render_srng(&srng, source_name);
        diagnostics.append(&mut render_diagnostics);
        image
    };

    ConversionOutcome {
        srng,
        diagnostics,
        original,
        rendered,
    }
}

pub fn render_srng(source: &str, source_name: &str) -> (Option<RgbaImage>, Vec<StudioDiagnostic>) {
    if source.len() > MAX_SOURCE_BYTES {
        return (
            None,
            vec![diagnostic(
                "error",
                "UI300",
                format!(
                    "source is {} bytes; Studio limit is {} bytes",
                    source.len(),
                    MAX_SOURCE_BYTES
                ),
            )],
        );
    }

    if let Some(error) = render_budget_error(source, source_name) {
        return (None, vec![error]);
    }

    pipeline::render_srng(source, source_name)
}

fn render_budget_error(source: &str, source_name: &str) -> Option<StudioDiagnostic> {
    use srng::runtime::{execute_json, RuntimeOptions};

    let ir = srng::compile_to_json(source, source_name);
    let scene = execute_json(&ir, &RuntimeOptions::default()).ok()?;
    let (width, height) = scene
        .nodes
        .iter()
        .find(|node| node.active && node.kind == "canvas")
        .and_then(|node| Some((node.geometry.width?, node.geometry.height?)))
        .unwrap_or((scene.viewport.width, scene.viewport.height));

    if !width.is_finite() || !height.is_finite() || width <= 0.0 || height <= 0.0 {
        return None;
    }

    let pixels = width.ceil() * height.ceil();
    if width > MAX_RENDER_SIDE || height > MAX_RENDER_SIDE || pixels > MAX_RENDER_PIXELS {
        return Some(diagnostic(
            "error",
            "UI301",
            format!(
                "render target {:.0}x{:.0} exceeds Studio safety budget (max side {:.0}px, max {:.0} pixels)",
                width, height, MAX_RENDER_SIDE, MAX_RENDER_PIXELS
            ),
        ));
    }

    None
}

fn diagnostic(severity: &str, code: &str, message: impl Into<String>) -> StudioDiagnostic {
    StudioDiagnostic {
        severity: severity.to_string(),
        code: code.to_string(),
        message: message.into(),
    }
}

#[cfg(test)]
mod limits_tests {
    use super::*;
    use srng::svg::{import_svg, ImportOptions};

    fn imported_srng(svg: &str, name: &str) -> String {
        let imported = import_svg(svg, name, &ImportOptions::default());
        assert!(
            !imported.diagnostics.iter().any(|d| d.severity == "error"),
            "{:?}",
            imported.diagnostics
        );
        imported.source
    }

    #[test]
    fn rejects_excessive_canvas_before_raster_allocation() {
        let source = imported_srng(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="50000" height="50000"><rect width="10" height="10" fill="#fff"/></svg>"#,
            "huge.svg",
        );
        let (image, diagnostics) = render_srng(&source, "huge.srng");
        assert!(image.is_none());
        assert!(
            diagnostics.iter().any(|d| d.code == "UI301"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn accepts_normal_render_budget() {
        let source = imported_srng(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="640" height="480"><rect width="640" height="480" fill="#fff"/></svg>"#,
            "normal.svg",
        );
        let (image, diagnostics) = render_srng(&source, "normal.srng");
        assert!(
            !diagnostics.iter().any(|d| d.code == "UI301"),
            "{diagnostics:?}"
        );
        assert!(image.is_some(), "{diagnostics:?}");
    }
}
