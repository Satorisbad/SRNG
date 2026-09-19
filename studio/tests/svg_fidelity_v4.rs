use srng::svg::strip_svg_provenance;
use srng_studio::{convert_svg, render_srng, RgbaImage};

fn rgba_at(image: &RgbaImage, x: usize, y: usize) -> [u8; 4] {
    let width = image.width as usize;
    let i = (y * width + x) * 4;
    [image.pixels[i], image.pixels[i + 1], image.pixels[i + 2], image.pixels[i + 3]]
}

fn render(svg: &str) -> RgbaImage {
    let outcome = convert_svg(svg, "v4.svg");
    assert!(!outcome.has_errors(), "{:?}", outcome.diagnostics);
    outcome.rendered.expect("SVG should render")
}

#[test]
fn root_viewbox_and_group_transform_are_applied() {
    let image = render(r##"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100" viewBox="0 0 10 10"><g transform="translate(2 0)"><rect width="2" height="2" fill="#ff0000"/></g></svg>"##);
    assert!(rgba_at(&image, 25, 5)[0] > 180);
    assert!(rgba_at(&image, 5, 5)[3] < 20);
}

#[test]
fn inherited_group_and_fill_opacity_are_applied() {
    let image = render(r##"<svg xmlns="http://www.w3.org/2000/svg" width="12" height="12"><g opacity="0.5"><rect width="12" height="12" fill="#ff0000" fill-opacity="0.5"/></g></svg>"##);
    let pixel = rgba_at(&image, 5, 5);
    assert!(pixel[3] > 40 && pixel[3] < 100, "expected about 25% alpha: {pixel:?}");
}

#[test]
fn linear_gradient_paints_natively() {
    let image = render(r##"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="4"><defs><linearGradient id="g"><stop offset="0" stop-color="#ff0000"/><stop offset="1" stop-color="#0000ff"/></linearGradient></defs><rect width="20" height="4" fill="url(#g)"/></svg>"##);
    let left = rgba_at(&image, 1, 2); let right = rgba_at(&image, 18, 2);
    assert!(left[0] > left[2], "left should be red-dominant: {left:?}");
    assert!(right[2] > right[0], "right should be blue-dominant: {right:?}");
}

#[test]
fn radial_gradient_renders() {
    let image = render(r##"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20"><defs><radialGradient id="g"><stop offset="0" stop-color="#ff0000"/><stop offset="1" stop-color="#0000ff"/></radialGradient></defs><rect width="20" height="20" fill="url(#g)"/></svg>"##);
    let center = rgba_at(&image, 10, 10); let edge = rgba_at(&image, 1, 1);
    assert!(center[0] > center[2], "center should be red-dominant: {center:?}");
    assert!(edge[2] > edge[0], "edge should be blue-dominant: {edge:?}");
}

#[test]
fn luminance_mask_uses_luminance_not_source_alpha() {
    let image = render(r##"<svg xmlns="http://www.w3.org/2000/svg" width="8" height="8"><defs><mask id="m" mask-type="luminance"><rect width="8" height="8" fill="#808080"/></mask></defs><rect width="8" height="8" fill="#ff0000" mask="url(#m)"/></svg>"##);
    let pixel = rgba_at(&image, 4, 4);
    assert!(pixel[3] > 90 && pixel[3] < 180, "gray luminance mask should yield partial alpha: {pixel:?}");
}

#[test]
fn object_bounding_box_clip_scales_to_target() {
    let image = render(r##"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="10"><defs><clipPath id="c" clipPathUnits="objectBoundingBox"><rect width="0.5" height="1"/></clipPath></defs><rect width="20" height="10" fill="#00ff00" clip-path="url(#c)"/></svg>"##);
    assert!(rgba_at(&image, 3, 5)[3] > 180); assert!(rgba_at(&image, 17, 5)[3] < 30);
}

#[test]
fn embedded_raster_image_renders() {
    let image = render(r##"<svg xmlns="http://www.w3.org/2000/svg" width="8" height="4"><image x="0" y="0" width="8" height="4" href="data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAIAAAABCAYAAAD0In+KAAAADklEQVR4nGP4z8AAQv8BD/kD/YURmXYAAAAASUVORK5CYII="/></svg>"##);
    assert!(image.pixels.iter().any(|b| *b != 0));
}

#[test]
fn simple_use_is_resolved_and_positioned() {
    let image = render(r##"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20"><defs><path id="p" d="M0 0 L8 0 L8 8 Z" fill="#ff0000"/></defs><use href="#p" x="5" y="5"/></svg>"##);
    let pixel = rgba_at(&image, 7, 6);
    assert!(pixel[0] > 150 && pixel[3] > 150, "use target should render red: {pixel:?}");
}

#[test]
fn practical_text_path_produces_pixels() {
    let image = render(r##"<svg xmlns="http://www.w3.org/2000/svg" width="80" height="24"><text x="2" y="18" font-size="16" font-family="sans-serif" fill="#ffffff">SRNG</text></svg>"##);
    assert!(image.pixels.chunks_exact(4).any(|p| p[3] > 0), "text should produce visible pixels");
}

#[test]
fn nested_pattern_content_remains_renderable() {
    let image = render(r##"<svg xmlns="http://www.w3.org/2000/svg" width="12" height="4"><defs><pattern id="p" patternUnits="userSpaceOnUse" width="4" height="4"><g><rect width="2" height="4" fill="#ff0000"/><rect x="2" width="2" height="4" fill="#0000ff"/></g></pattern></defs><rect width="12" height="4" fill="url(#p)"/></svg>"##);
    assert!(rgba_at(&image, 0, 1)[0] > rgba_at(&image, 0, 1)[2]);
    assert!(rgba_at(&image, 2, 1)[2] > rgba_at(&image, 2, 1)[0]);
    assert!(rgba_at(&image, 4, 1)[0] > rgba_at(&image, 4, 1)[2]);
}

#[test]
fn advanced_scene_survives_svg_provenance_strip() {
    let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="64" height="48" viewBox="0 0 32 24"><defs><linearGradient id="g"><stop offset="0" stop-color="#ff0000"/><stop offset="1" stop-color="#0000ff"/></linearGradient><clipPath id="c" clipPathUnits="objectBoundingBox"><rect width="0.75" height="1"/></clipPath></defs><g transform="translate(1 1)" opacity="0.8"><rect width="20" height="12" fill="url(#g)" clip-path="url(#c)"/><path id="p" d="M0 0 L4 0 L4 4 Z" fill="#00ff00"/><use href="#p" x="22" y="4"/></g></svg>"##;
    let imported = convert_svg(svg, "native-v4.svg");
    assert!(!imported.has_errors(), "{:?}", imported.diagnostics);
    let stripped = strip_svg_provenance(&imported.srng);
    assert!(!stripped.lines().any(|line| line.trim_start().starts_with("svg-")));
    assert!(stripped.contains("gradient-kind:"));
    assert!(stripped.contains("transform:"));
    assert!(stripped.contains("reference "), "native reusable content must remain a first-class reference: {stripped}");
    assert!(stripped.contains("resource-target:"), "reference target identity must survive provenance stripping: {stripped}");
    let (rendered, diagnostics) = render_srng(&stripped, "native-v4.srng");
    assert!(!diagnostics.iter().any(|d| d.severity == "error"), "{diagnostics:?}");
    let image = rendered.expect("native v4 SRNG should render");
    assert!(image.pixels.iter().any(|b| *b != 0));
}
