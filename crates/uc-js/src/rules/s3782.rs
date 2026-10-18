//! Arguments to built-in functions should match documented types
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// Declared parameter kinds: `n` = number, `s` = string, `*` = anything (generic / unchecked).
/// Only non-overloaded methods (or overloads that agree) of the built-ins Sonar checks
/// (`String`, `Math`, `Array`, `Number`, `Date`) are listed; rest parameters are omitted.
fn signature(receiver: Recv, name: &str) -> Option<&'static [u8]> {
    Some(match receiver {
        Recv::Math => match name {
            "abs" | "acos" | "acosh" | "asin" | "asinh" | "atan" | "atanh" | "cbrt" | "ceil"
            | "clz32" | "cos" | "cosh" | "exp" | "expm1" | "floor" | "fround" | "log" | "log10"
            | "log1p" | "log2" | "round" | "sign" | "sin" | "sinh" | "sqrt" | "tan" | "tanh"
            | "trunc" => b"n",
            "atan2" | "imul" | "pow" => b"nn",
            _ => return None,
        },
        Recv::String => match name {
            "charAt" | "charCodeAt" | "codePointAt" | "repeat" | "at" => b"n",
            "endsWith" | "includes" | "indexOf" | "lastIndexOf" | "startsWith" => b"sn",
            "padEnd" | "padStart" => b"ns",
            "slice" | "substr" | "substring" => b"nn",
            _ => return None,
        },
        Recv::Array => match name {
            "join" => b"s",
            "slice" | "splice" => b"nn",
            "at" => b"n",
            "indexOf" | "lastIndexOf" | "includes" => b"*n",
            "fill" => b"*nn",
            _ => return None,
        },
        Recv::Number => match name {
            "toFixed" | "toPrecision" | "toExponential" | "toString" => b"n",
            _ => return None,
        },
        Recv::Date => match name {
            "setDate" | "setMilliseconds" | "setTime" | "setUTCDate" | "setUTCMilliseconds" => b"n",
            "setFullYear" | "setUTCFullYear" => b"nnn",
            "setHours" | "setUTCHours" => b"nnnn",
            "setMinutes" | "setUTCMinutes" => b"nnn",
            "setMonth" | "setUTCMonth" | "setSeconds" | "setUTCSeconds" => b"nn",
            _ => return None,
        },
    })
}

#[derive(Clone, Copy)]
enum Recv {
    Math,
    String,
    Array,
    Number,
    Date,
}

fn primitive_name(t: &Ty) -> Option<&'static str> {
    match t {
        Ty::String => Some("string"),
        Ty::Number => Some("number"),
        Ty::Boolean => Some("boolean"),
        _ => None,
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::CallExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::CallExpression(call) = node.kind() else {
            return;
        };
        if call.arguments.is_empty() {
            return;
        }
        let Expression::StaticMemberExpression(m) = strip_parens(&call.callee) else {
            return;
        };
        let receiver = match strip_parens(&m.object) {
            Expression::Identifier(id) if ctx.is_global(id, "Math") => Recv::Math,
            obj => match type_of(ctx, obj) {
                Ty::String => Recv::String,
                Ty::Array => Recv::Array,
                Ty::Number => Recv::Number,
                Ty::Date => Recv::Date,
                _ => return,
            },
        };
        let Some(sig) = signature(receiver, &m.property.name) else {
            return;
        };
        for (arg, kind) in call.arguments.iter().zip(sig.iter()) {
            let expected = match kind {
                b'n' => "number",
                b's' => "string",
                _ => continue,
            };
            let Some(arg) = arg.as_expression() else {
                return;
            };
            let Some(actual) = primitive_name(&type_of(ctx, arg)) else {
                continue;
            };
            if actual != expected {
                ctx.report(
                    strip_parens(arg).span(),
                    format!("Verify that argument is of correct type: expected '{expected}' instead of '{actual}'."),
                );
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn argument_type() {
        check(
            "S3782",
            "a.js",
            r#"
const str = 'abc';
const arr = [1, 2];
Math.abs('1');                 // Noncompliant {{Verify that argument is of correct type: expected 'number' instead of 'string'.}}
Math.pow(2, true);             // Noncompliant {{Verify that argument is of correct type: expected 'number' instead of 'boolean'.}}
str.charAt('1');               // Noncompliant
str.includes(5);               // Noncompliant {{Verify that argument is of correct type: expected 'string' instead of 'number'.}}
str.padStart(2, 0);            // Noncompliant
arr.join(1);                   // Noncompliant
arr.indexOf('x');
Math.max('1', 2);
Math.abs(x);
str.charAt(1);
str.replace(1, 2);
foo.charAt('1');
"#,
        );
    }
}
