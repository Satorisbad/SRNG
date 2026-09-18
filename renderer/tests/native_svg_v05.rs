use srng::runtime::{execute_json, RuntimeOptions};
use srng_renderer::{prepare_scene, Paint, RevisionGate};

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
    gradient-fx: 32;
    gradient-fy: 32;
    gradient-r: 32;
    gradient-stops: 0 #ffffffff, 1 #000000ff;
}
"#;
    let gate = RevisionGate::default();
    let revision = gate.begin();
    let prepared = prepare_scene(&scene(source), revision, &gate);
    assert!(prepared.commands.iter().any(|command| matches!(
        command,
        srng_renderer::Command::Fill { paint: Paint::RadialGradient { .. }, .. }
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
    assert!(prepared.commands.iter().any(|command| matches!(command, srng_renderer::Command::Fill { .. })));
}
