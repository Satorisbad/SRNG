#![cfg(feature = "gpu")]

use srng_renderer::{Command, EmbeddedImage, FillRule, Paint, PathData, PreparedScene, Rgba, VectorRecord};

#[test]
fn gpu_scene_reports_resource_commands_without_renderer_execution() {
    let scene = PreparedScene {
        width: 64,
        height: 64,
        revision: 1,
        diagnostics: vec![],
        commands: vec![Command::DrawImage {
            image: EmbeddedImage {
                href: "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8DwHwAFgAI/ScLqWQAAAABJRU5ErkJggg==".into(),
                x: 0.0,
                y: 0.0,
                width: 32.0,
                height: 32.0,
                preserve_aspect_ratio: "xMidYMid meet".into(),
            },
        }],
    };
    let diagnostics = srng_renderer::gpu::build_scene(&scene).unwrap_err();
    assert!(diagnostics.iter().any(|d| d.code == "G600"));
}

#[test]
fn gpu_vector_scene_keeps_existing_gradient_path() {
    let scene = PreparedScene {
        width: 64,
        height: 64,
        revision: 1,
        diagnostics: vec![],
        commands: vec![Command::Fill {
            path: PathData { svg: "M 0 0 H 64 V 64 H 0 Z".into() },
            paint: Paint::Solid(Rgba { r: 255, g: 0, b: 0, a: 255 }),
            rule: FillRule::NonZero,
        }],
    };
    assert!(srng_renderer::gpu::build_scene(&scene).is_ok());
}

#[test]
fn native_mask_records_remain_first_class_commands() {
    let scene = PreparedScene {
        width: 32,
        height: 32,
        revision: 1,
        diagnostics: vec![],
        commands: vec![
            Command::PushMask { records: vec![VectorRecord {
                path: PathData { svg: "M 0 0 H 16 V 16 H 0 Z".into() },
                paint: Paint::Solid(Rgba { r: 255, g: 255, b: 255, a: 255 }),
            }]},
            Command::Fill {
                path: PathData { svg: "M 0 0 H 32 V 32 H 0 Z".into() },
                paint: Paint::Solid(Rgba { r: 0, g: 255, b: 0, a: 255 }),
                rule: FillRule::NonZero,
            },
            Command::PopMask,
        ],
    };
    assert!(matches!(scene.commands[0], Command::PushMask { .. }));
    assert!(matches!(scene.commands[2], Command::PopMask));
}
