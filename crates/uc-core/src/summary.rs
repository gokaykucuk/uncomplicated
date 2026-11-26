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
    /// Functions declared without a body (implemented in assembly or linked in).
    pub no_body: bool,
    /// Constants: 1-based index of the `const (...)` group in the file (0 for a single spec),
    /// so a group whose constants are used together (iota enums) is used as a whole.
    pub group: u32,
    /// Types: the `sync`/`sync/atomic` type held directly by value (`sync.Mutex`), if any.
    pub lock: Option<String>,
    /// Types: `lock` is the type of a field of this struct (what the per-file UG1018 sees), not
    /// nested in an array or anonymous struct.
    pub lock_direct: bool,
    /// Types: name hashes of the package-local named types held by value (struct fields,
    /// embedded fields, array elements, the underlying type of `type A B`), so lock-containing
    /// types can be resolved across files.
    pub value_fields: Vec<u64>,
}

/// A call statement `f(...)` whose results are discarded, `f` a bare identifier that is not a
/// local of the file (so a package-level function or a dot import).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoCall {
    /// Name hash of the callee (xxh3 of the name, `uc_go::name_hash`).
    pub name: u64,
    /// Location of the call expression.
    pub loc: Location,
}

/// A discarded call of a standard-library function whose error must be checked (`os.Remove`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoStdCall {
    /// `os.Remove`, `json.Unmarshal` (import path's package name and function).
    pub func: String,
    pub loc: Location,
}

/// A value of a named type (maybe package-local) copied by a receiver or parameter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoValueUse {
    /// Name hash of the type.
    pub type_name: u64,
    /// Location of the parameter name (or of its type when unnamed).
    pub loc: Location,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GoSummary {
    /// Package clause name (`foo`, `foo_test`, `main`).
    pub package: String,
    pub decls: Vec<GoDecl>,
    /// Name hashes of every identifier and type name used outside its own package-level
    /// declaration (sorted, deduplicated).
    pub refs: Vec<u64>,
    pub calls: Vec<GoCall>,
    pub std_calls: Vec<GoStdCall>,
    pub value_uses: Vec<GoValueUse>,
    /// The file uses `//go:linkname`, `//export` or cgo: unused-symbol checks are unreliable.
    pub opaque: bool,
    /// Name hashes of the remote symbols of `//go:linkname local remote` directives (another
    /// package may pull an unexported function of ours by name).
    pub linknames: Vec<u64>,
    /// Lines with a `NOSONAR` comment (sorted), for issues reported by cross-file passes.
    pub nosonar: Vec<u32>,
    /// `//nolint[:a,b]` comments (golangci-lint): line and linter list (empty for all linters).
    pub nolint: Vec<(u32, String)>,
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
