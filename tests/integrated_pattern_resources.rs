use srng::runtime::{execute_json, RuntimeOptions};
use srng::svg::{import_svg, ImportOptions};

#[test]
fn pattern_dimensions_survive_import_and_runtime() {
    let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="8" height="8"><defs><pattern id="p" width="4" height="5" patternUnits="userSpaceOnUse"><rect width="4" height="5" fill="red"/></pattern></defs><rect id="target" width="8" height="8" fill="url(#p)"/></svg>"#;
    let imported = import_svg(svg, "pattern.svg", &ImportOptions::default());
    let ir = srng::compile_to_json(&imported.source, "pattern.srng");
    let scene = execute_json(&ir, &RuntimeOptions::default()).expect("runtime should accept integrated pattern source");
    let target = scene.nodes.iter().find(|node| node.id == "target").expect("target node");
    assert_eq!(target.properties.get("pattern-width").map(String::as_str), Some("4px"), "source:\n{}\nprops:{:?}", imported.source, target.properties);
    assert_eq!(target.properties.get("pattern-height").map(String::as_str), Some("5px"), "source:\n{}\nprops:{:?}", imported.source, target.properties);
}