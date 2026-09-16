use std::fs;
use std::process::Command;

#[test]
fn render_cli_writes_valid_png_signature() {
    let base = std::env::temp_dir().join(format!("srng-render-cli-{}", std::process::id()));
    let input = base.with_extension("srng");
    let output = base.with_extension("png");

    fs::write(
        &input,
        "srng 0.1; canvas root { position: 0px 0px; size: 16px 12px; fill: none; } rect box { position: 2px 2px; size: 8px 6px; fill: #ff0000; }",
    )
    .unwrap();

    let status = Command::new(env!("CARGO_BIN_EXE_srng-render"))
        .arg(&input)
        .arg("-o")
        .arg(&output)
        .status()
        .expect("run srng-render");
    assert!(status.success());

    let bytes = fs::read(&output).expect("PNG output");
    assert!(bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]));

    let _ = fs::remove_file(input);
    let _ = fs::remove_file(output);
}
