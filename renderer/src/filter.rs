use crate::{RenderDiagnostic, Rgba};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub struct FilterGraph {
    pub nodes: Vec<FilterNode>,
    pub output: FilterInput,
    pub region: FilterRegion,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FilterNode {
    pub result: String,
    pub op: FilterPrimitive,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum FilterInput {
    SourceGraphic,
    SourceAlpha,
    Named(String),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FilterRegion {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl FilterRegion {
    pub fn full(width: u16, height: u16) -> Self {
        Self { x: 0.0, y: 0.0, width: f64::from(width), height: f64::from(height) }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum FilterPrimitive {
    GaussianBlur { input: FilterInput, sigma_x: f64, sigma_y: f64 },
    Offset { input: FilterInput, dx: f64, dy: f64 },
    Blend { input: FilterInput, input2: FilterInput, mode: BlendMode },
    Composite { input: FilterInput, input2: FilterInput, operator: CompositeOperator },
    ColorMatrix { input: FilterInput, matrix: [f64; 20] },
    Flood { color: Rgba },
    Merge { inputs: Vec<FilterInput> },
    Morphology { input: FilterInput, operator: MorphologyOperator, radius_x: f64, radius_y: f64 },
    ComponentTransfer { input: FilterInput, red: TransferFunction, green: TransferFunction, blue: TransferFunction, alpha: TransferFunction },
    Unsupported { name: String, input: FilterInput },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlendMode { Normal, Multiply, Screen, Darken, Lighten }

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CompositeOperator {
    Over,
    In,
    Out,
    Atop,
    Xor,
    Arithmetic { k1: f64, k2: f64, k3: f64, k4: f64 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MorphologyOperator { Erode, Dilate }

#[derive(Debug, Clone, PartialEq)]
pub enum TransferFunction {
    Identity,
    Table(Vec<f64>),
    Discrete(Vec<f64>),
    Linear { slope: f64, intercept: f64 },
    Gamma { amplitude: f64, exponent: f64, offset: f64 },
}

pub fn parse_filter_graph(raw: &str, region: FilterRegion) -> Result<(FilterGraph, Vec<RenderDiagnostic>), String> {
    let raw = raw.trim();
    if raw.is_empty() { return Err("filter graph is empty".into()); }
    let mut diagnostics = Vec::new();
    let mut nodes = Vec::new();
    let mut previous = FilterInput::SourceGraphic;
    let mut serial = 0usize;

    for statement in raw.split(';').map(str::trim).filter(|s| !s.is_empty()) {
        let (body, result) = if let Some((body, result)) = statement.rsplit_once("->") {
            (body.trim(), result.trim().to_string())
        } else {
            serial += 1;
            (statement, format!("_filter_{serial}"))
        };
        if result.is_empty() { return Err("filter result name cannot be empty".into()); }
        let (name, args) = split_call(body)?;
        let fields = parse_fields(args);
        let input = fields.get("in").map(|v| parse_input(v)).unwrap_or_else(|| previous.clone());
        let op = match name {
            "blur" | "feGaussianBlur" => {
                let nums = positional_numbers(args);
                let sx = field_number(&fields, "sigma").or_else(|| field_number(&fields, "sigma_x")).or_else(|| nums.first().copied()).ok_or_else(|| "feGaussianBlur requires sigma".to_string())?;
                let sy = field_number(&fields, "sigma_y").or_else(|| nums.get(1).copied()).unwrap_or(sx);
                finite_nonnegative(sx, "blur sigma")?; finite_nonnegative(sy, "blur sigma")?;
                FilterPrimitive::GaussianBlur { input, sigma_x: sx, sigma_y: sy }
            }
            "offset" | "feOffset" => {
                let nums = positional_numbers(args);
                let dx = field_number(&fields, "dx").or_else(|| nums.first().copied()).unwrap_or(0.0);
                let dy = field_number(&fields, "dy").or_else(|| nums.get(1).copied()).unwrap_or(0.0);
                FilterPrimitive::Offset { input, dx, dy }
            }
            "blend" | "feBlend" => FilterPrimitive::Blend {
                input,
                input2: fields.get("in2").map(|v| parse_input(v)).unwrap_or(FilterInput::SourceGraphic),
                mode: parse_blend(fields.get("mode").map(String::as_str).unwrap_or("normal"))?,
            },
            "composite" | "feComposite" => {
                let operator_name = fields.get("operator").or_else(|| fields.get("op")).map(String::as_str).unwrap_or("over");
                let operator = if operator_name == "arithmetic" {
                    CompositeOperator::Arithmetic {
                        k1: field_number(&fields, "k1").unwrap_or(0.0),
                        k2: field_number(&fields, "k2").unwrap_or(0.0),
                        k3: field_number(&fields, "k3").unwrap_or(0.0),
                        k4: field_number(&fields, "k4").unwrap_or(0.0),
                    }
                } else { parse_composite(operator_name)? };
                FilterPrimitive::Composite { input, input2: fields.get("in2").map(|v| parse_input(v)).unwrap_or(FilterInput::SourceGraphic), operator }
            }
            "colorMatrix" | "feColorMatrix" => {
                let values = fields.get("values").map(|v| parse_number_list(v)).transpose()?.unwrap_or_else(|| positional_numbers(args));
                if values.len() != 20 { return Err("feColorMatrix matrix requires exactly 20 values".into()); }
                let mut matrix = [0.0; 20]; matrix.copy_from_slice(&values);
                FilterPrimitive::ColorMatrix { input, matrix }
            }
            "flood" | "feFlood" => {
                let value = fields.get("color").or_else(|| fields.get("flood-color")).map(String::as_str).unwrap_or("#000000ff");
                FilterPrimitive::Flood { color: parse_hex_color(value)? }
            }
            "merge" | "feMerge" => {
                let values: Vec<FilterInput> = fields.get("inputs").map(|v| v.split(|c| c == ',' || c == '|').filter(|s| !s.is_empty()).map(|s| parse_input(s.trim())).collect()).unwrap_or_else(|| positional_tokens(args).into_iter().map(|s| parse_input(&s)).collect());
                if values.is_empty() { return Err("feMerge requires at least one input".into()); }
                FilterPrimitive::Merge { inputs: values }
            }
            "morphology" | "feMorphology" => {
                let nums = positional_numbers(args);
                let rx = field_number(&fields, "radius").or_else(|| field_number(&fields, "radius_x")).or_else(|| nums.first().copied()).unwrap_or(0.0);
                let ry = field_number(&fields, "radius_y").or_else(|| nums.get(1).copied()).unwrap_or(rx);
                finite_nonnegative(rx, "morphology radius")?; finite_nonnegative(ry, "morphology radius")?;
                let operator = match fields.get("operator").map(String::as_str).unwrap_or("erode") { "erode" => MorphologyOperator::Erode, "dilate" => MorphologyOperator::Dilate, other => return Err(format!("unsupported morphology operator `{other}`")) };
                FilterPrimitive::Morphology { input, operator, radius_x: rx, radius_y: ry }
            }
            "componentTransfer" | "feComponentTransfer" => FilterPrimitive::ComponentTransfer {
                input,
                red: parse_transfer(fields.get("r"))?, green: parse_transfer(fields.get("g"))?, blue: parse_transfer(fields.get("b"))?, alpha: parse_transfer(fields.get("a"))?,
            },
            other => {
                diagnostics.push(RenderDiagnostic { severity: "warning".into(), code: "G241".into(), message: format!("unsupported filter primitive `{other}` is explicitly bypassed; its input is preserved"), declaration: None });
                FilterPrimitive::Unsupported { name: other.to_string(), input }
            }
        };
        previous = FilterInput::Named(result.clone());
        nodes.push(FilterNode { result, op });
    }
    if nodes.is_empty() { return Err("filter graph contains no primitives".into()); }
    Ok((FilterGraph { output: previous, nodes, region }, diagnostics))
}

fn split_call(value: &str) -> Result<(&str, &str), String> {
    let open = value.find('(').ok_or_else(|| format!("invalid filter primitive `{value}`"))?;
    let close = value.rfind(')').ok_or_else(|| format!("invalid filter primitive `{value}`"))?;
    if close < open { return Err(format!("invalid filter primitive `{value}`")); }
    Ok((value[..open].trim(), value[open + 1..close].trim()))
}
fn parse_fields(args:&str)->HashMap<String,String>{args.split(',').flat_map(|chunk|chunk.split_whitespace()).filter_map(|token|token.split_once('=').map(|(k,v)|(k.trim().to_string(),v.trim().to_string()))).collect()}
fn positional_tokens(args:&str)->Vec<String>{args.split(|c:char|c==','||c.is_whitespace()).filter(|s|!s.is_empty()&&!s.contains('=')).map(str::to_string).collect()}
fn positional_numbers(args:&str)->Vec<f64>{positional_tokens(args).into_iter().filter_map(|v|v.parse().ok()).collect()}
fn field_number(fields:&HashMap<String,String>,key:&str)->Option<f64>{fields.get(key).and_then(|v|v.parse().ok())}
fn parse_number_list(value:&str)->Result<Vec<f64>,String>{value.split(|c:char|c==','||c=='|'||c.is_whitespace()).filter(|s|!s.is_empty()).map(|s|s.parse::<f64>().map_err(|_|format!("invalid filter number `{s}`"))).collect()}
fn parse_input(value:&str)->FilterInput{match value.trim(){"SourceGraphic"=>FilterInput::SourceGraphic,"SourceAlpha"=>FilterInput::SourceAlpha,other=>FilterInput::Named(other.to_string())}}
fn parse_blend(value:&str)->Result<BlendMode,String>{match value{"normal"=>Ok(BlendMode::Normal),"multiply"=>Ok(BlendMode::Multiply),"screen"=>Ok(BlendMode::Screen),"darken"=>Ok(BlendMode::Darken),"lighten"=>Ok(BlendMode::Lighten),other=>Err(format!("unsupported blend mode `{other}`"))}}
fn parse_composite(value:&str)->Result<CompositeOperator,String>{match value{"over"=>Ok(CompositeOperator::Over),"in"=>Ok(CompositeOperator::In),"out"=>Ok(CompositeOperator::Out),"atop"=>Ok(CompositeOperator::Atop),"xor"=>Ok(CompositeOperator::Xor),other=>Err(format!("unsupported composite operator `{other}`"))}}
fn finite_nonnegative(value:f64,label:&str)->Result<(),String>{if value.is_finite()&&value>=0.0{Ok(())}else{Err(format!("{label} must be finite and non-negative"))}}
fn parse_transfer(value:Option<&String>)->Result<TransferFunction,String>{let Some(value)=value else{return Ok(TransferFunction::Identity)};let value=value.trim();if value=="identity"{return Ok(TransferFunction::Identity)};if let Some(v)=value.strip_prefix("table:"){return Ok(TransferFunction::Table(parse_number_list(v)?))}if let Some(v)=value.strip_prefix("discrete:"){return Ok(TransferFunction::Discrete(parse_number_list(v)?))}if let Some(v)=value.strip_prefix("linear:"){let n=parse_number_list(v)?;if n.len()!=2{return Err("linear transfer requires slope intercept".into())}return Ok(TransferFunction::Linear{slope:n[0],intercept:n[1]})}if let Some(v)=value.strip_prefix("gamma:"){let n=parse_number_list(v)?;if n.len()!=3{return Err("gamma transfer requires amplitude exponent offset".into())}return Ok(TransferFunction::Gamma{amplitude:n[0],exponent:n[1],offset:n[2]})}Err(format!("unsupported component transfer function `{value}`"))}
fn parse_hex_color(value:&str)->Result<Rgba,String>{let value=value.trim();let hex=value.strip_prefix('#').ok_or_else(||format!("unsupported flood color `{value}`"))?;let byte=|s:&str|u8::from_str_radix(s,16).map_err(|_|"invalid flood color".to_string());match hex.len(){3=>Ok(Rgba{r:byte(&hex[0..1].repeat(2))?,g:byte(&hex[1..2].repeat(2))?,b:byte(&hex[2..3].repeat(2))?,a:255}),4=>Ok(Rgba{r:byte(&hex[0..1].repeat(2))?,g:byte(&hex[1..2].repeat(2))?,b:byte(&hex[2..3].repeat(2))?,a:byte(&hex[3..4].repeat(2))?}),6=>Ok(Rgba{r:byte(&hex[0..2])?,g:byte(&hex[2..4])?,b:byte(&hex[4..6])?,a:255}),8=>Ok(Rgba{r:byte(&hex[0..2])?,g:byte(&hex[2..4])?,b:byte(&hex[4..6])?,a:byte(&hex[6..8])?}),_=>Err("flood color must be hexadecimal RGB or RGBA".into())}}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn named_results_chain(){let (g,d)=parse_filter_graph("blur(2)->a; offset(in=a dx=3 dy=4)->b",FilterRegion::full(20,20)).unwrap();assert!(d.is_empty());assert_eq!(g.output,FilterInput::Named("b".into()));assert_eq!(g.nodes.len(),2);}
    #[test] fn unsupported_is_diagnostic_and_preserves_input(){let (g,d)=parse_filter_graph("blur(1)->a; feTurbulence(in=a)->noise",FilterRegion::full(20,20)).unwrap();assert_eq!(d.len(),1);assert!(matches!(&g.nodes[1].op,FilterPrimitive::Unsupported{input:FilterInput::Named(name),..} if name=="a"));}
    #[test] fn arithmetic_composite_parses(){let(g,_)=parse_filter_graph("feComposite(in=SourceGraphic in2=SourceAlpha operator=arithmetic k1=1 k2=2 k3=3 k4=4)->c",FilterRegion::full(2,2)).unwrap();assert!(matches!(g.nodes[0].op,FilterPrimitive::Composite{operator:CompositeOperator::Arithmetic{k1:1.0,k2:2.0,k3:3.0,k4:4.0},..}));}
}
