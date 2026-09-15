use crate::diagnostic::Diagnostic;

#[derive(Debug, Clone, PartialEq)]
pub struct Document {
    pub version: String,
    pub file_id: Option<String>,
    pub declarations: Vec<Declaration>,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Declaration {
    Unit(UnitDecl),
    Node(NodeDecl),
    Relation(RelationDecl),
    Reference(ReferenceDecl),
    Animation(AnimationDecl),
}

#[derive(Debug, Clone, PartialEq)]
pub struct UnitDecl {
    pub name: String,
    pub scale: f64,
    pub base: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NodeDecl {
    pub kind: String,
    pub id: String,
    pub properties: Vec<Property>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RelationDecl {
    pub from: String,
    pub to: String,
    pub properties: Vec<Property>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ReferenceDecl {
    pub id: String,
    pub target: String,
    pub properties: Vec<Property>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AnimationDecl {
    pub id: String,
    pub properties: Vec<Property>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Property {
    pub name: String,
    pub value: String,
}
