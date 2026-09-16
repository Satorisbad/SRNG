use srng::runtime::{execute_json, RuntimeOptions};
use srng::svg::{import_svg, ImportOptions};

#[test]
fn svg_import_reaches_runtime_scene() {
    let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="120" height="80"><rect id="card" x="10" y="12" width="50" height="30" fill="#336699"/></svg>"##;
    let imported = import_svg(svg, "card.svg", &ImportOptions::default());
    assert!(!imported.has_errors(), "{:?}", imported.diagnostics);

    let ir = srng::compile_to_json(&imported.source, "card.srng");
    let mut options = RuntimeOptions::default();
    options.viewport_width = 120.0;
    options.viewport_height = 80.0;
    let scene = execute_json(&ir, &options).expect("generated SRNG must execute");

    assert!(!scene.has_errors(), "{:?}", scene.diagnostics);
    let card = scene.nodes.iter().find(|node| node.id == "card").expect("card node");
    assert!(card.active);
    assert_eq!(card.geometry.x, Some(10.0));
    assert_eq!(card.geometry.y, Some(12.0));
    assert_eq!(card.geometry.width, Some(50.0));
    assert_eq!(card.geometry.height, Some(30.0));
}

#[test]
fn percentage_lengths_use_svg_viewport_axes() {
    let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="100"><rect id="half" x="10%" y="20%" width="50%" height="50%"/></svg>"#;
    let imported = import_svg(svg, "percent.svg", &ImportOptions::default());
    assert!(imported.source.contains("position: 20px 20px;"));
    assert!(imported.source.contains("size: 100px 50px;"));
}

#[test]
fn unsupported_transform_is_never_silent() {
    let svg = r#"<svg xmlns="http://www.w3.org/2000/svg"><rect id="moved" width="10" height="10" transform="translate(20 30)"/></svg>"#;
    let imported = import_svg(svg, "transform.svg", &ImportOptions::default());
    assert!(imported.diagnostics.iter().any(|d| d.code == "S130" && d.element.as_deref() == Some("moved")));
    assert!(imported.source.contains("svg-attr-transform: \"translate(20 30)\";"));
}
