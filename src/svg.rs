#[path = "svg_import.rs"]
mod importer;

pub use importer::{ImportDiagnostic, ImportOptions, ImportResult};

/// Import SVG into SRNG while normalizing diagnostics to the capabilities of
/// the current renderer stack. The lower-level importer still records the
/// compatibility codes so older tests and tooling remain stable; this facade
/// reports features that are now rendered as informational mappings rather
/// than stale unsupported-feature warnings.
pub fn import_svg(svg: &str, source_name: &str, options: &ImportOptions) -> ImportResult {
    let mut result = importer::import_svg(svg, source_name, options);
    for diagnostic in &mut result.diagnostics {
        match diagnostic.code.as_str() {
            "S232" => {
                diagnostic.severity = "info".to_string();
                diagnostic.message = diagnostic
                    .message
                    .replace(
                        "is preserved with tile metadata; pattern painting is the next renderer stage",
                        "is mapped to repeating SVG tile paint",
                    );
            }
            "S242" => {
                diagnostic.severity = "info".to_string();
                diagnostic.message = diagnostic
                    .message
                    .replace(
                        "requires alpha/luminance compositing that is not yet supported",
                        "is mapped to renderer alpha/luminance compositing",
                    );
            }
            _ => {}
        }
    }
    result
}
