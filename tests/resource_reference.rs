use srng::runtime::{execute_json, RuntimeOptions};
use srng::svg::{import_svg, ImportOptions};

fn scene_from_svg(svg: &str) -> srng::runtime::Scene {
    let imported = import_svg(svg, "resource-test.svg", &ImportOptions::default());
    let ir = srng::compile_to_json(&imported.source, "resource-test.srng");
    execute_json(&ir, &RuntimeOptions::default()).expect("resource scene should execute")
}

#[test]
fn group_reference_preserves_child_paints() {
    let scene = scene_from_svg(r##"
<svg xmlns="http://www.w3.org/2000/svg">
  <defs>
    <g id="badge">
      <rect id="back" width="20" height="10" fill="#ff0000"/>
      <circle id="dot" cx="5" cy="5" r="2" fill="#0000ff"/>
    </g>
  </defs>
  <use id="instance" href="#badge" x="30" y="40"/>
</svg>
"##);
    let back = scene.nodes.iter().find(|node| node.id == "instance::back").unwrap();
    let dot = scene.nodes.iter().find(|node| node.id == "instance::dot").unwrap();
    assert_eq!(back.properties.get("fill").map(String::as_str), Some("#ff0000"));
    assert_eq!(dot.properties.get("fill").map(String::as_str), Some("#0000ff"));
    assert_eq!(back.geometry.x, Some(30.0));
    assert_eq!(back.geometry.y, Some(40.0));
    assert_eq!(dot.geometry.x, Some(33.0));
    assert_eq!(dot.geometry.y, Some(43.0));
}

#[test]
fn nested_use_is_cycle_safe_and_resolves_shape() {
    let scene = scene_from_svg(r##"
<svg xmlns="http://www.w3.org/2000/svg">
  <defs>
    <rect id="base" width="4" height="5" fill="#00ff00"/>
    <use id="alias" href="#base" x="2" y="3"/>
  </defs>
  <use id="instance" href="#alias" x="10" y="20"/>
</svg>
"##);
    let reference = scene.references.iter().find(|reference| reference.id == "instance").unwrap();
    assert!(reference.resolved);
    assert_eq!(reference.resolved_kind.as_deref(), Some("rect"));
}

#[test]
fn direct_reference_cycle_is_diagnostic_not_scene_abort() {
    let source = r#"
srng 0.1;
reference a = "#b" { position: 0px 0px; }
reference b = "#a" { position: 0px 0px; }
rect valid { position: 1px 2px; size: 3px 4px; fill: #fff; }
"#;
    let ir = srng::compile_to_json(source, "cycle.srng");
    let scene = execute_json(&ir, &RuntimeOptions::default()).unwrap();
    assert!(scene.diagnostics.iter().any(|diagnostic| diagnostic.code == "R234"));
    assert!(scene.nodes.iter().any(|node| node.id == "valid" && node.active));
}

#[test]
fn broken_reference_does_not_disable_siblings() {
    let source = r#"
srng 0.1;
reference missing = "#does_not_exist" { position: 0px 0px; }
rect valid { position: 5px 6px; size: 7px 8px; fill: #fff; }
"#;
    let ir = srng::compile_to_json(source, "broken.srng");
    let scene = execute_json(&ir, &RuntimeOptions::default()).unwrap();
    assert!(scene.diagnostics.iter().any(|diagnostic| diagnostic.code == "R231"));
    assert!(scene.nodes.iter().any(|node| node.id == "valid" && node.active));
}

#[test]
fn symbol_viewbox_meet_maps_into_use_viewport() {
    let scene = scene_from_svg(r##"
<svg xmlns="http://www.w3.org/2000/svg">
  <defs>
    <symbol id="icon" viewBox="0 0 10 20" preserveAspectRatio="xMidYMid meet">
      <rect id="body" width="10" height="20" fill="#123456"/>
    </symbol>
  </defs>
  <use id="icon_instance" href="#icon" x="100" y="50" width="40" height="40"/>
</svg>
"##);
    let body = scene.nodes.iter().find(|node| node.id == "icon_instance::body").unwrap();
    assert_eq!(body.geometry.x, Some(110.0));
    assert_eq!(body.geometry.y, Some(50.0));
    assert_eq!(body.geometry.width, Some(20.0));
    assert_eq!(body.geometry.height, Some(40.0));
}

#[test]
fn use_style_only_overrides_inheritable_source_paint() {
    let scene = scene_from_svg(r##"
<svg xmlns="http://www.w3.org/2000/svg">
  <defs>
    <g id="pair">
      <rect id="inherits" width="5" height="5"/>
      <rect id="fixed" x="6" width="5" height="5" fill="#ff0000"/>
    </g>
  </defs>
  <use id="pair_instance" href="#pair" fill="#0000ff"/>
</svg>
"##);
    let inherits = scene.nodes.iter().find(|node| node.id == "pair_instance::inherits").unwrap();
    let fixed = scene.nodes.iter().find(|node| node.id == "pair_instance::fixed").unwrap();
    assert_eq!(inherits.properties.get("fill").map(String::as_str), Some("#0000ff"));
    assert_eq!(fixed.properties.get("fill").map(String::as_str), Some("#ff0000"));
}
