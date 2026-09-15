use srng::runtime::{execute_file, execute_json, RuntimeOptions};
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

fn run(source: &str) -> srng::runtime::Scene {
    let ir = srng::compile_to_json(source, "test.srng");
    execute_json(&ir, &RuntimeOptions::default()).expect("runtime should accept compiler IR")
}

#[test]
fn resolves_geometry_and_custom_units() {
    let scene = run(r#"
srng 0.1;
unit gu = 8 px;
rect card { position: 2gu 3gu; size: 50% 25vh; fill: #fff; }
"#);
    let card = &scene.nodes[0];
    assert!(card.active);
    assert_eq!(card.geometry.x, Some(16.0));
    assert_eq!(card.geometry.y, Some(24.0));
    assert_eq!(card.geometry.width, Some(960.0));
    assert_eq!(card.geometry.height, Some(270.0));
}

#[test]
fn broken_node_does_not_disable_valid_nodes() {
    let scene = run(r#"
srng 0.1;
rect broken { position: nope; }
rect valid { position: 10px 20px; size: 30px 40px; }
"#);
    assert!(
        !scene
            .nodes
            .iter()
            .find(|node| node.id == "broken")
            .unwrap()
            .active
    );
    assert!(
        scene
            .nodes
            .iter()
            .find(|node| node.id == "valid")
            .unwrap()
            .active
    );
    assert!(scene
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "R200"));
}

#[test]
fn preserves_relationships_and_reference_provenance() {
    let scene = run(r#"
srng 0.1;
rect card { position: 0px 0px; }
text title { position: 4px 4px; }
relation card -> title { kind: contains; gap: 1px; }
reference logo = "./brand.srng#logo" { mode: reference-only; position: 2px 2px; }
"#);
    assert!(scene.relations[0].active);
    assert_eq!(scene.relations[0].kind.as_deref(), Some("contains"));
    assert_eq!(scene.references[0].provenance, "./brand.srng#logo");
    assert!(!scene.references[0].resolved);
    assert!(scene
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "R235"));
}

#[test]
fn compiler_ir_round_trip_is_valid_json() {
    let scene = run("srng 0.1; rect box { position: 1px 2px; }");
    let json = scene.to_json_pretty().unwrap();
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(value["format"], "SRNG-SCENE");
}

#[test]
fn resolves_chained_cross_file_reference_without_flattening() {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let directory =
        std::env::temp_dir().join(format!("srng-runtime-{}-{nonce}", std::process::id()));
    fs::create_dir_all(&directory).unwrap();
    fs::write(
        directory.join("shape.srng"),
        "srng 0.1; path mark { position: 1px 2px; data: M 0 0; }",
    )
    .unwrap();
    fs::write(
        directory.join("alias.srng"),
        "srng 0.1; reference alias = \"./shape.srng#mark\";",
    )
    .unwrap();
    fs::write(
        directory.join("root.srng"),
        "srng 0.1; reference logo = \"./alias.srng#alias\";",
    )
    .unwrap();

    let scene = execute_file(directory.join("root.srng"), &RuntimeOptions::default()).unwrap();
    let logo = &scene.references[0];
    assert!(logo.resolved);
    assert_eq!(logo.resolved_kind.as_deref(), Some("path"));
    assert!(scene.nodes.is_empty());

    fs::remove_dir_all(directory).unwrap();
}
