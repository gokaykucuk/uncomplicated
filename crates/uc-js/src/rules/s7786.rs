//! Generic "Error" should be "TypeError" when thrown after type checking
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const TYPE_CHECKS: &[&str] = &[
    "isArguments",
    "isArray",
    "isArrayBuffer",
    "isArrayLike",
    "isArrayLikeObject",
    "isBigInt",
    "isBoolean",
    "isBuffer",
    "isDate",
    "isElement",
    "isError",
    "isFinite",
    "isFunction",
    "isInteger",
    "isLength",
    "isMap",
    "isNaN",
    "isNative",
    "isNil",
    "isNull",
    "isNumber",
    "isObject",
    "isObjectLike",
    "isPlainObject",
    "isPrototypeOf",
    "isRegExp",
    "isSafeInteger",
    "isSet",
    "isString",
    "isSymbol",
    "isTypedArray",
    "isUndefined",
    "isView",
    "isWeakMap",
    "isWeakSet",
    "isWindow",
    "isXMLDoc",
];

/// `^(?:[A-Z][\da-z]*)*Error$`
fn is_error_name(name: &str) -> bool {
    let Some(prefix) = name.strip_suffix("Error") else {
        return false;
    };
    let b = prefix.as_bytes();
    if b.is_empty() {
        return true;
    }
    b[0].is_ascii_uppercase()
        && b.iter()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_lowercase() || c.is_ascii_digit())
}

fn is_error_constructor(e: &Expression<'_>) -> bool {
    match strip_parens(e) {
        Expression::Identifier(id) => is_error_name(&id.name),
        Expression::StaticMemberExpression(m) => !m.optional && is_error_name(&m.property.name),
        _ => false,
    }
}

/// `in_call`: the expression is the callee of a call with arguments.
fn is_type_check_member(m: &Expression<'_>, in_call: bool) -> bool {
    match strip_parens(m) {
        Expression::StaticMemberExpression(s) => {
            (in_call && TYPE_CHECKS.contains(&s.property.name.as_str()))
                || is_type_check_member(&s.object, in_call)
        }
        _ => false,
    }
}

fn is_type_check(e: &Expression<'_>, in_call: bool) -> bool {
    match strip_parens(e) {
        Expression::Identifier(id) => in_call && matches!(id.name.as_str(), "isNaN" | "isFinite"),
        m @ Expression::StaticMemberExpression(_) => is_type_check_member(m, in_call),
        Expression::CallExpression(c) => is_type_check(&c.callee, !c.arguments.is_empty()),
        Expression::UnaryExpression(u) => {
            u.operator == UnaryOperator::Typeof
                || (u.operator == UnaryOperator::LogicalNot && is_type_check(&u.argument, false))
        }
        Expression::BinaryExpression(b) => {
            if b.operator == BinaryOperator::Instanceof {
                return !is_error_constructor(&b.right);
            }
            is_type_check(&b.left, in_call) || is_type_check(&b.right, in_call)
        }
        Expression::LogicalExpression(l) => {
            is_type_check(&l.left, in_call) && is_type_check(&l.right, in_call)
        }
        _ => false,
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::ThrowStatement]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::ThrowStatement(t) = node.kind() else {
            return;
        };
        let Expression::NewExpression(n) = strip_parens(&t.argument) else {
            return;
        };
        let Expression::Identifier(callee) = &n.callee else {
            return;
        };
        if callee.name != "Error" {
            return;
        }
        let block = ctx.parent(node.id());
        let AstKind::BlockStatement(b) = block.kind() else {
            return;
        };
        if b.body.len() != 1 {
            return;
        }
        if let AstKind::IfStatement(s) = ctx.parent_kind(block.id())
            && is_type_check(&s.test, false)
        {
            ctx.report(
                callee.span,
                "`new Error()` is too unspecific for a type check. Use `new TypeError()` instead.",
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn type_error() {
        check(
            "S7786",
            "a.js",
            r#"
if (typeof x !== 'string') {
  throw new Error('x must be a string'); // Noncompliant {{`new Error()` is too unspecific for a type check. Use `new TypeError()` instead.}}
}
if (!Array.isArray(x)) {
  throw new Error('array expected'); // Noncompliant
}
if (!(x instanceof Foo)) {
  throw new Error('Foo expected'); // Noncompliant
}
if (_.isString(x) && isNaN(y)) {
  throw new Error('bad'); // Noncompliant
}
if (x instanceof CustomError) {
  throw new Error('bad');
}
if (typeof x !== 'string') throw new Error('not lone');
if (typeof x !== 'string') {
  log();
  throw new Error('two statements');
}
if (x.length > 1) {
  throw new Error('not a type check');
}
if (typeof x !== 'string') {
  throw new TypeError('good');
}
"#,
        );
    }
}
