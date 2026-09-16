mod model;
mod prepare;

pub use model::*;
pub use prepare::prepare_scene;

#[cfg(feature = "cpu")]
pub mod cpu;

#[cfg(feature = "gpu")]
pub mod gpu;
