# Writing JS/TS rules

Rules live in `crates/uc-js/src/rules/sNNNN.rs`, one file per Sonar rule key. `build.rs` discovers
them automatically: there is no registry to edit. The rule key comes from the file name
(`s1067.rs` → `S1067`) and must exist in `catalog/js.json`.

## Shape of a rule

```rust
//! <rule title from the catalog>
use crate::prelude::*;

pub struct Rule {
    max: u32,
}

impl Rule {
    pub fn new(p: &Params) -> Self {
        Self { max: p.u32("max", 3) } // default = Sonar's default
    }
}

impl JsRule for Rule {
    /// Node types `run` is called for. The engine walks the tree once and dispatches.
    fn interests(&self) -> &'static [AstType] {
        &[AstType::IfStatement]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::IfStatement(stmt) = node.kind() else { return };
        // ...
        ctx.report(stmt.span, "message");
    }

    /// Optional: whole-file checks (comments, tokens, lines, cross-node aggregation).
    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {}
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn rule() {
        check("S1234", "file.ts", r#"
if (a) {}   // Noncompliant {{message}}
if (b) { foo(); }
"#);
    }
}
```

Rules must be stateless between files (`Send + Sync`). Per-file state goes in local variables of
`run_once`, which can scan `ctx.nodes().iter()` itself.

## Context (`Ctx`) and helpers

- Reporting: `ctx.report(span, msg)`, `ctx.report_with(span, msg, [(span, Some(msg))])` for
  secondary locations, `ctx.report_gap(span, msg, gap)` for linear remediation cost.
- Tree: `ctx.parent(id)`, `ctx.parent_kind(id)`, `ctx.parent_skip_parens(id)`,
  `ctx.ancestors(id)`, `ctx.enclosing_function(id)`, `ctx.node(id)`, `ctx.nodes()`.
- Symbols: `ctx.symbol_of(&IdentifierReference)`, `ctx.is_global(ident, "name")`,
  `ctx.symbol_decl(sym)`, `ctx.scoping()` (oxc `Scoping`: `get_resolved_references(sym)`,
  `symbol_is_mutated`, `symbol_name`, `symbol_flags`, `iter_bindings_in`, …), `ctx.semantic`.
- Text and tokens: `ctx.text(span)`, `ctx.text_of(&node)`, `ctx.tokens` (no comments),
  `ctx.tokens_in(span)`, `ctx.same_tokens(a, b)` (syntactic equivalence), `ctx.line(offset)`,
  `ctx.lines` (`LineIndex`: `line_text`, `line_count`, …), `ctx.comments()`, `ctx.comment_text(c)`.
- File: `ctx.is_ts()`, `ctx.is_jsx()`, `ctx.is_test()`, `ctx.file.path`.
- `helpers::ast`: `strip_parens`, `strip_ts`, `dotted_path`, `callee_path`, `callee_name`,
  `property_key_name`, `string_value`, `is_undefined`, `is_null`, `find_token`, `token_after`,
  `token_before`, `function_report_span` (Sonar's location for function issues), `is_function`.
- `helpers::types`: `type_of(ctx, expr) -> Ty` with `is_string()`, `is_number()`, `is_array()`,
  `is_promise()`, `is_primitive()`, `is_object_like()`, `may_be_nullish()`, …; `symbol_type`,
  `from_annotation`. **`Ty::Unknown` means "can't tell": a type-dependent rule must stay silent
  then.** Precision first: a missed issue is acceptable, a false positive is not.
- `helpers::cognitive`: cognitive complexity points.
- Your batch owns `helpers/batch_<name>.rs`: put shared helpers for your rules there. Do not edit
  other shared files (`ctx.rs`, `helpers/ast.rs`, `helpers/types.rs`, `lib.rs`, …). If you need
  something there, implement it locally in your batch module.

The oxc AST (v0.154) has changed a lot across versions, so don't rely on memory: read the source in
`~/.cargo/registry/src/index.crates.io-*/`:
`oxc_ast-0.154.0/src/ast/{js,ts,jsx,literal}.rs` (node structs),
`oxc_ast-0.154.0/src/generated/ast_kind.rs` (`AstKind` / `AstType` variants),
`oxc_ast-0.154.0/src/ast_impl/` (helper methods),
`oxc_semantic-0.154.0/src/` (`Semantic`, `Scoping`, `AstNodes`),
`oxc_regular_expression-0.154.0` (regex AST: `RegExpLiteral.regex.pattern`),
`oxc_ecmascript-0.154.0` (constant evaluation helpers).
Notable details: parentheses are kept as `ParenthesizedExpression` nodes; `type_annotation` lives
on `VariableDeclarator` / `FormalParameter`; arrow bodies are `ArrowFunctionBody`
(`as_function_body()` / `as_expression()`); `.js` files are parsed with JSX enabled.

## Behaviour and parity

Each rule must behave like the Sonar rule with the same key: same triggers, same default
parameters, same primary location (line matters for parity checks), same message text.
The spec for each rule is its description (`<key>.html` under the SonarJS checkout listed in your
batch file). You may read the SonarJS rule directory to understand edge cases and messages, but
write your own implementation and your own tests: do not port their code line by line. Rules
derived from ESLint core or plugin rules (MIT) may follow those implementations' behaviour.

Rules run on code SonarJS considers in scope: `catalog/js.json` says which languages (`langs`) and
whether main or test code (`scope`); the engine filters by those, so the rule itself doesn't need to.

## Performance

This tool must be 10–100× faster than SonarQube. In rules:
- Subscribe to the narrowest node types. Never scan all nodes from `run`.
- No allocation on the hot path where avoidable; compile regexes once in `new()`.
- No quadratic algorithms over the file; use hash maps.
- Prefer cheap syntactic checks before semantic lookups.

## Tests

Every rule file has a `#[cfg(test)]` module with compliant and noncompliant cases, using
`check` / `check_with(key, file, json!({...}), code)` from `test_util`:
`// Noncompliant`, `// Noncompliant 2`, `// Noncompliant@+1`, `// Noncompliant {{exact message}}`.
Use `.ts`/`.tsx` file names for TypeScript, `.js`/`.jsx` for JavaScript. Run one rule's tests
with `cargo test -p uc-js rules::s1234`.
