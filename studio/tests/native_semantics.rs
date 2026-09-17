use srng::svg::strip_svg_provenance;
use srng_studio::{convert_svg, render_srng};

#[test]
fn supported_import_renders_identically_without_svg_prefixed_properties() {
    let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="64" height="48">
      <defs>
        <pattern id="p" patternUnits="userSpaceOnUse" width="8" height="8">
          <rect width="4" height="8" fill="#ff0000"/>
          <rect x="4" width="4" height="8" fill="#0000ff"/>
        </pattern>
        <clipPath id="cut"><rect x="2" y="2" width="60" height="44" rx="6"/></clipPath>
        <mask id="fade"><rect x="32" width="32" height="48" fill="#808080"/></mask>
      </defs>
      <rect id="background" width="64" height="48" rx="6" fill="url(#p)" clip-path="url(#cut)"/>
      <rect id="masked" x="16" y="8" width="40" height="32" rx="5" fill="#38c172" mask="url(#fade)"/>
    </svg>"##;

    let imported = convert_svg(svg, "native-semantics.svg");
    assert!(!imported.has_errors(), "{:?}", imported.diagnostics);
    let normal = imported.rendered.expect("normal imported SRNG should render");

    let stripped = strip_svg_provenance(&imported.srng);
    assert!(
        !stripped.lines().any(|line| line.trim_start().starts_with("svg-")),
        "stripped SRNG still contains svg-* properties:\n{stripped}"
    );
    assert!(stripped.contains("pattern-ref:"));
    assert!(stripped.contains("pattern-source:"));
    assert!(stripped.contains("mask-ref:"));
    assert!(stripped.contains("source-data:"));
    assert!(stripped.contains("clip:"));

    let (native, diagnostics) = render_srng(&stripped, "native-semantics.srng");
    assert!(
        !diagnostics.iter().any(|d| d.severity == "error"),
        "native-only SRNG produced errors: {diagnostics:?}"
    );
    let native = native.expect("native-only SRNG should render");

    assert_eq!(normal.width, native.width);
    assert_eq!(normal.height, native.height);
    assert_eq!(normal.pixels, native.pixels, "native semantics changed rendered pixels");
}
