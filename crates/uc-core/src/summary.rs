//! Per-file facts that cross-file analyses need: the declarations and references of a Go file
//! (package-level checks) and the imports and exports of a JS/TS module (module-graph checks).
//! Summaries are computed during per-file analysis and cached with the file's result, so
//! cross-file passes never re-parse unchanged files.

use serde::{Deserialize, Serialize};

use crate::issue::Location;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Summary {
    Go(GoSummary),
    Js(JsSummary),
}

impl Summary {
    pub fn go(&self) -> Option<&GoSummary> {
        match self {
            Self::Go(g) => Some(g),
            Self::Js(_) => None,
        }
    }

    pub fn js(&self) -> Option<&JsSummary> {
        match self {
            Self::Js(j) => Some(j),
            Self::Go(_) => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GoDeclKind {
    Func,
    Type,
    Var,
    Const,
}

/// A package-level declaration (functions, not methods).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoDecl {
    pub name: String,
    pub kind: GoDeclKind,
    /// Location of the declared name.
    pub loc: Location,
    /// Functions: the last result is `error`.
    pub returns_error: bool,
    /// Struct types: names of the field types held by value (`sync.Mutex`, `otherStruct`...),
    /// so lock-containing types can be resolved across files.
    pub value_fields: Vec<String>,
}

/// A call statement whose results are discarded: `f(x)` where `f` is not declared in the file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoCall {
    pub name: String,
    pub loc: Location,
}

/// A value of a package-local named type copied by a receiver or parameter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoValueUse {
    pub type_name: String,
    /// `"receiver"` or `"parameter"`.
    pub what: String,
    pub loc: Location,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GoSummary {
    /// Package clause name (`foo`, `foo_test`, `main`).
    pub package: String,
    pub decls: Vec<GoDecl>,
    /// Hashes (xxh3) of every identifier name used outside its own declaration.
    pub refs: Vec<u64>,
    pub calls: Vec<GoCall>,
    pub value_uses: Vec<GoValueUse>,
    /// The file uses `//go:linkname`, `//export` or cgo: unused-symbol checks are unreliable.
    pub opaque: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum JsImportKind {
    /// `import ... from "x"` / `import "x"`.
    Static,
    /// `export ... from "x"`.
    ReExport,
    /// `import("x")`.
    Dynamic,
    /// `require("x")`.
    Require,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsImport {
    pub specifier: String,
    pub kind: JsImportKind,
    /// Imported binding names (`default`, `*` for namespaces); empty for side-effect imports.
    pub names: Vec<String>,
    /// `import type` / `export type` (erased at compile time: no runtime edge).
    pub type_only: bool,
    /// Location of the specifier string.
    pub loc: Location,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct JsSummary {
    pub imports: Vec<JsImport>,
    /// Exported names, `default` included, types included.
    pub exports: Vec<String>,
    /// Locations of the exported names, parallel to `exports`.
    pub export_locs: Vec<Location>,
    /// `export * from "x"` specifiers.
    pub star_exports: Vec<String>,
    /// Uses `module.exports` / `exports.x` (CommonJS).
    pub commonjs: bool,
}
