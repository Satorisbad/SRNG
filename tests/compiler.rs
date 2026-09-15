use srng::ast::Declaration;
use srng::diagnostic::Severity;

#[test]
fn parses_core_v01_declarations() {
    let source = r#"
srng 0.1;
file "test";
unit gu = 8 px;
rect card { position: 10px 20px; size: 100px 50px; }
text title { position: 20px 30px; content: "Hi"; }
relation card -> title { kind: contains; gap: 2gu; }
reference icon = "./icons.srng#star" { mode: reference-only; position: 4px 4px; }
animation hover { reference: "./motion.srng#hover"; }
"#;

    let doc = srng::compile(source);
    assert_eq!(doc.version, "0.1");
    assert_eq!(doc.file_id.as_deref(), Some("test"));
    assert_eq!(doc.declarations.len(), 6);
    assert!(!doc.diagnostics.iter().any(|d| d.severity == Severity::Error));
    assert!(matches!(doc.declarations[0], Declaration::Unit(_)));
    assert!(matches!(doc.declarations[3], Declaration::Relation(_)));
}

#[test]
fn recovers_and_keeps_valid_remainder() {
    let source = r#"
srng 0.1;
rect broken { position 10px 20px; fill: #ffffff; }
rect good { position: 30px 40px; size: 50px 60px; }
"#;

    let doc = srng::compile(source);
    assert!(doc.diagnostics.iter().any(|d| d.severity == Severity::Error));
    assert!(doc.declarations.iter().any(|d| match d {
        Declaration::Node(node) => node.id == "good",
        _ => false,
    }));
}

#[test]
fn reports_broken_relations_without_dropping_ir() {
    let source = r#"
srng 0.1;
rect card { position: 0px 0px; }
relation card -> missing { kind: contains; }
"#;

    let doc = srng::compile(source);
    assert!(doc.diagnostics.iter().any(|d| d.code == "E211"));
    let json = srng::ir::emit_json(&doc, "test.srng");
    assert!(json.contains("\"diagnostics\""));
    assert!(json.contains("\"card\""));
}

#[test]
fn accepts_percent_units() {
    let source = "srng 0.1; rect box { position: 10% 20%; size: 50% 25%; }";
    let doc = srng::compile(source);
    assert!(!doc.diagnostics.iter().any(|d| d.code == "E003"));
}
