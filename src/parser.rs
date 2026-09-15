use crate::ast::{AnimationDecl, Declaration, Document, NodeDecl, Property, ReferenceDecl, RelationDecl, UnitDecl};
use crate::diagnostic::Diagnostic;
use crate::lexer::{Token, TokenKind};

pub fn parse(tokens: Vec<Token>, mut diagnostics: Vec<Diagnostic>) -> Document {
    let mut parser = Parser { tokens, index: 0, diagnostics: Vec::new() };
    let mut version = "0.1".to_string();
    let mut file_id = None;
    let mut declarations = Vec::new();

    if parser.match_ident("srng") {
        if let Some(v) = parser.take_scalar() { version = v; } else { parser.error_here("E100", "expected SRNG version after `srng`"); }
        parser.expect_semi();
    } else {
        parser.error_here("E101", "file must begin with `srng <version>;`");
    }

    while !parser.at_eof() {
        if parser.match_ident("file") {
            file_id = parser.take_scalar();
            if file_id.is_none() { parser.error_here("E102", "expected file identifier or string"); }
            parser.expect_semi();
            continue;
        }
        if parser.match_ident("unit") {
            if let Some(unit) = parser.parse_unit() { declarations.push(Declaration::Unit(unit)); }
            continue;
        }
        if parser.match_ident("relation") {
            if let Some(relation) = parser.parse_relation() { declarations.push(Declaration::Relation(relation)); }
            continue;
        }
        if parser.match_ident("reference") {
            if let Some(reference) = parser.parse_reference() { declarations.push(Declaration::Reference(reference)); }
            continue;
        }
        if parser.match_ident("animation") {
            if let Some(animation) = parser.parse_animation() { declarations.push(Declaration::Animation(animation)); }
            continue;
        }
        if parser.match_ident("node") {
            if let Some(node) = parser.parse_node(true) { declarations.push(Declaration::Node(node)); }
            continue;
        }
        if parser.peek_is_ident() {
            if let Some(node) = parser.parse_node(false) { declarations.push(Declaration::Node(node)); }
            continue;
        }
        parser.error_here("E103", "unexpected top-level token");
        parser.recover_top_level();
    }

    diagnostics.extend(parser.diagnostics);
    validate(&declarations, &mut diagnostics);
    Document { version, file_id, declarations, diagnostics }
}

struct Parser { tokens: Vec<Token>, index: usize, diagnostics: Vec<Diagnostic> }

impl Parser {
    fn parse_unit(&mut self) -> Option<UnitDecl> {
        let name = self.take_ident_or_error("E110", "expected unit name")?;
        if !self.match_kind(&TokenKind::Equal) { self.error_here("E111", "expected `=` in unit declaration"); }
        let scale_text = self.take_scalar().unwrap_or_else(|| { self.error_here("E112", "expected unit scale"); "1".to_string() });
        let scale = scale_text.parse::<f64>().unwrap_or_else(|_| { self.error_here("E113", "unit scale must be numeric"); 1.0 });
        let base = self.take_ident_or_error("E114", "expected base unit")?;
        self.expect_semi();
        Some(UnitDecl { name, scale, base })
    }

    fn parse_relation(&mut self) -> Option<RelationDecl> {
        let from = self.take_ident_or_error("E120", "expected source id after `relation`")?;
        if !self.match_kind(&TokenKind::Arrow) { self.error_here("E121", "expected `->` in relation"); }
        let to = self.take_ident_or_error("E122", "expected target id after `->`")?;
        let properties = self.parse_property_block("relation");
        Some(RelationDecl { from, to, properties })
    }

    fn parse_reference(&mut self) -> Option<ReferenceDecl> {
        let id = self.take_ident_or_error("E130", "expected reference id")?;
        if !self.match_kind(&TokenKind::Equal) { self.error_here("E131", "expected `=` in reference declaration"); }
        let target = self.take_scalar().unwrap_or_else(|| { self.error_here("E132", "expected reference target path"); String::new() });
        let properties = if self.peek_kind(&TokenKind::LBrace) { self.parse_property_block("reference") } else { self.expect_semi(); Vec::new() };
        Some(ReferenceDecl { id, target, properties })
    }

