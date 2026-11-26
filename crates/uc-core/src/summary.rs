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
    /// `import ... from "x"` / `import "x"` / `import x = require("x")`.
    Static,
    /// `export ... from "x"`.
    ReExport,
    /// `import("x")`.
    Dynamic,
    /// `require("x")`.
    Require,
}

/// One binding of an import or re-export statement.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsImportName {
    /// Name imported from the target module: an export name, `default`, or `*` (namespace
    /// import, `export * as ns`, or `export *`).
    pub name: String,
    /// Re-exports: the name this module exports it as (`None` for `export * from`).
    pub exported: Option<String>,
    /// `type` modifier on the binding or on the statement.
    pub type_only: bool,
    /// The binding is used as a value (always true in JS; TypeScript elides imports only used
    /// as types). Re-exported bindings count as used.
    pub value_used: bool,
    /// Location of the imported name.
    pub loc: Location,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsImport {
    pub specifier: String,
    pub kind: JsImportKind,
    /// Imported bindings; empty for side-effect imports, dynamic imports and requires.
    pub names: Vec<JsImportName>,
    /// `import type` / `export type` statement (erased at compile time: no runtime edge).
    pub type_only: bool,
    /// Dynamic imports and requires: inside a function (evaluated lazily).
    pub in_function: bool,
    /// Location of the specifier string.
    pub loc: Location,
    /// Cross-file rules suppressed at this statement (`NOSONAR`, `eslint-disable` comments,
    /// `@ts-expect-error` / `@ts-ignore` for resolution rules).
    pub suppressed: Vec<String>,
}

impl JsImport {
    /// True if the statement is kept at runtime: it isn't type-only, and in TypeScript at least
    /// one binding is used as a value (or it's a side-effect import or a re-export).
    pub fn is_runtime(&self) -> bool {
        !self.type_only
            && (self.names.is_empty() || self.names.iter().any(|n| !n.type_only && (n.value_used || n.exported.is_some())))
    }
}

/// A name exported by a module.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsExport {
    /// Exported name (`default` for default exports).
    pub name: String,
    /// The export is a type (interface, type alias, `export type`).
    pub type_only: bool,
    /// Re-exports: index into [`JsSummary::imports`] of the statement it comes from, with the
    /// name imported there (`*` for namespaces).
    pub source: Option<(u32, String)>,
    /// Location of the exported name.
    pub loc: Location,
    /// Cross-file rules suppressed at this export.
    pub suppressed: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct JsSummary {
    pub imports: Vec<JsImport>,
    /// Exported names, `default` included, types included. `export * from` statements are the
    /// re-export imports with a `*` name and no `exported` name.
    pub exports: Vec<JsExport>,
    /// Uses `module.exports` / `exports.x` / `define(...)` (CommonJS / AMD), or TypeScript's
    /// `export =`: the named exports can't be listed.
    pub commonjs: bool,
    /// Has ESM syntax (`import` / `export`).
    pub esm: bool,
    /// Specifiers of `declare module "x" { ... }` blocks (module augmentation: they add exports
    /// to another module).
    pub augments: Vec<String>,
}

impl JsSummary {
    /// `export * from "x"` statements, as indices into `imports`.
    pub fn star_exports(&self) -> impl Iterator<Item = usize> + '_ {
        self.imports.iter().enumerate().filter_map(|(i, im)| {
            (im.kind == JsImportKind::ReExport && im.names.iter().any(|n| n.name == "*" && n.exported.is_none()))
                .then_some(i)
        })
    }
}
