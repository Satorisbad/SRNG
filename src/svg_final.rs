pub use crate::svg_v4::{ImportDiagnostic, ImportOptions, ImportResult};
pub use crate::svg_v4::strip_svg_provenance;

/// Final public SVG facade for the v0.4 fidelity milestone.
///
/// `svg_v4` promotes source-attribute semantics. This last normalization pass
/// also catches presentation values originating from inline `style`, which the
/// lower importer records as `svg-opacity` compatibility fields rather than
/// `svg-attr-*` fields.
pub fn import_svg(svg: &str, source_name: &str, options: &ImportOptions) -> ImportResult {
    let mut result = crate::svg_v4::import_svg(svg, source_name, options);
    promote_style_semantics(&mut result.source);
    let has_opacity = result.source.lines().any(|line| {
        let line = line.trim_start();
        line.starts_with("opacity:") || line.starts_with("fill-opacity:") || line.starts_with("stroke-opacity:")
    });
    if has_opacity {
        for diagnostic in &mut result.diagnostics {
            if diagnostic.code == "S131" && diagnostic.message.contains("opacity") {
                diagnostic.severity = "info".into();
                diagnostic.message = "opacity is represented by native SRNG opacity semantics".into();
            }
        }
    }
    result
}

fn promote_style_semantics(source: &mut String) {
    const ALIASES: &[(&str, &str)] = &[
        ("svg-opacity:", "opacity:"),
        ("svg-fill-opacity:", "fill-opacity:"),
        ("svg-stroke-opacity:", "stroke-opacity:"),
    ];
    let mut out = String::with_capacity(source.len() + source.len() / 16);
    for line in source.lines() {
        out.push_str(line);
        out.push('\n');
        let trimmed = line.trim_start();
        let indent = &line[..line.len() - trimmed.len()];
        for (from, to) in ALIASES {
            if let Some(rest) = trimmed.strip_prefix(from) {
                out.push_str(indent);
                out.push_str(to);
                out.push_str(rest);
                out.push('\n');
                break;
            }
        }
    }
    *source = out;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inline_style_opacity_becomes_native() {
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><rect width="10" height="10" style="fill:#ff0000;opacity:0.5;fill-opacity:0.5"/></svg>"##;
        let result = import_svg(svg, "style.svg", &ImportOptions::default());
        assert!(result.source.contains("opacity:"));
        assert!(result.source.contains("fill-opacity:"));
    }
}
