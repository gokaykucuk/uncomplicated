//! Everything a Go rule file usually needs.
pub use rustc_hash::{FxHashMap, FxHashSet};
pub use uc_core::{Params, Span};

pub use crate::ctx::span;
pub use crate::helpers::{self, *};
pub use crate::{Ctx, GoRule, Nd};
