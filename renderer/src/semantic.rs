mod common;
mod compatibility;
mod geometry;
mod opacity;
mod radial;
mod text;

use crate::{prepare, PreparedScene, RevisionGate};
use srng::runtime::Scene;

/// The renderer's single semantic-normalization entry point.
///
/// Order matters: text geometry is synthesized first, radial resources are
/// materialized before paint normalization, inherited opacity is resolved
/// before it is baked into paints, geometry/transforms/masks/clips are then
/// normalized, and finally native resources are bridged into the remaining
/// compatibility keys consumed by the lower-level renderer preparation code.
pub fn prepare_scene(scene: &Scene, revision: u64, gate: &RevisionGate) -> PreparedScene {
    let mut normalized = scene.clone();

    text::normalize(&mut normalized);
    radial::normalize(&mut normalized);
    opacity::normalize(&mut normalized);
    geometry::normalize(&mut normalized);
    compatibility::normalize(&mut normalized);

    prepare::prepare_scene(&normalized, revision, gate)
}
