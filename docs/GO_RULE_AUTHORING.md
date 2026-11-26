# Writing Go rules

Go rules live in `crates/uc-go/src/rules/`. `build.rs` discovers them: `sNNNN.rs` implements the
Sonar rule `SNNNN` from `catalog/go.json`; `ugNNNN.rs` implements our own rule `UGNNNN` from
`catalog/go_extra.json` (rules beyond Sonar's Go analyzer). There is no registry to edit.

## Shape of a rule

```rust
//! <title>
use crate::prelude::*;

pub struct Rule {
    max: u32,
}

impl Rule {
    pub fn new(p: &Params) -> Self {
        Self { max: p.u32("max", 7) }
    }
}

impl GoRule for Rule {
    /// tree-sitter-go named node kinds `run` is called for (see node-types.json of tree-sitter-go 0.25).
    fn interests(&self) -> &'static [&'static str] {
        &["function_declaration", "method_declaration"]
    }

    fn run(&self, node: Nd<'_>, ctx: &Ctx<'_>) {
        // ...
        ctx.report(node, "message");
    }

    /// Optional whole-file pass (comments, lines, cross-node aggregation).
    fn run_once(&self, ctx: &Ctx<'_>) {}
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn rule() {
        check("S1234", r#"package p

func f() {} // Noncompliant {{message}}
"#);
    }
}
```

`Nd<'t>` is a `tree_sitter::Node`. The engine walks every node once and dispatches named nodes to
subscribed rules. Files that fail to parse only get `S2260` (like sonar-go).

## Context and helpers

- Reporting: `ctx.report(node, msg)`, `ctx.report_span(span, msg)`,
  `ctx.report_with(node, msg, [(span(n), Some(msg))])`, `ctx.report_issue(span, msg, secondaries, gap)`.
- Tree: `ctx.parent(n)`, `ctx.ancestors(n)`, `ctx.enclosing_function(n)`, `ctx.field(n, "name")`,
  `ctx.named_children(n)` (comments excluded), `ctx.children(n)`, `ctx.nodes` (pre-order, all nodes),
  `ctx.root`.
- Text/tokens: `ctx.text(n)`, `ctx.line(n)`, `ctx.end_line(n)`, `ctx.lines` (`LineIndex`),
  `ctx.comments`, `ctx.comment_text(c)`, `ctx.tokens_in(n)`, `ctx.same_tokens(a, b)` (syntactic
  equivalence ignoring whitespace/comments), `ctx.token_count(n)`. File: `ctx.file.path`,
  `ctx.file.is_test`.
- `helpers`: `strip_parens`, `binary_op`, `is_function`, `function_report_span`, `block_statements`,
  `is_empty_block`, `block_has_comment`, `string_value`, `call_path` (`fmt.Println`), `selector_path`,
  `call_args`, `imports` (local name → import path), `PREDECLARED`, `looks_like_error_ident`.
- `scope::function_scopes(ctx, func)`: local declarations of a function with their reads and
  writes (parameters, `:=`, `var`, range variables, type-switch aliases, closures).
- `cognitive`: cognitive complexity increments.
- Read the grammar in `~/.cargo/registry/src/index.crates.io-*/tree-sitter-go-0.25.0/src/node-types.json`
  (field names: `condition`, `consequence`, `alternative`, `body`, `name`, `parameters`, `result`,
  `left`, `right`, `operator`, `function`, `arguments`, `operand`, `field`, ...). When unsure about a
  shape, print `node.to_sexp()` in a scratch test.

There is no Go type checker: identify types syntactically (declarations in the same function,
literals, well-known std-lib calls, import aliases). Precision first: when unsure, stay silent.

## Behaviour, performance, tests

Same requirements as `docs/RULE_AUTHORING.md`: Sonar rules match Sonar's triggers, defaults,
location and message (spec: the `.html` description next to each rule's `.json` in the sonar-go
checkout; read sonar-go's check for edge cases but write your own implementation). Narrowest
`interests`, no per-node regex compilation, no quadratic scans. Every rule has tests with compliant
and noncompliant code. Run with `cargo test -p uc-go rules::s1234`.

## Package-level rules

A Go package spans the files of a directory, so some checks can't be decided from one file.
These run in `crates/uc-engine/src/gopkg.rs` after per-file analysis:

- While analyzing a file, `crates/uc-go/src/summary.rs` records a `GoSummary` (cached with the
  file's result): package name, package-level declarations (functions with whether they return an
  `error`, types with the lock and package types they hold by value, vars, consts and their
  `const (...)` group), the hashes (`uc_go::name_hash`) of every name the file references, discarded
  call statements, by-value parameters of named types, `NOSONAR`/`//nolint` lines, and whether the
  file is opaque (cgo, `//go:linkname`, `//export`). Keep it cheap: one pass, names hashed.
- `gopkg::run` groups summaries by directory and package clause and reports onto the files'
  results. Its issues are never cached (they depend on other files), and it filters `NOSONAR` and
  `//nolint` itself. A directory with a file that failed to parse is skipped by the unused check,
  and Go files of the directory that weren't analyzed (generated, excluded) are scanned as text
  before a declaration is reported unused.
- Cross-file rules are listed in `gopkg::IMPLEMENTED` and have catalog entries in
  `catalog/go_extra.json` but no `rules/ugNNNN.rs` file (UG1056 unused declarations, UG1057
  unchecked errors). `gopkg::EXTENDS` lists per-file rules the pass extends under their own key
  (UG1018: locks held through types declared in other files); it must not report what the per-file
  rule already reports.
- Test them in `gopkg.rs` with several synthetic files (`uc_go::analyze` each, then `run`).

Taint rules (UG1060–UG1065) share one intra-procedural analysis, `crates/uc-go/src/taint.rs`,
computed once per file on first use (`taint::report(ctx, kind, message)`). Sinks that the
syntactic rules UG1008 and UG1009 report too are dropped from UG1060/UG1061 when both are on.
Rules that depend on the module's Go version read `ctx.file.go_version` (the `go` directive of
the nearest `go.mod`, `None` when unknown) and are tested with `check_version`.
