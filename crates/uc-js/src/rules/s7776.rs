//! Arrays used only for existence checks should be Sets
use crate::helpers::batch_unicorn::{
    ancestors, const_init, is_method_call, is_static_call, paren_parent,
};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const RETURNS_ARRAY: &[&str] = &[
    "copyWithin",
    "fill",
    "filter",
    "flat",
    "flatMap",
    "map",
    "reverse",
    "sort",
    "splice",
    "toReversed",
    "toSorted",
    "toSpliced",
    "with",
    "split",
    "toArray",
];

fn is_string_literal(e: &Expression<'_>) -> bool {
    match strip_parens(e) {
        Expression::StringLiteral(_) => true,
        Expression::TemplateLiteral(t) => t.expressions.is_empty(),
        _ => false,
    }
}

fn is_array_init(ctx: &Ctx<'_, '_>, e: &Expression<'_>, depth: u8) -> bool {
    if depth > 4 {
        return false;
    }
    match strip_parens(e) {
        Expression::ArrayExpression(_) => true,
        Expression::NewExpression(n) => {
            matches!(&n.callee, Expression::Identifier(id) if id.name == "Array")
        }
        Expression::CallExpression(c) => {
            if !c.optional && matches!(&c.callee, Expression::Identifier(id) if id.name == "Array")
            {
                return true;
            }
            if is_static_call(c, &["Array"], &["from", "of"], None)
                || is_method_call(c, RETURNS_ARRAY, None, false).is_some()
            {
                return true;
            }
            if let Some(m) = is_method_call(c, &["slice", "concat"], None, false) {
                return !is_string_literal(&m.object)
                    && (!matches!(strip_parens(&m.object), Expression::Identifier(_))
                        || const_init(ctx, &m.object)
                            .is_some_and(|i| is_array_init(ctx, i, depth + 1)));
            }
            false
        }
        _ => false,
    }
}

/// Array literal of unique static primitives.
fn is_known_unique_array(e: &Expression<'_>) -> bool {
    let Expression::ArrayExpression(a) = strip_parens(e) else {
        return false;
    };
    let mut seen: FxHashSet<String> = FxHashSet::default();
    for el in &a.elements {
        let key = match el.as_expression().map(strip_parens) {
            Some(Expression::StringLiteral(s)) => format!("s{}", s.value),
            Some(Expression::NumericLiteral(n))
                if !(n.value == 0.0 && n.value.is_sign_negative()) =>
            {
                format!("n{}", n.value)
            }
            Some(Expression::BooleanLiteral(b)) => format!("b{}", b.value),
            Some(Expression::NullLiteral(_)) => "null".to_string(),
            _ => return false,
        };
        if !seen.insert(key) {
            return false;
        }
    }
    true
}

enum Use {
    Includes(NodeId),
    Extra,
}

