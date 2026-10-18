//! Return values from functions without side effects should not be ignored
use oxc_ast_visit::Visit;
use oxc_syntax::scope::ScopeFlags;

use crate::helpers::batch_bug1::{is_html, parent_skip_parens_only};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const ARRAY: &[&str] = &[
    "concat",
    "includes",
    "join",
    "slice",
    "indexOf",
    "lastIndexOf",
    "entries",
    "filter",
    "findIndex",
    "findLast",
    "findLastIndex",
    "keys",
    "map",
    "values",
    "find",
    "reduce",
    "reduceRight",
    "toString",
    "toLocaleString",
];
const DATE: &[&str] = &[
    "getDate",
    "getDay",
    "getFullYear",
    "getHours",
    "getMilliseconds",
    "getMinutes",
    "getMonth",
    "getSeconds",
    "getTime",
    "getTimezoneOffset",
    "getUTCDate",
    "getUTCDay",
    "getUTCFullYear",
    "getUTCHours",
    "getUTCMilliseconds",
    "getUTCMinutes",
    "getUTCMonth",
    "getUTCSeconds",
    "getYear",
    "toDateString",
    "toISOString",
    "toJSON",
    "toGMTString",
    "toLocaleDateString",
    "toLocaleTimeString",
    "toTimeString",
    "toUTCString",
    "toString",
    "toLocaleString",
];
const MATH: &[&str] = &[
    "abs", "E", "LN2", "LN10", "LOG2E", "LOG10E", "PI", "SQRT1_2", "SQRT2", "acos", "acosh",
    "asin", "asinh", "atan", "atanh", "atan2", "cbrt", "ceil", "clz32", "cos", "cosh", "exp",
    "expm1", "floor", "fround", "hypot", "imul", "log", "log1p", "log10", "log2", "max", "min",
    "pow", "random", "round", "sign", "sin", "sinh", "sqrt", "tan", "tanh", "trunc",
];
const NUMBER: &[&str] = &[
    "toExponential",
    "toFixed",
    "toPrecision",
    "toLocaleString",
    "toString",
];
const REGEXP: &[&str] = &["test", "toString"];
const STRING: &[&str] = &[
    "charAt",
    "charCodeAt",
    "codePointAt",
    "concat",
    "includes",
    "endsWith",
    "indexOf",
    "lastIndexOf",
    "localeCompare",
    "match",
    "normalize",
    "padEnd",
    "padStart",
    "repeat",
    "replace",
    "search",
    "slice",
    "split",
    "startsWith",
    "substr",
    "substring",
    "toLocaleLowerCase",
    "toLocaleUpperCase",
    "toLowerCase",
    "toUpperCase",
    "trim",
    "length",
    "toString",
    "valueOf",
    "anchor",
    "big",
    "blink",
    "bold",
    "fixed",
    "fontcolor",
    "fontsize",
    "italics",
    "link",
    "small",
    "strike",
    "sub",
    "sup",
];

/// Side-effect-free methods of the receiver, `None` when its type isn't precisely known.
fn pure_methods(ctx: &Ctx<'_, '_>, object: &Expression<'_>) -> Option<&'static [&'static str]> {
    if let Expression::Identifier(id) = strip_ts(object)
        && ctx.is_global(id, "Math")
    {
        return Some(MATH);
    }
    match type_of(ctx, object) {
        Ty::String => Some(STRING),
        Ty::Number => Some(NUMBER),
        Ty::Array => Some(ARRAY),
        Ty::Date => Some(DATE),
        Ty::RegExp => Some(REGEXP),
        _ => None,
    }
}

#[derive(Default)]
struct AssignmentFinder {
    found: bool,
}

impl<'a> Visit<'a> for AssignmentFinder {
    fn visit_assignment_expression(&mut self, _: &AssignmentExpression<'a>) {
        self.found = true;
    }
    fn visit_function(&mut self, _: &Function<'a>, _: ScopeFlags) {}
    fn visit_arrow_function_expression(&mut self, _: &ArrowFunctionExpression<'a>) {}
}

/// `arr.find(x => (found = x))`: early-exit search used for its side effect.
fn is_find_with_assignment(name: &str, args: &[Argument<'_>]) -> bool {
    if !matches!(name, "find" | "findIndex" | "findLast" | "findLastIndex") {
        return false;
    }
    let mut finder = AssignmentFinder::default();
    match args.first().and_then(|a| a.as_expression()) {
        Some(Expression::ArrowFunctionExpression(a)) => {
            if let Some(b) = a.get_function_body() {
                finder.visit_function_body(b);
            } else if let Some(e) = a.get_expression() {
                finder.visit_expression(e);
            }
        }
        Some(Expression::FunctionExpression(f)) => match &f.body {
            Some(b) => finder.visit_function_body(b),
            None => return false,
        },
        _ => return false,
    }
    finder.found
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::CallExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::CallExpression(call) = node.kind() else {
            return;
        };
        if !matches!(
            parent_skip_parens_only(ctx, node.id()).kind(),
            AstKind::ExpressionStatement(_)
        ) {
            return;
        }
        let (object, name) = match &call.callee {
            Expression::StaticMemberExpression(m) => (&m.object, m.property.name.as_str()),
            _ => return,
        };
        static ALL: std::sync::OnceLock<FxHashSet<&'static str>> = std::sync::OnceLock::new();
        let all = ALL.get_or_init(|| {
            [ARRAY, DATE, MATH, NUMBER, REGEXP, STRING]
                .concat()
                .into_iter()
                .collect()
        });
        if !all.contains(name) || is_html(ctx) {
            return;
        }
        let Some(methods) = pure_methods(ctx, object) else {
            return;
        };
        if !methods.contains(&name) {
            return;
        }
        // `str.replace(re, fn)`: the callback may have side effects; only flag non-function replacements.
        if name == "replace" && call.arguments.len() > 1 {
            let Some(arg) = call.arguments[1].as_expression() else {
                return;
            };
            if !type_of(ctx, arg).is_primitive() {
                return;
            }
        }
        if is_find_with_assignment(name, &call.arguments) {
            return;
        }
        if name == "map" {
            ctx.report(call.span, "Consider using \"forEach\" instead of \"map\" as its return value is not being used here.");
        } else {
            ctx.report(
                call.span,
                format!("The return value of \"{name}\" must be used."),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn ignored_return() {
        check(
            "S2201",
            "a.ts",
            r#"
const s = "abc";
s.toUpperCase(); // Noncompliant {{The return value of "toUpperCase" must be used.}}
const arr = [1, 2];
arr.map(x => x * 2); // Noncompliant {{Consider using "forEach" instead of "map" as its return value is not being used here.}}
arr.concat([3]); // Noncompliant
Math.max(1, 2); // Noncompliant
const d = new Date();
d.getTime(); // Noncompliant
s.replace("a", "b"); // Noncompliant
s.replace(/a/g, (m) => { log(m); return m; });
arr.find(x => { found = x; return true; });
arr.forEach(x => x);
arr.push(3);
arr.sort();
const t = s.toUpperCase();
unknown.map(x => x);
function f(a: any) { a.map(x => x); }
"#,
        );
    }
}
