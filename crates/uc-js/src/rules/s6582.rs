//! Optional chaining should be preferred
//!
//! Covers the core `typescript-eslint/prefer-optional-chain` patterns: `a && a.b && a.b.c` and
//! `!a || !a.b`, where each operand extends the previous one with property accesses or calls.
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const MESSAGE: &str =
    "Prefer using an optional chain expression instead, as it's more concise and easier to read.";

/// A side-effect free access path: identifiers, `this`, static / literal member accesses.
fn is_pure_path(e: &Expression<'_>) -> bool {
    match strip_parens(e) {
        Expression::Identifier(_) | Expression::ThisExpression(_) => true,
        Expression::StaticMemberExpression(m) => !m.optional && is_pure_path(&m.object),
        Expression::PrivateFieldExpression(m) => !m.optional && is_pure_path(&m.object),
        Expression::ComputedMemberExpression(m) => {
            !m.optional
                && matches!(
                    strip_parens(&m.expression),
                    Expression::StringLiteral(_)
                        | Expression::NumericLiteral(_)
                        | Expression::Identifier(_)
                )
                && is_pure_path(&m.object)
        }
        _ => false,
    }
}

/// The operand can never be falsy (typescript-eslint only accepts operands whose type may be
/// nullish or a falsy-able primitive).
///
/// Only applied to JavaScript: SonarJS type-checks JS with strict null checks (JSDoc types are
/// non-nullable), while TypeScript projects often disable them, making every type nullable.
fn never_falsy(ctx: &Ctx<'_, '_>, e: &Expression<'_>) -> bool {
    if ctx.is_ts() {
        return false;
    }
    if type_of(ctx, e).is_object_like() {
        return true;
    }
    let Expression::Identifier(id) = strip_parens(e) else {
        return false;
    };
    let Some(sym) = ctx.symbol_of(id) else {
        return false;
    };
    if ctx.scoping().symbol_is_mutated(sym) {
        return false;
    }
    match ctx.symbol_decl(sym).kind() {
        AstKind::VariableDeclarator(v) => matches!(
            v.init.as_ref().map(strip_parens),
            Some(Expression::CallExpression(c)) if matches!(&c.callee, Expression::Identifier(r) if r.name == "require")
        ),
        AstKind::ImportNamespaceSpecifier(_) => true,
        AstKind::FormalParameter(_) => {
            let decl = ctx.symbol_decl(sym).id();
            let func = ctx.parent(ctx.parent(decl).id());
            helpers::batch_smell3::function_jsdoc(ctx, func.id())
                .is_some_and(|doc| jsdoc_param_is_object(doc, &id.name))
        }
        _ => false,
    }
}

/// `@param {T} name` where `T` is a non-nullable object type (`object`, `Function`, `Foo[]`,
/// `Array.<T>`, a class name, ...).
fn jsdoc_param_is_object(doc: &str, name: &str) -> bool {
    let mut rest = doc;
    while let Some(i) = rest.find("@param") {
        rest = &rest[i + 6..];
        let s = rest.trim_start();
        let Some(s) = s.strip_prefix('{') else {
            continue;
        };
        let Some(end) = s.find('}') else { return false };
        let ty = s[..end].trim();
        let after = s[end + 1..].trim_start();
        let pname: &str = after
            .split(|c: char| c.is_whitespace())
            .next()
            .unwrap_or("");
        if pname != name {
            continue;
        }
        if ty.is_empty() || ty.contains(['|', '?', '*', '=', '(']) || ty.starts_with('!') {
            return false;
        }
        let base = ty.split(['.', '<', '[']).next().unwrap_or("");
        return ty.ends_with("[]")
            || matches!(base, "object" | "function")
            || (base.starts_with(|c: char| c.is_ascii_uppercase())
                && !matches!(base, "String" | "Number" | "Boolean" | "Symbol" | "BigInt"));
    }
    false
}

/// True if `b` is `a` followed by at least one member access or call.
fn extends(ctx: &Ctx<'_, '_>, a: &Expression<'_>, b: &Expression<'_>) -> bool {
    let a_span = strip_parens(a).span();
    let mut cur = strip_parens(b);
    let mut steps = 0;
    loop {
        if steps > 0 && ctx.same_tokens(cur.span(), a_span) {
            return true;
        }
        cur = match cur {
            Expression::StaticMemberExpression(m) if !m.optional => strip_parens(&m.object),
            Expression::PrivateFieldExpression(m) if !m.optional => strip_parens(&m.object),
            Expression::ComputedMemberExpression(m) if !m.optional => strip_parens(&m.object),
            Expression::CallExpression(c) if !c.optional => strip_parens(&c.callee),
            _ => return false,
        };
        steps += 1;
    }
}

/// Operands of a left-associated chain of the same logical operator.
fn flatten<'b, 'a>(e: &'b Expression<'a>, op: LogicalOperator, out: &mut Vec<&'b Expression<'a>>) {
    match strip_parens(e) {
        Expression::LogicalExpression(l) if l.operator == op => {
            flatten(&l.left, op, out);
            flatten(&l.right, op, out);
        }
        _ => out.push(e),
    }
}

