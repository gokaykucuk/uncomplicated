//! Fast node-kind and field lookups for hot paths. `Node::kind()` and `child_by_field_name`
//! resolve strings on every call; these tables map kind ids to an enum once.

use std::sync::LazyLock;

use tree_sitter::Node;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum K {
    Other,
    If,
    For,
    ExprSwitch,
    TypeSwitch,
    Select,
    ExprCase,
    TypeCase,
    DefaultCase,
    CommCase,
    Break,
    Continue,
    Goto,
    Binary,
    FuncLit,
    FuncDecl,
    MethodDecl,
    Paren,
    Identifier,
    Nil,
    Block,
    Comment,
    TypeSpec,
    StructType,
    Else,
}

static TABLE: LazyLock<Vec<K>> = LazyLock::new(|| {
    let lang = &*crate::LANGUAGE;
    (0..lang.node_kind_count() as u16)
        .map(|id| {
            let named = lang.node_kind_is_named(id);
            match (lang.node_kind_for_id(id).unwrap_or(""), named) {
                ("if_statement", true) => K::If,
                ("for_statement", true) => K::For,
                ("expression_switch_statement", true) => K::ExprSwitch,
                ("type_switch_statement", true) => K::TypeSwitch,
                ("select_statement", true) => K::Select,
                ("expression_case", true) => K::ExprCase,
                ("type_case", true) => K::TypeCase,
                ("default_case", true) => K::DefaultCase,
                ("communication_case", true) => K::CommCase,
                ("break_statement", true) => K::Break,
                ("continue_statement", true) => K::Continue,
                ("goto_statement", true) => K::Goto,
                ("binary_expression", true) => K::Binary,
                ("func_literal", true) => K::FuncLit,
                ("function_declaration", true) => K::FuncDecl,
                ("method_declaration", true) => K::MethodDecl,
                ("parenthesized_expression", true) => K::Paren,
                ("identifier", true) => K::Identifier,
                ("nil", true) => K::Nil,
                ("block", true) => K::Block,
                ("comment", true) => K::Comment,
                ("type_spec", true) => K::TypeSpec,
                ("struct_type", true) => K::StructType,
                ("else", false) => K::Else,
                _ => K::Other,
            }
        })
        .collect()
});

#[inline]
pub fn kind(n: Node<'_>) -> K {
    TABLE.get(n.kind_id() as usize).copied().unwrap_or(K::Other)
}

pub struct Fields {
    pub body: u16,
    pub condition: u16,
    pub consequence: u16,
    pub alternative: u16,
    pub initializer: u16,
    pub operator: u16,
    pub left: u16,
    pub right: u16,
    pub name: u16,
    pub type_: u16,
}

pub static F: LazyLock<Fields> = LazyLock::new(|| {
    let id = |n: &str| crate::LANGUAGE.field_id_for_name(n).map_or(0, |f| f.get());
    Fields {
        body: id("body"),
        condition: id("condition"),
        consequence: id("consequence"),
        alternative: id("alternative"),
        initializer: id("initializer"),
        operator: id("operator"),
        left: id("left"),
        right: id("right"),
        name: id("name"),
        type_: id("type"),
    }
});

#[inline]
pub fn field<'t>(n: Node<'t>, id: u16) -> Option<Node<'t>> {
    n.child_by_field_id(id)
}
