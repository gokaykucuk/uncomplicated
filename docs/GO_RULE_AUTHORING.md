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