    fn parse_animation(&mut self) -> Option<AnimationDecl> {
        let id = self.take_ident_or_error("E140", "expected animation id")?;
        let properties = self.parse_property_block("animation");
        Some(AnimationDecl { id, properties })
    }

    fn parse_node(&mut self, explicit_node_keyword: bool) -> Option<NodeDecl> {
        let kind = if explicit_node_keyword { self.take_ident_or_error("E150", "expected node kind")? } else { self.take_ident_or_error("E151", "expected declaration kind")? };
        let id = self.take_ident_or_error("E152", "expected node id")?;
        let properties = self.parse_property_block(&kind);
        Some(NodeDecl { kind, id, properties })
    }

    fn parse_property_block(&mut self, owner: &str) -> Vec<Property> {
        if !self.match_kind(&TokenKind::LBrace) {
            self.error_here("E160", format!("expected `{{` after {owner} declaration"));
            self.recover_top_level();
            return Vec::new();
        }
        let mut properties = Vec::new();
        while !self.at_eof() && !self.peek_kind(&TokenKind::RBrace) {
            let name = match self.take_ident() {
                Some(name) => name,
                None => { self.error_here("E161", "expected property name"); self.recover_property(); continue; }
            };
            if !self.match_kind(&TokenKind::Colon) {
                self.error_here("E162", format!("expected `:` after property `{name}`"));
                self.recover_property();
                continue;
            }
            let value = self.collect_value_until_semi();
            if value.is_empty() { self.error_here("E163", format!("property `{name}` has no value")); }
            properties.push(Property { name, value });
            self.expect_semi();
        }
        if !self.match_kind(&TokenKind::RBrace) { self.error_here("E164", "unterminated property block"); }
        properties
    }

    fn collect_value_until_semi(&mut self) -> String {
        let mut parts = Vec::new();
        while !self.at_eof() && !self.peek_kind(&TokenKind::Semi) && !self.peek_kind(&TokenKind::RBrace) {
            let token = self.advance().clone();
            parts.push(token_text(&token.kind));
        }
        normalize_value(parts)
    }

    fn expect_semi(&mut self) {
        if !self.match_kind(&TokenKind::Semi) { self.error_here("E170", "expected `;`"); self.recover_property(); }
    }

    fn match_ident(&mut self, expected: &str) -> bool {
        match &self.peek().kind {
            TokenKind::Ident(value) if value == expected => { self.index += 1; true }
            _ => false,
        }
    }

    fn take_ident(&mut self) -> Option<String> {
        match self.peek().kind.clone() {
            TokenKind::Ident(value) => { self.index += 1; Some(value) }
            _ => None,
        }
    }

    fn take_ident_or_error(&mut self, code: &'static str, message: &str) -> Option<String> {
        let value = self.take_ident();
        if value.is_none() { self.error_here(code, message); }
        value
    }

    fn take_scalar(&mut self) -> Option<String> {
        match self.peek().kind.clone() {
            TokenKind::Ident(value) | TokenKind::Number(value) | TokenKind::String(value) | TokenKind::Color(value) => { self.index += 1; Some(value) }
            _ => None,
        }
    }

