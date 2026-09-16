use srng::runtime::{execute_file, execute_json, RuntimeOptions};
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

fn run(source: &str) -> srng::runtime::Scene {
    let ir = srng::compile_to_json(source, "test.srng");
    execute_json(&ir, &RuntimeOptions::default()).unwrap()
}

fn temp_dir() -> std::path::PathBuf {
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let path = std::env::temp_dir().join(format!("srng-regression-{}-{nonce}", std::process::id()));
    fs::create_dir_all(&path).unwrap();
    path
}

#[test]
fn rejects_local_reference_cycles() {
    let scene = run(r#"
srng 0.1;
reference a = ".#b";
reference b = ".#a";
"#);
    assert!(scene.references.iter().all(|r| !r.resolved && !r.active));
    assert!(scene.diagnostics.iter().any(|d| d.code == "R234"));
}

#[test]
fn rejects_cross_file_reference_cycles() {
    let dir = temp_dir();
    fs::write(dir.join("a.srng"), "srng 0.1; reference a = \"./b.srng#b\";").unwrap();
    fs::write(dir.join("b.srng"), "srng 0.1; reference b = \"./a.srng#a\";").unwrap();
    let scene = execute_file(dir.join("a.srng"), &RuntimeOptions::default()).unwrap();
    assert!(!scene.references[0].resolved);
    assert!(scene.diagnostics.iter().any(|d| d.code == "R234"));
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn broken_reference_deactivates_relations() {
    let scene = run(r#"
srng 0.1;
rect ok { position: 0px 0px; size: 10px 10px; }
reference missing = ".#does-not-exist";
relation ok -> missing { kind: contains; }
"#);
    assert!(!scene.references[0].active);
    assert!(!scene.relations[0].active);
    assert!(scene.diagnostics.iter().any(|d| d.code == "R210"));
}

#[test]
fn linked_geometry_and_provenance_are_retained() {
    let dir = temp_dir();
    fs::write(dir.join("asset.srng"), "srng 0.1; unit gu = 8 px; rect logo { position: 2gu 3gu; size: 4gu 5gu; fill: #ffffff; }").unwrap();
    fs::write(dir.join("root.srng"), "srng 0.1; reference mark = \"./asset.srng#logo\" { position: 100px 200px; }").unwrap();
    let scene = execute_file(dir.join("root.srng"), &RuntimeOptions::default()).unwrap();
    let reference = &scene.references[0];
    assert!(reference.resolved && reference.active);
    assert_eq!(reference.provenance, "./asset.srng#logo");
    assert_eq!(reference.geometry.x, Some(100.0));
    assert_eq!(reference.linked_geometry.as_ref().unwrap().x, Some(16.0));
    assert_eq!(reference.linked_geometry.as_ref().unwrap().height, Some(40.0));
    assert_eq!(reference.linked_properties.get("fill").map(String::as_str), Some("#ffffff"));
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn scene_serialization_preserves_units_and_paint_order() {
    let scene = run(r#"
srng 0.1;
unit gu = 8 px;
rect back { position: 0px 0px; size: 10px 10px; fill: #000000; }
rect front { position: 1gu 1gu; size: 5px 5px; fill: #ffffff; }
"#);
    assert_eq!(scene.unit_context["gu"].scale, 8.0);
    assert!(scene.nodes[0].paint_order < scene.nodes[1].paint_order);
    let json = scene.to_json_pretty().unwrap();
    assert!(json.contains("\"unit_context\""));
    assert!(json.contains("\"paint_order\""));
}

#[test]
fn scientific_notation_lengths_are_supported() {
    let scene = run("srng 0.1; rect box { position: 1e2px 2.5e1px; size: 10px 20px; }");
    let node = &scene.nodes[0];
    assert!(node.active);
    assert_eq!(node.geometry.x, Some(100.0));
    assert_eq!(node.geometry.y, Some(25.0));
}
