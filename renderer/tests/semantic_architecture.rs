use std::path::Path;

#[test]
fn renderer_uses_single_semantic_pipeline_entrypoint() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));

    assert!(root.join("src/lib.rs").exists());
    assert!(!root.join("src/lib2.rs").exists());

    for version in 4..=9 {
        assert!(
            !root.join(format!("src/semantic_v{version}.rs")).exists(),
            "version-numbered semantic wrapper semantic_v{version}.rs must not be reintroduced"
        );
    }

    let lib = std::fs::read_to_string(root.join("src/lib.rs")).unwrap();
    assert!(lib.contains("mod semantic;"));
    assert!(lib.contains("pub use semantic::prepare_scene;"));
    assert!(!lib.contains("semantic_v"));

    let semantic = std::fs::read_to_string(root.join("src/semantic.rs")).unwrap();
    assert!(semantic.contains("text::normalize(&mut normalized);"));
    assert!(semantic.contains("radial::normalize(&mut normalized);"));
    assert!(semantic.contains("opacity::normalize(&mut normalized);"));
    assert!(semantic.contains("geometry::normalize(&mut normalized);"));
    assert!(semantic.contains("compatibility::normalize(&mut normalized);"));
    assert_eq!(
        semantic.matches("pub fn prepare_scene(").count(),
        1,
        "semantic.rs should expose exactly one preparation entrypoint"
    );
}
