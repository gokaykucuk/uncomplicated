//! Everything a rule file usually needs.
pub use oxc_ast::{AstKind, AstType, ast::*};
pub use oxc_parser::Kind;
pub use oxc_semantic::{AstNode, NodeId, ScopeId, SymbolFlags, SymbolId};
pub use oxc_span::{GetSpan, Span};
pub use rustc_hash::{FxHashMap, FxHashSet};
pub use uc_core::Params;

pub use crate::helpers::{self, *};
pub use crate::{Ctx, JsRule};
