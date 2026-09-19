pub mod ast;
pub mod diagnostic;
pub mod ir;
pub mod lexer;
pub mod parser;
#[path = "svg_import.rs"]
mod svg_import_impl;
#[path = "svg.rs"]
mod svg_base;
#[path = "svg_v4.rs"]
mod svg_v4;
#[path = "svg_final.rs"]
pub mod svg;
#[path = "runtime_v2.rs"]
pub mod runtime;

use ast::Document;

pub fn compile(source: &str) -> Document {
    let lexed = lexer::lex(source);
    parser::parse(lexed.tokens, lexed.diagnostics)
}

pub fn compile_to_json(source: &str, source_name: &str) -> String {
    let document = compile(source);
    ir::emit_json(&document, source_name)
}
