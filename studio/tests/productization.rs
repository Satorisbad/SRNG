use srng_studio::{render_srng, RgbaImage};

fn assert_nonempty(image: &RgbaImage) {
    assert!(image.width > 0);
    assert!(image.height > 0);
    assert_eq!(image.pixels.len(), image.width * image.height * 4);
    assert!(image.pixels.iter().any(|byte| *byte != 0));
}

#[test]
fn direct_srng_document_renders_without_svg_conversion() {
    let source = r#"
srng 0.1;
file "direct";
canvas root { size: 96px 64px; }
rect panel { position: 8px 8px; size: 80px 48px; fill: #4f7cff; }
relation root -> panel { kind: contains; }
"#;

    let (image, diagnostics) = render_srng(source, "direct.srng");
    assert!(
        !diagnostics.iter().any(|d| d.severity == "error"),
        "{diagnostics:?}"
    );
    assert_nonempty(image.as_ref().expect("direct SRNG should render"));
}

#[test]
fn malformed_srng_returns_diagnostics_instead_of_panicking() {
    let source = r#"
srng 0.1;
canvas root { size: 64px 64px; }
rect broken {
    position: nope;
    size: 20px;
    fill: #zzzzzz;
}
"#;

    let (image, diagnostics) = render_srng(source, "malformed.srng");
    assert!(image.is_none() || !diagnostics.is_empty());
    assert!(diagnostics.len() < 256, "diagnostics must stay bounded");
}

#[test]
fn repeated_native_renders_are_deterministic() {
    let source = r#"
srng 0.1;
file "repeat";
canvas root { size: 128px 96px; }
rect a { position: 4px 4px; size: 120px 88px; fill: #10141f; }
rect b { position: 24px 20px; size: 80px 56px; fill: #8fb3ff; }
relation root -> a { kind: contains; }
relation root -> b { kind: contains; }
"#;

    let mut baseline = None;
    for _ in 0..20 {
        let (image, diagnostics) = render_srng(source, "repeat.srng");
        assert!(
            !diagnostics.iter().any(|d| d.severity == "error"),
            "{diagnostics:?}"
        );
        let image = image.expect("repeat render should succeed");
        assert_nonempty(&image);
        if let Some(expected) = baseline.as_ref() {
            assert_eq!(&image.pixels, expected, "repeated render changed pixels");
        } else {
            baseline = Some(image.pixels.clone());
        }
    }
}

#[test]
fn deep_relationship_chain_stays_renderable() {
    let source = r#"
srng 0.1;
file "deep";
canvas root { size: 256px 256px; }
rect a { position: 8px 8px; size: 240px 240px; fill: #10141f; }
rect b { position: 16px 16px; size: 224px 224px; fill: #1f2a44; }
rect c { position: 24px 24px; size: 208px 208px; fill: #2f3a54; }
rect d { position: 32px 32px; size: 192px 192px; fill: #4f7cff; }
rect e { position: 40px 40px; size: 176px 176px; fill: #8fb3ff; }
relation root -> a { kind: contains; }
relation a -> b { kind: contains; }
relation b -> c { kind: contains; }
relation c -> d { kind: contains; }
relation d -> e { kind: contains; }
"#;

    let (image, diagnostics) = render_srng(source, "deep.srng");
    assert!(
        !diagnostics.iter().any(|d| d.severity == "error"),
        "{diagnostics:?}"
    );
    assert_nonempty(image.as_ref().expect("deep scene should render"));
}
