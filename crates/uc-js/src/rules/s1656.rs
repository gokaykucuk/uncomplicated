//! Variables should not be self-assigned
use crate::helpers::batch_bug1::{static_expr_name, static_key_name};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// A view of a reference-like node, the way ESLint's `isSameReference` sees it.
enum Ref<'b, 'a> {
    Super,
    This,
    Ident(&'a str),
    Lit(&'b Expression<'a>),
    Member {
        computed: bool,
        object: &'b Expression<'a>,
        prop: Prop<'b, 'a>,
        static_name: Option<String>,
    },
    Other,
}

enum Prop<'b, 'a> {
    Name(&'a str),
    Private(&'a str),
    Expr(&'b Expression<'a>),
}

fn member_ref<'b, 'a>(m: &'b MemberExpression<'a>) -> Ref<'b, 'a> {
    match m {
        MemberExpression::StaticMemberExpression(s) => Ref::Member {
            computed: false,
            object: &s.object,
            prop: Prop::Name(s.property.name.as_str()),
            static_name: Some(s.property.name.to_string()),
        },
        MemberExpression::ComputedMemberExpression(c) => Ref::Member {
            computed: true,
            object: &c.object,
            prop: Prop::Expr(&c.expression),
            static_name: static_expr_name(&c.expression),
        },
        MemberExpression::PrivateFieldExpression(p) => Ref::Member {
            computed: false,
            object: &p.object,
            prop: Prop::Private(p.field.name.as_str()),
            static_name: None,
        },
    }
}

fn expr_ref<'b, 'a>(e: &'b Expression<'a>) -> Ref<'b, 'a> {
    let e = strip_parens(e);
    match e {
        Expression::Super(_) => Ref::Super,
        Expression::ThisExpression(_) => Ref::This,
        Expression::Identifier(id) => Ref::Ident(id.name.as_str()),
        Expression::StringLiteral(_)
        | Expression::NumericLiteral(_)
        | Expression::BooleanLiteral(_)
        | Expression::NullLiteral(_)
        | Expression::BigIntLiteral(_)
        | Expression::RegExpLiteral(_) => Ref::Lit(e),
        Expression::ChainExpression(c) => match c.expression.as_member_expression() {
            Some(m) => member_ref(m),
            None => Ref::Other,
        },
        _ => match e.as_member_expression() {
            Some(m) => member_ref(m),
            None => Ref::Other,
        },
    }
}

fn same_literal(a: &Expression<'_>, b: &Expression<'_>) -> bool {
    match (a, b) {
        (Expression::StringLiteral(x), Expression::StringLiteral(y)) => x.value == y.value,
        (Expression::NumericLiteral(x), Expression::NumericLiteral(y)) => x.value == y.value,
        (Expression::BooleanLiteral(x), Expression::BooleanLiteral(y)) => x.value == y.value,
        (Expression::NullLiteral(_), Expression::NullLiteral(_)) => true,
        (Expression::BigIntLiteral(x), Expression::BigIntLiteral(y)) => x.value == y.value,
        (Expression::RegExpLiteral(x), Expression::RegExpLiteral(y)) => {
            x.regex.pattern.text == y.regex.pattern.text && x.regex.flags == y.regex.flags
        }
        _ => false,
    }
}

fn same_prop(a: &Prop<'_, '_>, b: &Prop<'_, '_>) -> bool {
    match (a, b) {
        (Prop::Name(x), Prop::Name(y)) | (Prop::Private(x), Prop::Private(y)) => x == y,
        (Prop::Name(x), Prop::Expr(e)) | (Prop::Expr(e), Prop::Name(x)) => {
            matches!(strip_parens(e), Expression::Identifier(id) if id.name == *x)
        }
        (Prop::Expr(x), Prop::Expr(y)) => same_ref(&expr_ref(x), &expr_ref(y)),
        _ => false,
    }
}

fn same_ref(a: &Ref<'_, '_>, b: &Ref<'_, '_>) -> bool {
    match (a, b) {
        (Ref::Super, Ref::Super) | (Ref::This, Ref::This) => true,
        (Ref::Ident(x), Ref::Ident(y)) => x == y,
        (Ref::Lit(x), Ref::Lit(y)) => same_literal(x, y),
        (
            Ref::Member {
                computed: c1,
                object: o1,
                prop: p1,
                static_name: n1,
            },
            Ref::Member {
                computed: c2,
                object: o2,
                prop: p2,
                static_name: n2,
            },
        ) => {
            if let Some(n1) = n1 {
                return same_ref(&expr_ref(o1), &expr_ref(o2))
                    && n2.as_deref() == Some(n1.as_str());
            }
            c1 == c2 && same_ref(&expr_ref(o1), &expr_ref(o2)) && same_prop(p1, p2)
        }
        _ => false,
    }
}

/// A node on the left of the assignment, mirroring ESTree pattern node types.
enum Left<'b, 'a> {
    Target(&'b AssignmentTarget<'a>),
    Rest(&'b AssignmentTargetRest<'a>),
    Other,
}

fn maybe_default<'b, 'a>(t: &'b AssignmentTargetMaybeDefault<'a>) -> Left<'b, 'a> {
    match t.as_assignment_target() {
        Some(t) => Left::Target(t),
        None => Left::Other,
    }
}

impl Rule {
    fn each<'a>(&self, ctx: &Ctx<'a, '_>, left: Left<'_, 'a>, right: &Expression<'a>) {
        let right = strip_parens(right);
        match left {
            Left::Rest(_) | Left::Other => {}
            Left::Target(t) => match t {
                AssignmentTarget::AssignmentTargetIdentifier(l) => {
                    if let Expression::Identifier(r) = right
                        && l.name == r.name
                    {
                        self.report(ctx, right);
                    }
                }
                AssignmentTarget::ArrayAssignmentTarget(arr) => {
                    let Expression::ArrayExpression(rarr) = right else {
                        return;
                    };
                    let mut lefts: Vec<Left<'_, 'a>> = arr
                        .elements
                        .iter()
                        .map(|e| match e {
                            Some(e) => maybe_default(e),
                            None => Left::Other,
                        })
                        .collect();
                    if let Some(rest) = &arr.rest {
                        lefts.push(Left::Rest(rest));
                    }
                    let n_right = rarr.elements.len();
                    let end = lefts.len().min(n_right);
                    for (i, l) in lefts.into_iter().take(end).enumerate() {
                        let is_rest = matches!(l, Left::Rest(_));
                        if is_rest && i < n_right - 1 {
                            break;
                        }
                        let r = &rarr.elements[i];
                        match (l, r) {
                            (Left::Rest(rest), ArrayExpressionElement::SpreadElement(s)) => {
                                self.each(ctx, Left::Target(&rest.target), &s.argument);
                            }
                            (l, r) => {
                                if let Some(re) = r.as_expression() {
                                    self.each(ctx, l, re);
                                }
                            }
                        }
                        if matches!(r, ArrayExpressionElement::SpreadElement(_)) {
                            break;
                        }
                    }
                }
                AssignmentTarget::ObjectAssignmentTarget(obj) => {
                    let Expression::ObjectExpression(robj) = right else {
                        return;
                    };
                    if robj.properties.is_empty() {
                        return;
                    }
                    let start_j = robj
                        .properties
                        .iter()
                        .rposition(|p| matches!(p, ObjectPropertyKind::SpreadProperty(_)))
                        .map_or(0, |i| i + 1);
                    for lp in &obj.properties {
                        let lname = match lp {
                            AssignmentTargetProperty::AssignmentTargetPropertyIdentifier(p) => {
                                Some(p.binding.name.to_string())
                            }
                            AssignmentTargetProperty::AssignmentTargetPropertyProperty(p) => {
                                static_key_name(&p.name)
                            }
                        };
                        let Some(lname) = lname else { continue };
                        for rp in &robj.properties[start_j..] {
                            let ObjectPropertyKind::ObjectProperty(rp) = rp else {
                                continue;
                            };
                            if rp.kind != PropertyKind::Init || rp.method {
                                continue;
                            }
                            if static_key_name(&rp.key).as_deref() != Some(lname.as_str()) {
                                continue;
                            }
                            match lp {
                                AssignmentTargetProperty::AssignmentTargetPropertyIdentifier(p) => {
                                    if p.init.is_none()
                                        && let Expression::Identifier(r) = strip_parens(&rp.value)
                                        && r.name == p.binding.name
                                    {
                                        self.report(ctx, strip_parens(&rp.value));
                                    }
                                }
                                AssignmentTargetProperty::AssignmentTargetPropertyProperty(p) => {
                                    self.each(ctx, maybe_default(&p.binding), &rp.value);
                                }
                            }
                        }
                    }
                }
                _ => {
                    let Some(lm) = t.as_member_expression() else {
                        return;
                    };
                    let rref = expr_ref(right);
                    if matches!(rref, Ref::Member { .. }) && same_ref(&member_ref(lm), &rref) {
                        self.report(ctx, right);
                    }
                }
            },
        }
    }

    fn report(&self, ctx: &Ctx<'_, '_>, right: &Expression<'_>) {
        ctx.report(
            right.span(),
            format!("'{}' is assigned to itself.", ctx.text(right.span())),
        );
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::AssignmentExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::AssignmentExpression(a) = node.kind() else {
            return;
        };
        if matches!(
            a.operator,
            AssignmentOperator::Assign
                | AssignmentOperator::LogicalAnd
                | AssignmentOperator::LogicalOr
                | AssignmentOperator::LogicalNullish
        ) {
            self.each(ctx, Left::Target(&a.left), &a.right);
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn self_assign() {
        check(
            "S1656",
            "a.js",
            r#"
foo = foo; // Noncompliant {{'foo' is assigned to itself.}}
[a, b] = [a, b]; // Noncompliant 2
[a, ...b] = [x, ...b]; // Noncompliant
({ a, b } = { a, x }); // Noncompliant
({ a: c } = { a: c }); // Noncompliant
foo &&= foo; // Noncompliant
obj.a = obj.a; // Noncompliant {{'obj.a' is assigned to itself.}}
obj.a.b = obj.a.b; // Noncompliant
obj["a"] = obj.a; // Noncompliant
obj[a] = obj[a]; // Noncompliant
this.x = this.x; // Noncompliant
foo = (foo); // Noncompliant
foo = +foo;
foo += foo;
[foo = 1] = [foo];
[a, b] = [b, a];
[...a] = [...a, 1];
({ a = 1 } = { a });
({ ...a } = { ...a });
obj.a().b = obj.a().b;
a().b = a().b;
obj[a] = obj[b];
obj.a = obj.b;
let x = x;
"#,
        );
    }
}