/// The operand as a checked path: `x` for `&&`, `!x` → `x` for `||`.
fn subject<'b, 'a>(e: &'b Expression<'a>, op: LogicalOperator) -> Option<&'b Expression<'a>> {
    let e = strip_parens(e);
    match op {
        LogicalOperator::And => Some(e),
        LogicalOperator::Or => match e {
            Expression::UnaryExpression(u) if u.operator == UnaryOperator::LogicalNot => {
                Some(strip_parens(&u.argument))
            }
            _ => None,
        },
        LogicalOperator::Coalesce => None,
    }
}

/// Typed contexts where `a?.b`'s `undefined` may not be accepted (Sonar's type-based exclusions).
fn in_typed_context(ctx: &Ctx<'_, '_>, node: &AstNode<'_>) -> bool {
    if !ctx.is_ts() {
        return false;
    }
    let parent = ctx.parent(node.id());
    match parent.kind() {
        AstKind::ReturnStatement(_) => {
            let Some(f) = helpers::batch_smell3::enclosing_function(ctx, node.id()) else {
                return false;
            };
            let ret = match f.kind() {
                AstKind::Function(f) => f.return_type.as_ref(),
                AstKind::ArrowFunctionExpression(a) => a.return_type.as_ref(),
                _ => None,
            };
            ret.is_some_and(|r| !accepts_undefined(&r.type_annotation))
        }
        AstKind::VariableDeclarator(v) => v
            .type_annotation
            .as_ref()
            .is_some_and(|a| !accepts_undefined(&a.type_annotation)),
        AstKind::ObjectProperty(_) | AstKind::CallExpression(_) | AstKind::NewExpression(_) => true,
        AstKind::AssignmentExpression(a) => a.operator == AssignmentOperator::Assign,
        _ => false,
    }
}

fn accepts_undefined(t: &TSType<'_>) -> bool {
    match t {
        TSType::TSUndefinedKeyword(_)
        | TSType::TSAnyKeyword(_)
        | TSType::TSUnknownKeyword(_)
        | TSType::TSVoidKeyword(_) => true,
        TSType::TSUnionType(u) => u.types.iter().any(accepts_undefined),
        TSType::TSParenthesizedType(p) => accepts_undefined(&p.type_annotation),
        _ => false,
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::LogicalExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        // The upstream rule needs a TypeScript program (absent for HTML / Vue scripts).
        if !helpers::batch_smell3::has_type_services(ctx)
            || !helpers::batch_smell3::es_version_allows(ctx, 2020, true)
        {
            return;
        }
        let AstKind::LogicalExpression(l) = node.kind() else {
            return;
        };
        let op = l.operator;
        if op == LogicalOperator::Coalesce {
            return;
        }
        // Only handle the outermost expression of a chain.
        let mut p = ctx.parent(node.id());
        while let AstKind::ParenthesizedExpression(_) = p.kind() {
            p = ctx.parent(p.id());
        }
        if matches!(p.kind(), AstKind::LogicalExpression(pl) if pl.operator == op) {
            return;
        }
        let mut operands = Vec::new();
        flatten(&l.left, op, &mut operands);
        flatten(&l.right, op, &mut operands);
        let subjects: Vec<Option<&Expression<'a>>> =
            operands.iter().map(|e| subject(e, op)).collect();
        let mut i = 0;
        while i + 1 < operands.len() {
            let Some(first) = subjects[i].filter(|s| is_pure_path(s) && !never_falsy(ctx, s))
            else {
                i += 1;
                continue;
            };
            let mut end = i;
            let mut prev = first;
            while end + 1 < operands.len() {
                match subjects[end + 1] {
                    Some(next) if extends(ctx, prev, next) => {
                        prev = next;
                        end += 1;
                    }
                    _ => break,
                }
            }
            if end > i {
                let whole = end == operands.len() - 1 && i == 0;
                if !(whole && op == LogicalOperator::And && in_typed_context(ctx, node)) {
                    let start = strip_parens(operands[i]).span().start;
                    let stop = operands[end].span().end;
                    ctx.report(Span::new(start, stop), MESSAGE);
                }
                i = end + 1;
            } else {
                i += 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn optional_chain() {
        check(
            "S6582",
            "a.js",
            r#"
if (foo && foo.bar) {}                   // Noncompliant {{Prefer using an optional chain expression instead, as it's more concise and easier to read.}}
if (foo && foo.bar && foo.bar.baz) {}    // Noncompliant
if (foo && foo.bar()) {}                 // Noncompliant
if (!foo || !foo.bar) {}                 // Noncompliant
if (foo && bar) {}
if (foo && foo.bar && other) {}          // Noncompliant
if (foo.bar && foo.baz) {}
if (foo() && foo().bar) {}
if (foo?.bar) {}
const x = a && a.b && b && b.c;          // Noncompliant 2
"#,
        );
    }

    #[test]
    fn typed_contexts() {
        check(
            "S6582",
            "a.ts",
            r#"
function f(a: { b: string } | null): string | null {
  return a && a.b;
}
function g(a: { b: string } | null) {
  if (a && a.b) {}                       // Noncompliant
  foo(a && a.b);
}
"#,
        );
    }
}
