pub use crate::svg_v4::{ImportDiagnostic, ImportOptions, ImportResult};
pub use crate::svg_v4::strip_svg_provenance;

/// Final public SVG facade for the v0.4 fidelity milestone.
///
/// `svg_v4` promotes source-attribute semantics. This last normalization pass
/// also catches presentation values originating from inline `style`, which the
/// lower importer records as `svg-opacity` compatibility fields rather than
/// `svg-attr-*` fields. Reusable resources are then promoted into native SRNG
/// declarations without reconstructing SVG XML or flattening group contents.
pub fn import_svg(svg: &str, source_name: &str, options: &ImportOptions) -> ImportResult {
    let mut result = crate::svg_v4::import_svg(svg, source_name, options);
    promote_style_semantics(&mut result.source);
    crate::resource_reference::promote_reusable_resources(svg, &mut result);
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

    #[test]
    fn referenced_group_is_materialized_as_resource_subtree() {
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg"><defs><g id="badge"><rect id="back" width="20" height="10" fill="#f00"/><circle id="dot" cx="5" cy="5" r="2" fill="#00f"/></g></defs><use id="badge_instance" href="#badge" x="30" y="40"/></svg>"##;
        let result = import_svg(svg, "group.svg", &ImportOptions::default());
        assert!(result.source.contains("group badge {"));
        assert!(result.source.contains("rect back {"));
        assert!(result.source.contains("circle dot {"));
        assert!(result.source.contains("relation badge -> back"));
        assert!(result.source.contains("relation badge -> dot"));
        assert!(result.source.contains("reference badge_instance = \"#badge\""));
    }
}