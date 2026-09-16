use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Debug, Clone, PartialEq)]
pub struct PreparedScene {
    pub width: u16,
    pub height: u16,
    pub revision: u64,
    pub commands: Vec<Command>,
    pub diagnostics: Vec<RenderDiagnostic>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    PushClip { path: PathData, rule: FillRule },
    PopClip,
    PushMaskSvg { svg: String },
    PopMask,
    Fill { path: PathData, paint: Paint, rule: FillRule },
    Stroke { path: PathData, paint: Paint, style: StrokeStyle },
}

#[derive(Debug, Clone, PartialEq)]
pub struct PathData {
    pub svg: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Paint {
    Solid(Rgba),
    LinearGradient {
        start: (f64, f64),
        end: (f64, f64),
        stops: Vec<GradientStop>,
    },
    SvgPattern {
        svg: String,
        tile_width: f64,
        tile_height: f64,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GradientStop {
    pub offset: f32,
    pub color: Rgba,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FillRule {
    NonZero,
    EvenOdd,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StrokeStyle {
    pub width: f64,
    pub miter_limit: f64,
    pub line_cap: LineCap,
    pub line_join: LineJoin,
    pub dash: Vec<f64>,
    pub dash_offset: f64,
}

impl Default for StrokeStyle {
    fn default() -> Self {
        Self {
            width: 1.0,
            miter_limit: 4.0,
            line_cap: LineCap::Butt,
            line_join: LineJoin::Miter,
            dash: Vec::new(),
            dash_offset: 0.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineCap {
    Butt,
    Round,
    Square,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineJoin {
    Miter,
    Round,
    Bevel,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderDiagnostic {
    pub severity: String,
    pub code: String,
    pub message: String,
    pub declaration: Option<String>,
}

#[derive(Debug, Default)]
pub struct RevisionGate(AtomicU64);

impl RevisionGate {
    pub fn begin(&self) -> u64 {
        self.0.fetch_add(1, Ordering::SeqCst) + 1
    }

    pub fn current(&self) -> u64 {
        self.0.load(Ordering::SeqCst)
    }

    pub fn is_current(&self, revision: u64) -> bool {
        self.current() == revision
    }
}
