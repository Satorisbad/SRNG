pub mod ast;
pub mod diagnostic;
pub mod ir;
pub mod lexer;
pub mod parser;
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