fn classify(ctx: &Ctx<'_, '_>, ident: &AstNode<'_>) -> Option<Use> {
    let span = ident.kind().span();
    let parent = paren_parent(ctx, ident.id());
    match parent.kind() {
        AstKind::StaticMemberExpression(m) if m.object.span() == span => {
            let grand = ctx.parent(parent.id());
            match m.property.name.as_str() {
                "includes" => match grand.kind() {
                    AstKind::CallExpression(c)
                        if c.callee.span() == m.span && is_method_call(c, &["includes"], Some(1), false).is_some() =>
                    {
                        Some(Use::Includes(grand.id()))
                    }
                    _ => None,
                },
                "length" if !m.optional => match grand.kind() {
                    AstKind::CallExpression(c) if c.callee.span() == m.span => None,
                    AstKind::AssignmentExpression(_)
                    | AstKind::UpdateExpression(_)
                    | AstKind::ArrayAssignmentTarget(_)
                    | AstKind::AssignmentTargetPropertyProperty(_)
                    | AstKind::AssignmentTargetWithDefault(_)
                    | AstKind::ForInStatement(_)
                    | AstKind::ForOfStatement(_)
                    | AstKind::TSAsExpression(_)
                    | AstKind::TSNonNullExpression(_)
                    | AstKind::TSTypeAssertion(_) => None,
                    _ => Some(Use::Extra),
                },
                "forEach" => match grand.kind() {
                    AstKind::CallExpression(c)
                        if c.callee.span() == m.span && is_method_call(c, &["forEach"], Some(1), false).is_some() =>
                    {
                        matches!(c.arguments[0].as_expression(), Some(Expression::ArrowFunctionExpression(a))
                            if a.params.items.len() == 1 && a.params.rest.is_none())
                        .then_some(Use::Extra)
                    }
                    _ => None,
                },
                _ => None,
            }
        }
        AstKind::ForOfStatement(f) if f.right.span() == span => Some(Use::Extra),
        AstKind::SpreadElement(_) => matches!(
            ctx.parent_kind(parent.id()),
            AstKind::ArrayExpression(_) | AstKind::CallExpression(_) | AstKind::NewExpression(_)
        )
        .then_some(Use::Extra),
        _ => None,
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::VariableDeclarator]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::VariableDeclarator(d) = node.kind() else {
            return;
        };
        let BindingPattern::BindingIdentifier(id) = &d.id else {
            return;
        };
        let Some(init) = &d.init else { return };
        let decl = ctx.parent(node.id());
        if !matches!(decl.kind(), AstKind::VariableDeclaration(_))
            || matches!(ctx.parent_kind(decl.id()), AstKind::ExportDeclaration(_))
        {
            return;
        }
        if !is_array_init(ctx, init, 0) {
            return;
        }
        let Some(sym) = id.symbol_id.get() else {
            return;
        };
        let mut includes = Vec::new();
        let mut extra = 0;
        for r in ctx.scoping().get_resolved_references(sym) {
            let Some(u) = classify(ctx, ctx.node(r.node_id())) else {
                return;
            };
            match u {
                Use::Includes(call) => includes.push(call),
                Use::Extra => extra += 1,
            }
        }
        if includes.is_empty() || (extra > 0 && !is_known_unique_array(init)) {
            return;
        }
        if includes.len() == 1 {
            // A single check is fine unless it runs repeatedly (in a loop or a function).
            let root = ctx.parent(decl.id()).id();
            let repeated = ancestors(ctx, includes[0])
                .take_while(|a| a.id() != root)
                .any(|a| {
                    matches!(
                        a.kind(),
                        AstKind::ForOfStatement(_)
                            | AstKind::ForStatement(_)
                            | AstKind::ForInStatement(_)
                            | AstKind::WhileStatement(_)
                            | AstKind::DoWhileStatement(_)
                            | AstKind::Function(_)
                            | AstKind::ArrowFunctionExpression(_)
                    )
                });
            if !repeated {
                return;
            }
        }
        let name = id.name.as_str();
        ctx.report(id.span, format!("`{name}` should be a `Set`, and use `{name}.has()` to check existence or non-existence."));
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn set_has() {
        check(
            "S7776",
            "a.js",
            r#"
const array = [1, 2, 3]; // Noncompliant {{`array` should be a `Set`, and use `array.has()` to check existence or non-existence.}}
const hasValue = value => array.includes(value);
const list = [1, 2];
const once = list.includes(1);
const names = users.map(u => u.name); // Noncompliant
for (const u of others) { if (names.includes(u)) {} }
const known = ['a', 'b']; // Noncompliant
function f(x) { return known.includes(x) && known.length > 1; }
const dup = ['a', 'a'];
function g(x) { return dup.includes(x) && dup.length > 1; }
const mixed = [1, 2];
function h(x) { mixed.push(x); return mixed.includes(x); }
const str = 'abc'.slice(1);
function i(x) { return str.includes(x); }
"#,
        );
    }
}
