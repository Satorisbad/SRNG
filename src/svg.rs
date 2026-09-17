pub use crate::svg_import_impl::{ImportDiagnostic, ImportOptions, ImportResult};

/// Import SVG into SRNG while normalizing diagnostics to the capabilities of
/// the current renderer stack. The lower-level importer still records the
/// compatibility codes so older tests and tooling remain stable; this facade
/// reports features that are now rendered as informational mappings rather
/// than stale unsupported-feature warnings.
pub fn import_svg(svg: &str, source_name: &str, options: &ImportOptions) -> ImportResult {
    let mut result = crate::svg_import_impl::import_svg(svg, source_name, options);
    promote_native_semantics(&mut result.source);

    for diagnostic in &mut result.diagnostics {
        match diagnostic.code.as_str() {
            "S232" => {
                diagnostic.severity = "info".to_string();
                diagnostic.message = diagnostic
                    .message
                    .replace(
                        "is preserved with tile metadata; pattern painting is the next renderer stage",
                        "is mapped to native SRNG pattern paint with SVG provenance retained",
                    );
            }
            "S242" => {
                diagnostic.severity = "info".to_string();
                diagnostic.message = diagnostic
                    .message
                    .replace(
                        "requires alpha/luminance compositing that is not yet supported",
                        "is mapped to native SRNG mask semantics with SVG provenance retained",
                    );
            }
            _ => {}
        }
    }
    result
}

/// Add renderer-facing SRNG-native semantic aliases beside SVG provenance.
/// `svg-*` remains optional provenance; the unprefixed properties are the
/// semantic contract that downstream SRNG tooling should consume.
fn promote_native_semantics(source: &mut String) {
    const ALIASES: &[(&str, &str)] = &[
        ("svg-pattern-ref:", "pattern-ref:"),
        ("svg-pattern-width:", "pattern-width:"),
        ("svg-pattern-height:", "pattern-height:"),
        ("svg-pattern-source-xml:", "pattern-source:"),
        ("svg-mask-ref:", "mask-ref:"),
        ("svg-mask-mode:", "mask-mode:"),
        ("svg-attr-mask:", "mask-ref:"),
        ("svg-source-tag:", "source-tag:"),
        ("svg-source-xml:", "source-data:"),
    ];

    let mut out = String::with_capacity(source.len() + source.len() / 8);
    for line in source.lines() {
        out.push_str(line);
        out.push('\n');

        let trimmed = line.trim_start();
        let indent_len = line.len() - trimmed.len();
        let indent = &line[..indent_len];
        for (svg_key, native_key) in ALIASES {
            if let Some(rest) = trimmed.strip_prefix(svg_key) {
                out.push_str(indent);
                out.push_str(native_key);
                out.push_str(rest);
                out.push('\n');
                break;
            }
        }
    }
    *source = out;
}

/// Remove SVG-only provenance lines while preserving native SRNG semantics.
/// This is primarily useful for regression tests and AI-readability checks.
pub fn strip_svg_provenance(source: &str) -> String {
    source
        .lines()
        .filter(|line| !line.trim_start().starts_with("svg-"))
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pattern_semantics_survive_svg_provenance_strip() {
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20"><defs><pattern id="p" patternUnits="userSpaceOnUse" width="4" height="4"><rect width="2" height="4" fill="#fff"/></pattern></defs><rect id="box" width="20" height="20" fill="url(#p)"/></svg>"##;
        let result = import_svg(svg, "pattern.svg", &ImportOptions::default());
        let stripped = strip_svg_provenance(&result.source);
        assert!(stripped.contains("pattern-ref: \"p\";"));
        assert!(stripped.contains("pattern-width:"));
        assert!(stripped.contains("pattern-height:"));
        assert!(stripped.contains("pattern-source:"));
        assert!(stripped.contains("source-tag: \"defs\";"));
        assert!(stripped.contains("source-data:"));
        assert!(!stripped.lines().any(|line| line.trim_start().starts_with("svg-")));
    }

    #[test]
    fn binary_mask_keeps_native_mode_alias() {
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20"><defs><mask id="m"><circle cx="10" cy="10" r="5" fill="white"/></mask></defs><rect id="box" width="20" height="20" fill="#fff" mask="url(#m)"/></svg>"##;
        let result = import_svg(svg, "mask.svg", &ImportOptions::default());
        let stripped = strip_svg_provenance(&result.source);
        assert!(stripped.contains("mask-mode: \"binary-opaque-clip\";"));
        assert!(stripped.contains("mask-ref: \"url(#m)\";"));
        assert!(stripped.contains("clip:"));
    }

    #[test]
    fn complex_mask_keeps_native_resource_reference_after_strip() {
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20"><defs><mask id="m"><rect width="10" height="20" fill="#808080"/></mask></defs><rect id="box" width="20" height="20" fill="#fff" mask="url(#m)"/></svg>"##;
        let result = import_svg(svg, "mask.svg", &ImportOptions::default());
        let stripped = strip_svg_provenance(&result.source);
        assert!(stripped.contains("mask-ref: \"url(#m)\";"));
        assert!(stripped.contains("source-tag: \"defs\";"));
        assert!(stripped.contains("source-data:"));
        assert!(!stripped.lines().any(|line| line.trim_start().starts_with("svg-")));
    }
}
