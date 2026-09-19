use srng::runtime::{execute_json, RuntimeOptions};
use srng_renderer::{prepare_scene, Command, FilterOp, GradientSpread, ImageCodec, Paint, RevisionGate};

fn scene(source: &str) -> srng::runtime::Scene {
    let ir = srng::compile_to_json(source, "native-svg-v05.srng");
    let mut options = RuntimeOptions::default();
    options.viewport_width = 64.0;
    options.viewport_height = 64.0;
    execute_json(&ir, &options).expect("runtime scene")
}

#[test]
fn native_radial_gradient_lowers_without_svg_pattern() {
    let source = r#"
srng 0.1;
rect r {
    position: 0px 0px;
    size: 64px 64px;
    fill: radial-gradient;
    gradient-cx: 32;
    gradient-cy: 32;
    gradient-fx: 24;
    gradient-fy: 24;
    gradient-fr: 4;
    gradient-r: 32;
    gradient-spread: reflect;
    gradient-stops: 0 #ffffffff, 1 #000000ff;
}
"#;
    let gate = RevisionGate::default();
    let revision = gate.begin();
    let prepared = prepare_scene(&scene(source), revision, &gate);
    assert!(prepared.commands.iter().any(|command| matches!(
        command,
        Command::Fill { paint: Paint::RadialGradient { focal_radius, spread: GradientSpread::Reflect, .. }, .. }
            if (*focal_radius - 4.0).abs() < 1e-6
    )));
    assert!(!prepared.diagnostics.iter().any(|d| d.severity == "error"), "{:?}", prepared.diagnostics);
}

#[test]
fn deterministic_text_fallback_produces_path_data() {
    let source = r#"
srng 0.1;
text label {
    position: 4px 24px;
    content: "SRNG 0.5!";
    font-size: 14px;
    font-weight: bold;
    font-style: italic;
    letter-spacing: 1px;
    text-anchor: start;
    fill: #ffffffff;
}
"#;
    let gate = RevisionGate::default();
    let revision = gate.begin();
    let prepared = prepare_scene(&scene(source), revision, &gate);
    assert!(prepared.commands.iter().any(|command| matches!(command, Command::Fill { .. })));
}

#[test]
fn native_filter_chain_lowers_to_filter_block() {
    let source = r#"
srng 0.1;
rect filtered {
    position: 8px 8px;
    size: 24px 24px;
    fill: #ff0000ff;
    filter-chain: "blur(2 3); offset(4 -1)";
}
"#;
    let gate = RevisionGate::default();
    let revision = gate.begin();
    let prepared = prepare_scene(&scene(source), revision, &gate);
    assert!(prepared.commands.iter().any(|command| matches!(
        command,
        Command::PushFilter { filters }
            if matches!(filters.as_slice(), [FilterOp::GaussianBlur { .. }, FilterOp::Offset { .. }])
    )));
    assert!(prepared.commands.iter().any(|command| matches!(command, Command::PopFilter)));
}

#[test]
fn png_data_image_decodes_to_native_resource() {
    let source = r#"
srng 0.1;
group image {
    position: 2px 3px;
    size: 8px 9px;
    image-data: "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAFgwJ/lC9pWQAAAABJRU5ErkJggg==";
    image-preserve-aspect-ratio: "xMidYMid meet";
}
"#;
    let gate = RevisionGate::default();
    let revision = gate.begin();
    let prepared = prepare_scene(&scene(source), revision, &gate);
    assert!(prepared.commands.iter().any(|command| matches!(command, Command::DrawImage { image } if image.codec == ImageCodec::Png && image.intrinsic_width == 1 && image.intrinsic_height == 1 && image.pixels.len() == 4 && image.width == 8.0 && image.height == 9.0)));
    assert!(!prepared.diagnostics.iter().any(|d| d.code == "G260"), "{:?}", prepared.diagnostics);
}

#[test]
fn mime_mismatch_reports_image_diagnostic() {
    let source = r#"
srng 0.1;
group image {
    position: 0px 0px;
    size: 8px 8px;
    image-data: "data:image/png;base64,R0lGODlh";
}
"#;
    let gate = RevisionGate::default();
    let revision = gate.begin();
    let prepared = prepare_scene(&scene(source), revision, &gate);
    assert!(prepared.diagnostics.iter().any(|d| d.code == "G260"));
    assert!(!prepared.commands.iter().any(|command| matches!(command, Command::DrawImage { .. })));
}

#[test]
fn external_image_url_is_rejected() {
    let source = r#"
srng 0.1;
group image {
    position: 0px 0px;
    size: 8px 8px;
    href: "https://example.invalid/image.png";
}
"#;
    let gate = RevisionGate::default();
    let revision = gate.begin();
    let prepared = prepare_scene(&scene(source), revision, &gate);
    assert!(prepared.diagnostics.iter().any(|d| d.code == "G260" && d.message.contains("disabled")));
}
