use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use fontdb::{Database, Family, Query, Stretch, Style, Weight};
use ttf_parser::{Face, GlyphId, OutlineBuilder, Rect};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FontStyle {
    Normal,
    Italic,
    Oblique,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FontRequest {
    pub families: Vec<String>,
    pub weight: u16,
    pub style: FontStyle,
}

impl Default for FontRequest {
    fn default() -> Self {
        Self { families: vec!["sans-serif".to_string()], weight: 400, style: FontStyle::Normal }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GlyphIdValue(pub u16);

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FontMetrics {
    pub units_per_em: u16,
    pub ascent: i16,
    pub descent: i16,
    pub line_gap: i16,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GlyphMetrics {
    pub advance: u16,
    pub bbox: Option<GlyphBoundingBox>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GlyphBoundingBox {
    pub x_min: i16,
    pub y_min: i16,
    pub x_max: i16,
    pub y_max: i16,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PathSegment {
    MoveTo(f32, f32),
    LineTo(f32, f32),
    QuadTo(f32, f32, f32, f32),
    CurveTo(f32, f32, f32, f32, f32, f32),
    Close,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct GlyphOutline {
    pub segments: Vec<PathSegment>,
}

impl GlyphOutline {
    pub fn to_svg_path_scaled(&self, scale: f64, tx: f64, baseline_y: f64) -> String {
        let mut out = String::new();
        for segment in &self.segments {
            match *segment {
                PathSegment::MoveTo(x, y) => push_fmt(&mut out, format_args!("M {} {} ", fmt(tx + f64::from(x) * scale), fmt(baseline_y - f64::from(y) * scale))),
                PathSegment::LineTo(x, y) => push_fmt(&mut out, format_args!("L {} {} ", fmt(tx + f64::from(x) * scale), fmt(baseline_y - f64::from(y) * scale))),
                PathSegment::QuadTo(x1, y1, x, y) => push_fmt(&mut out, format_args!("Q {} {} {} {} ", fmt(tx + f64::from(x1) * scale), fmt(baseline_y - f64::from(y1) * scale), fmt(tx + f64::from(x) * scale), fmt(baseline_y - f64::from(y) * scale))),
                PathSegment::CurveTo(x1, y1, x2, y2, x, y) => push_fmt(&mut out, format_args!("C {} {} {} {} {} {} ", fmt(tx + f64::from(x1) * scale), fmt(baseline_y - f64::from(y1) * scale), fmt(tx + f64::from(x2) * scale), fmt(baseline_y - f64::from(y2) * scale), fmt(tx + f64::from(x) * scale), fmt(baseline_y - f64::from(y) * scale))),
                PathSegment::Close => out.push_str("Z "),
            }
        }
        out.trim().to_string()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FontDiagnostic {
    pub code: &'static str,
    pub message: String,
}

#[derive(Debug)]
pub enum FontError {
    Io { path: PathBuf, source: std::io::Error },
    InvalidFont { source: String },
    FaceUnavailable,
}

impl fmt::Display for FontError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => write!(f, "could not read font `{}`: {source}", path.display()),
            Self::InvalidFont { source } => write!(f, "invalid font: {source}"),
            Self::FaceUnavailable => write!(f, "font face is unavailable"),
        }
    }
}

impl std::error::Error for FontError {}

#[derive(Clone)]
pub struct FontFace {
    data: Arc<Vec<u8>>,
    face_index: u32,
    pub family_name: String,
    pub post_script_name: Option<String>,
    pub weight: u16,
    pub style: FontStyle,
    pub source_path: Option<PathBuf>,
}

impl fmt::Debug for FontFace {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FontFace")
            .field("family_name", &self.family_name)
            .field("post_script_name", &self.post_script_name)
            .field("weight", &self.weight)
            .field("style", &self.style)
            .field("source_path", &self.source_path)
            .finish()
    }
}

impl FontFace {
    pub fn from_bytes(data: Vec<u8>, face_index: u32, family_name: impl Into<String>) -> Result<Self, FontError> {
        let family_name = family_name.into();
        let face = Face::parse(&data, face_index).map_err(|error| FontError::InvalidFont { source: format!("{error:?}") })?;
        let weight = face.weight().to_number();
        let style = if face.is_italic() { FontStyle::Italic } else { FontStyle::Normal };
        let post_script_name = face.names().into_iter().find(|name| name.name_id == ttf_parser::name_id::POST_SCRIPT_NAME).and_then(|name| name.to_string());
        drop(face);
        Ok(Self { data: Arc::new(data), face_index, family_name, post_script_name, weight, style, source_path: None })
    }

    pub fn from_file(path: impl AsRef<Path>, face_index: u32) -> Result<Self, FontError> {
        let path = path.as_ref();
        let data = fs::read(path).map_err(|source| FontError::Io { path: path.to_path_buf(), source })?;
        let face = Face::parse(&data, face_index).map_err(|error| FontError::InvalidFont { source: format!("{error:?}") })?;
        let family_name = face.names().into_iter().find(|name| name.name_id == ttf_parser::name_id::TYPOGRAPHIC_FAMILY).and_then(|name| name.to_string())
            .or_else(|| face.names().into_iter().find(|name| name.name_id == ttf_parser::name_id::FAMILY).and_then(|name| name.to_string()))
            .unwrap_or_else(|| path.file_stem().and_then(|value| value.to_str()).unwrap_or("Unknown").to_string());
        drop(face);
        let mut result = Self::from_bytes(data, face_index, family_name)?;
        result.source_path = Some(path.to_path_buf());
        Ok(result)
    }

    fn parsed(&self) -> Result<Face<'_>, FontError> {
        Face::parse(self.data.as_slice(), self.face_index).map_err(|error| FontError::InvalidFont { source: format!("{error:?}") })
    }

    pub fn metrics(&self) -> Result<FontMetrics, FontError> {
        let face = self.parsed()?;
        Ok(FontMetrics { units_per_em: face.units_per_em(), ascent: face.ascender(), descent: face.descender(), line_gap: face.line_gap() })
    }

    pub fn glyph_for_char(&self, ch: char) -> Result<Option<GlyphIdValue>, FontError> {
        Ok(self.parsed()?.glyph_index(ch).map(|id| GlyphIdValue(id.0)))
    }

    pub fn glyph_metrics(&self, glyph: GlyphIdValue) -> Result<GlyphMetrics, FontError> {
        let face = self.parsed()?;
        let glyph = GlyphId(glyph.0);
        Ok(GlyphMetrics {
            advance: face.glyph_hor_advance(glyph).unwrap_or(0),
            bbox: face.glyph_bounding_box(glyph).map(rect_to_bbox),
        })
    }

    pub fn glyph_outline(&self, glyph: GlyphIdValue) -> Result<Option<GlyphOutline>, FontError> {
        let face = self.parsed()?;
        let mut builder = OutlineCollector::default();
        let bounds = face.outline_glyph(GlyphId(glyph.0), &mut builder);
        if bounds.is_none() && builder.outline.segments.is_empty() { return Ok(None); }
        Ok(Some(builder.outline))
    }
}

pub struct FontSystem {
    db: Database,
    cache: BTreeMap<String, FontFace>,
}

impl fmt::Debug for FontSystem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FontSystem").field("cached_faces", &self.cache.len()).finish()
    }
}

impl Default for FontSystem {
    fn default() -> Self {
        let mut db = Database::new();
        db.load_system_fonts();
        Self { db, cache: BTreeMap::new() }
    }
}

impl FontSystem {
    pub fn empty() -> Self { Self { db: Database::new(), cache: BTreeMap::new() } }

    pub fn load_system_fonts(&mut self) { self.db.load_system_fonts(); }

    pub fn load_font_file(&mut self, path: impl AsRef<Path>) -> Result<(), FontError> {
        let path = path.as_ref();
        let data = fs::read(path).map_err(|source| FontError::Io { path: path.to_path_buf(), source })?;
        Face::parse(&data, 0).map_err(|error| FontError::InvalidFont { source: format!("{error:?}") })?;
        self.db.load_font_file(path).map_err(|error| FontError::InvalidFont { source: error.to_string() })?;
        Ok(())
    }

    pub fn resolve(&mut self, request: &FontRequest) -> Result<(Option<FontFace>, Vec<FontDiagnostic>), FontError> {
        let mut diagnostics = Vec::new();
        let mut families = request.families.iter().filter(|family| !family.trim().is_empty()).cloned().collect::<Vec<_>>();
        if families.is_empty() { families.push("sans-serif".to_string()); }

        for requested_family in &families {
            let family = map_family(requested_family);
            let query = Query {
                families: &[family],
                weight: Weight(request.weight.clamp(1, 1000)),
                stretch: Stretch::Normal,
                style: map_style(request.style),
            };
            if let Some(id) = self.db.query(&query) {
                let key = format!("{:?}", id);
                if let Some(face) = self.cache.get(&key) { return Ok((Some(face.clone()), diagnostics)); }
                let mut loaded: Option<FontFace> = None;
                self.db.with_face_data(id, |data, face_index| {
                    if let Ok(mut face) = FontFace::from_bytes(data.to_vec(), face_index, requested_family.clone()) {
                        if let Some(info) = self.db.face(id) {
                            face.family_name = info.families.first().map(|pair| pair.0.clone()).unwrap_or_else(|| requested_family.clone());
                            face.post_script_name = Some(info.post_script_name.clone());
                            face.weight = info.weight.0;
                            face.style = unmap_style(info.style);
                            if let fontdb::Source::File(path) = &info.source { face.source_path = Some(path.clone()); }
                        }
                        loaded = Some(face);
                    }
                });
                if let Some(face) = loaded {
                    self.cache.insert(key, face.clone());
                    return Ok((Some(face), diagnostics));
                }
            }
            diagnostics.push(FontDiagnostic { code: "F001", message: format!("requested font family `{requested_family}` is unavailable") });
        }

        Ok((None, diagnostics))
    }
}

fn map_family(name: &str) -> Family<'_> {
    match name.trim().trim_matches('"').trim_matches('\'').to_ascii_lowercase().as_str() {
        "serif" => Family::Serif,
        "sans-serif" => Family::SansSerif,
        "monospace" => Family::Monospace,
        "cursive" => Family::Cursive,
        "fantasy" => Family::Fantasy,
        _ => Family::Name(name.trim().trim_matches('"').trim_matches('\'')),
    }
}

fn map_style(style: FontStyle) -> Style {
    match style { FontStyle::Normal => Style::Normal, FontStyle::Italic => Style::Italic, FontStyle::Oblique => Style::Oblique }
}
fn unmap_style(style: Style) -> FontStyle {
    match style { Style::Italic => FontStyle::Italic, Style::Oblique => FontStyle::Oblique, Style::Normal => FontStyle::Normal }
}
fn rect_to_bbox(rect: Rect) -> GlyphBoundingBox { GlyphBoundingBox { x_min: rect.x_min, y_min: rect.y_min, x_max: rect.x_max, y_max: rect.y_max } }

#[derive(Default)]
struct OutlineCollector { outline: GlyphOutline }
impl OutlineBuilder for OutlineCollector {
    fn move_to(&mut self, x: f32, y: f32) { self.outline.segments.push(PathSegment::MoveTo(x, y)); }
    fn line_to(&mut self, x: f32, y: f32) { self.outline.segments.push(PathSegment::LineTo(x, y)); }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) { self.outline.segments.push(PathSegment::QuadTo(x1, y1, x, y)); }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) { self.outline.segments.push(PathSegment::CurveTo(x1, y1, x2, y2, x, y)); }
    fn close(&mut self) { self.outline.segments.push(PathSegment::Close); }
}