    fn match_kind(&mut self, expected: &TokenKind) -> bool {
        if same_variant(&self.peek().kind, expected) { self.index += 1; true } else { false }
    }
    fn peek_kind(&self, expected: &TokenKind) -> bool { same_variant(&self.peek().kind, expected) }
    fn peek_is_ident(&self) -> bool { matches!(self.peek().kind, TokenKind::Ident(_)) }
    fn at_eof(&self) -> bool { matches!(self.peek().kind, TokenKind::Eof) }
    fn peek(&self) -> &Token { &self.tokens[self.index.min(self.tokens.len() - 1)] }
    fn advance(&mut self) -> &Token {
        let idx = self.index.min(self.tokens.len() - 1);
        if self.index < self.tokens.len() - 1 { self.index += 1; }
        &self.tokens[idx]
    }
    fn error_here(&mut self, code: &'static str, message: impl Into<String>) {
        let token = self.peek();
        self.diagnostics.push(Diagnostic::error(code, message, token.line, token.column));
    }
    fn recover_property(&mut self) {
        while !self.at_eof() && !self.peek_kind(&TokenKind::Semi) && !self.peek_kind(&TokenKind::RBrace) { self.index += 1; }
        if self.peek_kind(&TokenKind::Semi) { self.index += 1; }
    }
    fn recover_top_level(&mut self) {
        while !self.at_eof() {
            if self.peek_kind(&TokenKind::Semi) { self.index += 1; break; }
            if self.peek_kind(&TokenKind::RBrace) { self.index += 1; break; }
            self.index += 1;
        }
    }
}

fn same_variant(a: &TokenKind, b: &TokenKind) -> bool { std::mem::discriminant(a) == std::mem::discriminant(b) }

fn token_text(kind: &TokenKind) -> String {
    match kind {
        TokenKind::Ident(v) | TokenKind::Number(v) | TokenKind::Color(v) => v.clone(),
        TokenKind::String(v) => format!("\"{}\"", v.replace('"', "\\\"")),
        TokenKind::LBrace => "{".into(), TokenKind::RBrace => "}".into(), TokenKind::Colon => ":".into(),
        TokenKind::Semi => ";".into(), TokenKind::Equal => "=".into(), TokenKind::Arrow => "->".into(),
        TokenKind::Comma => ",".into(), TokenKind::At => "@".into(), TokenKind::Eof => String::new(),
    }
}

fn normalize_value(parts: Vec<String>) -> String {
    let mut out = String::new();
    for part in parts {
        let no_space_before = matches!(part.as_str(), "," | ")" | "]" | "}");
        let no_space_after_prev = out.ends_with('(') || out.ends_with('[') || out.ends_with('{') || out.ends_with('@');
        if !out.is_empty() && !no_space_before && !no_space_after_prev { out.push(' '); }
        out.push_str(&part);
    }
    out
}

fn validate(declarations: &[Declaration], diagnostics: &mut Vec<Diagnostic>) {
    use std::collections::HashSet;
    let mut ids = HashSet::new();
    let mut units = HashSet::new();
    for builtin in ["px", "pt", "mm", "cm", "in", "%", "vw", "vh"] { units.insert(builtin.to_string()); }

    for declaration in declarations {
        match declaration {
            Declaration::Unit(unit) => {
                if !units.insert(unit.name.clone()) { diagnostics.push(Diagnostic::warning("W200", format!("unit `{}` is declared more than once", unit.name), 0, 0)); }
            }
            Declaration::Node(node) => {
                if !ids.insert(node.id.clone()) { diagnostics.push(Diagnostic::error("E200", format!("duplicate id `{}`", node.id), 0, 0)); }
                if !node.properties.iter().any(|p| p.name == "position") { diagnostics.push(Diagnostic::warning("W201", format!("node `{}` has no explicit position", node.id), 0, 0)); }
            }
            Declaration::Reference(reference) => {
                if !ids.insert(reference.id.clone()) { diagnostics.push(Diagnostic::error("E200", format!("duplicate id `{}`", reference.id), 0, 0)); }
                if !reference.target.contains('#') { diagnostics.push(Diagnostic::warning("W202", format!("reference `{}` does not name a target id with `#`", reference.id), 0, 0)); }
            }
            Declaration::Relation(_) | Declaration::Animation(_) => {}
        }
    }

    for declaration in declarations {
        if let Declaration::Relation(relation) = declaration {
            if !ids.contains(&relation.from) { diagnostics.push(Diagnostic::error("E210", format!("relation source `{}` is not declared", relation.from), 0, 0)); }
            if !ids.contains(&relation.to) { diagnostics.push(Diagnostic::error("E211", format!("relation target `{}` is not declared", relation.to), 0, 0)); }
        }
    }
}
