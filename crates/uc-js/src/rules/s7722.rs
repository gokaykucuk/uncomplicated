//! Built-in error objects should have meaningful messages
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const BUILTIN_ERRORS: &[&str] = &[
    "Error",
    "EvalError",
    "RangeError",
    "ReferenceError",
    "SyntaxError",
    "TypeError",
    "URIError",
    "InternalError",
    "AggregateError",
    "SuppressedError",
];

enum Static {
    Str(bool),
    NotString,
    Unknown,
}

/// Statically known value of a message argument.
fn static_value(ctx: &Ctx<'_, '_>, e: &Expression<'_>, depth: u8) -> Static {
    match strip_parens(e) {
        Expression::StringLiteral(s) => Static::Str(s.value.is_empty()),
        Expression::TemplateLiteral(t) if t.expressions.is_empty() => Static::Str(
            t.quasis
                .iter()
                .all(|q| q.value.cooked.is_some_and(|c| c.is_empty())),
        ),
        Expression::NumericLiteral(_)
        | Expression::BooleanLiteral(_)
        | Expression::NullLiteral(_)
        | Expression::BigIntLiteral(_)
        | Expression::ArrayExpression(_)
        | Expression::ObjectExpression(_) => Static::NotString,
        e if is_undefined(ctx, e) => Static::NotString,
        Expression::Identifier(id) if depth < 3 => {
            let Some(sym) = ctx.symbol_of(id) else {
                return Static::Unknown;
            };
            let decl = ctx.symbol_decl(sym);
            let AstKind::VariableDeclarator(v) = decl.kind() else {
                return Static::Unknown;
            };
            let is_const = matches!(ctx.parent_kind(decl.id()),
                AstKind::VariableDeclaration(d) if d.kind == VariableDeclarationKind::Const);
            match (&v.id, &v.init) {
                (BindingPattern::BindingIdentifier(_), Some(init)) if is_const => {
                    static_value(ctx, init, depth + 1)
                }
                _ => Static::Unknown,
            }
        }
        _ => Static::Unknown,
    }
}

fn is_stack_property(kind: &AstKind<'_>) -> bool {
    match kind {
        AstKind::StaticMemberExpression(m) => m.property.name == "stack",
        AstKind::ComputedMemberExpression(m) => {
            matches!(&m.expression, Expression::StringLiteral(s) if s.value == "stack")
        }
        _ => false,
    }
}

/// `member` is `<x>.stack` used as a plain read.
fn is_stack_read(ctx: &Ctx<'_, '_>, member: &AstNode<'_>) -> bool {
    if !is_stack_property(&member.kind()) {
        return false;
    }
    let span = member.kind().span();
    let parent = ctx.parent_skip_parens(member.id());
    match parent.kind() {
        AstKind::ExpressionStatement(_)
        | AstKind::VariableDeclarator(_)
        | AstKind::ReturnStatement(_) => true,
        AstKind::AssignmentExpression(a) => strip_parens(&a.right).span() == span,
        AstKind::ObjectProperty(p) => strip_parens(&p.value).span() == span,
        AstKind::CallExpression(_) => match ctx.parent_skip_parens(parent.id()).kind() {
            AstKind::ExpressionStatement(_) => true,
            AstKind::AwaitExpression(_) => {
                let aw = ctx.parent_skip_parens(parent.id());
                matches!(
                    ctx.parent_skip_parens(aw.id()).kind(),
                    AstKind::ExpressionStatement(_)
                )
            }
            _ => false,
        },
        _ => false,
    }
}

/// `new Error().stack` or `const err = new Error(); log(err.stack)`: capturing a stack trace.
fn is_stack_capture(ctx: &Ctx<'_, '_>, node: &AstNode<'_>) -> bool {
    let parent = ctx.parent_skip_parens(node.id());
    if is_stack_property(&parent.kind()) {
        let obj_span = match parent.kind() {
            AstKind::StaticMemberExpression(m) => strip_parens(&m.object).span(),
            AstKind::ComputedMemberExpression(m) => strip_parens(&m.object).span(),
            _ => return false,
        };
        return obj_span == node.kind().span() && is_stack_read(ctx, parent);
    }
    let AstKind::VariableDeclarator(v) = parent.kind() else {
        return false;
    };
    let BindingPattern::BindingIdentifier(b) = &v.id else {
        return false;
    };
    let Some(sym) = b.symbol_id.get() else {
        return false;
    };
    let scoping = ctx.scoping();
    if scoping.symbol_is_mutated(sym) || !scoping.symbol_redeclarations(sym).is_empty() {
        return false;
    }
    let mut reads = 0;
    for r in scoping.get_resolved_references(sym) {
        if !r.is_read() {
            continue;
        }
        reads += 1;
        let member = ctx.parent(r.node_id());
        if !is_stack_read(ctx, member) {
            return false;
        }
    }
    reads > 0
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::NewExpression, AstType::CallExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let (callee, args, span) = match node.kind() {
            AstKind::NewExpression(n) => (&n.callee, &n.arguments, n.span),
            AstKind::CallExpression(c) => (&c.callee, &c.arguments, c.span),
            _ => return,
        };
        let Expression::Identifier(id) = callee else {
            return;
        };
        let name = id.name.as_str();
        if !BUILTIN_ERRORS.contains(&name) || ctx.symbol_of(id).is_some() {
            return;
        }
        let index = match name {
            "AggregateError" => 1,
            "SuppressedError" => 2,
            _ => 0,
        };
        if args.iter().take(index + 1).any(|a| a.is_spread()) {
            return;
        }
        let Some(arg) = args.get(index).and_then(|a| a.as_expression()) else {
            if !is_stack_capture(ctx, node) {
                ctx.report(span, format!("Pass a message to the `{name}` constructor."));
            }
            return;
        };
        match static_value(ctx, arg, 0) {
            Static::Str(true) => {
                ctx.report(arg.span(), "Error message should not be an empty string.")
            }
            Static::NotString => ctx.report(arg.span(), "Error message should be a string."),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn rule() {
        check(
            "S7722",
            "a.js",
            r#"
throw new Error(); // Noncompliant {{Pass a message to the `Error` constructor.}}
throw TypeError(); // Noncompliant {{Pass a message to the `TypeError` constructor.}}
throw new Error(""); // Noncompliant {{Error message should not be an empty string.}}
throw new RangeError(42); // Noncompliant {{Error message should be a string.}}
throw new AggregateError(errors); // Noncompliant
const empty = '';
throw new Error(empty); // Noncompliant
new Error().stack = 'custom'; // Noncompliant
const err = new Error(); console.log(err.message); // Noncompliant
throw new Error('Something went wrong');
throw new Error(message);
throw new AggregateError(errors, 'many');
throw new Error(...args);
new Error().stack;
const stack = new Error().stack;
logger.debug('call site:', new Error().stack);
const err2 = new Error(); log(err2.stack);
function f(Error) { throw new Error(); }
"#,
        );
    }
}