fn push_fmt(out: &mut String, args: fmt::Arguments<'_>) { use fmt::Write as _; let _ = out.write_fmt(args); }
fn fmt(value: f64) -> String { if value.fract().abs() < 1e-9 { format!("{}", value as i64) } else { format!("{value:.6}").trim_end_matches('0').trim_end_matches('.').to_string() } }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn malformed_font_is_rejected() {
        let error = FontFace::from_bytes(vec![0, 1, 2, 3], 0, "Broken").unwrap_err();
        assert!(matches!(error, FontError::InvalidFont { .. }));
    }

    #[test]
    fn empty_database_reports_missing_font_without_panicking() {
        let mut system = FontSystem::empty();
        let (face, diagnostics) = system.resolve(&FontRequest { families: vec!["Definitely Missing SRNG Font".into()], weight: 400, style: FontStyle::Normal }).unwrap();
        assert!(face.is_none());
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "F001");
    }

    #[test]
    fn outline_scaling_preserves_curve_commands() {
        let outline = GlyphOutline { segments: vec![PathSegment::MoveTo(0.0, 0.0), PathSegment::QuadTo(10.0, 20.0, 30.0, 0.0), PathSegment::CurveTo(1.0, 2.0, 3.0, 4.0, 5.0, 6.0), PathSegment::Close] };
        let path = outline.to_svg_path_scaled(2.0, 4.0, 50.0);
        assert!(path.contains('Q'));
        assert!(path.contains('C'));
        assert!(path.contains('Z'));
    }
}