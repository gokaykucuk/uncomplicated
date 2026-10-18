//! Function references should not be passed directly to iterator methods
use crate::helpers::batch_unicorn::{const_init, method_call};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const IGNORED_CALLEES: &[&str] = &[
    "Promise",
    "React.Children",
    "Children",
    "lodash",
    "underscore",
    "_",
    "Async",
    "async",
    "this",
    "$",
    "jQuery",
];

fn ignored_names(method: &str) -> &'static [&'static str] {
    match method {
        "every" | "filter" | "find" | "findLast" | "findIndex" | "findLastIndex" | "some" => {
            &["Boolean"]
        }
        "map" => &["String", "Number", "BigInt", "Boolean", "Symbol"],
        _ => &[],
    }
}

fn is_iterator_method(m: &str) -> bool {
    matches!(
        m,
        "every"
            | "filter"
            | "find"
            | "findLast"
            | "findIndex"
            | "findLastIndex"
            | "flatMap"
            | "forEach"
            | "map"
            | "reduce"
            | "reduceRight"
            | "some"
    )
}

fn matches_ignored(e: &Expression<'_>) -> bool {
    dotted_path(e).is_some_and(|p| IGNORED_CALLEES.contains(&p.as_str()))
}

fn params_count(params: &FormalParameters<'_>) -> usize {
    params.items.len() + usize::from(params.rest.is_some())
}

fn is_predicate(ret: Option<&TSTypeAnnotation<'_>>) -> bool {
    matches!(
        ret.map(|r| &r.type_annotation),
        Some(TSType::TSTypePredicate(_))
    )
}

/// What we know about a callback reference.
enum Arity {
    /// Known number of declared parameters (and whether it is a type predicate).
    Known(usize, bool),
    /// A class: no call signature, Sonar reports it.
    Class,
    Unknown,
}

fn function_arity(e: &Expression<'_>) -> Arity {
    match strip_parens(e) {
        Expression::ArrowFunctionExpression(a) => Arity::Known(
            params_count(&a.params),
            is_predicate(a.return_type.as_deref()),
        ),
        Expression::FunctionExpression(f) => Arity::Known(
            params_count(&f.params),
            is_predicate(f.return_type.as_deref()),
        ),
        Expression::ClassExpression(_) => Arity::Class,
        _ => Arity::Unknown,
    }
}

fn arity(ctx: &Ctx<'_, '_>, e: &Expression<'_>) -> Arity {
    match strip_parens(e) {
        Expression::Identifier(id) => {
            let Some(sym) = ctx.symbol_of(id) else {
                return match id.name.as_str() {
                    "parseInt" => Arity::Known(2, false),
                    _ => Arity::Unknown,
                };
            };
            if ctx.scoping().symbol_is_mutated(sym) {
                return Arity::Unknown;
            }
            match ctx.symbol_decl(sym).kind() {
                AstKind::Function(f) => Arity::Known(
                    params_count(&f.params),
                    is_predicate(f.return_type.as_deref()),
                ),
                AstKind::Class(_) => Arity::Class,
                AstKind::VariableDeclarator(v) if v.type_annotation.is_none() => {
                    const_init(ctx, e).map_or(Arity::Unknown, function_arity)
                }
                AstKind::FormalParameter(p) => {
                    match p.type_annotation.as_ref().map(|t| &t.type_annotation) {
                        Some(TSType::TSFunctionType(f)) => Arity::Known(
                            params_count(&f.params),
                            matches!(f.return_type.type_annotation, TSType::TSTypePredicate(_)),
                        ),
                        _ => Arity::Unknown,
                    }
                }
                _ => Arity::Unknown,
            }
        }
        Expression::StaticMemberExpression(m) => {
            let Some(Expression::ObjectExpression(obj)) = const_init(ctx, &m.object) else {
                return Arity::Unknown;
            };
            for p in &obj.properties {
                if let ObjectPropertyKind::ObjectProperty(p) = p
                    && !p.computed
                    && p.kind == PropertyKind::Init
                    && property_key_name(&p.key) == Some(m.property.name.as_str())
                {
                    return function_arity(&p.value);
                }
            }
            Arity::Unknown
        }
        _ => Arity::Unknown,
    }
}

fn collect<'b, 'a>(e: &'b Expression<'a>, out: &mut Vec<&'b Expression<'a>>) {
    match strip_parens(e) {
        Expression::ConditionalExpression(c) => {
            collect(&c.consequent, out);
            collect(&c.alternate, out);
        }
        e => out.push(e),
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
        if call.optional || call.arguments.is_empty() || call.arguments.len() > 2 {
            return;
        }
        if call
            .arguments
            .iter()
            .any(|a| matches!(a, Argument::SpreadElement(_)))
        {
            return;
        }
        let Some((member, method)) = method_call(call) else {
            return;
        };
        if !is_iterator_method(method) {
            return;
        }
        let is_reduce = matches!(method, "reduce" | "reduceRight");
        if !is_reduce
            && matches!(
                ctx.parent_skip_parens(node.id()).kind(),
                AstKind::AwaitExpression(_)
            )
        {
            return;
        }
        let object = strip_parens(&member.object);
        if matches_ignored(object)
            || matches!(object, Expression::CallExpression(c) if matches_ignored(&c.callee))
            || (method == "filter"
                && matches!(object, Expression::Identifier(o) if o.name == "Vue"))
            || (method == "map" && matches!(object, Expression::Identifier(o) if o.name == "types"))
        {
            return;
        }
        // Sonar only reports on arrays (type information).
        if !type_of(ctx, &member.object).is_array() {
            return;
        }
        let Some(first) = call.arguments[0].as_expression() else {
            return;
        };
        let mut callbacks = Vec::new();
        collect(first, &mut callbacks);
        let ignored = ignored_names(method);
        let predicate_overloads = matches!(method, "every" | "filter" | "find" | "findLast");
        for cb in callbacks {
            let name = match cb {
                Expression::Identifier(id) => {
                    if ignored.contains(&id.name.as_str()) {
                        continue;
                    }
                    Some(id.name.as_str())
                }
                Expression::StaticMemberExpression(_) => None,
                _ => continue,
            };
            let report = match arity(ctx, cb) {
                Arity::Known(n, predicate) => n > 1 && !(predicate && predicate_overloads),
                Arity::Class => true,
                Arity::Unknown => false,
            };
            if !report {
                continue;
            }
            let msg = match name {
                Some(n) => format!("Do not pass function `{n}` directly to `.{method}(…)`."),
                None => format!("Do not pass function directly to `.{method}(…)`."),
            };
            ctx.report(cb.span(), msg);
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn callback_reference() {
        check(
            "S7727",
            "a.ts",
            r#"
['1', '2', '3'].map(parseInt); // Noncompliant {{Do not pass function `parseInt` directly to `.map(…)`.}}
const addIndex = (value: number, index: number) => value + index;
[1, 2, 3].map(addIndex); // Noncompliant
function processWithIndex(item: number, idx: number): number { return item * idx; }
[1, 2, 3].map(processWithIndex); // Noncompliant
const reducer = (acc: number, curr: number) => acc + curr;
[1, 2, 3].reduce(reducer); // Noncompliant {{Do not pass function `reducer` directly to `.reduce(…)`.}}
const utils = { process: (x: number, y: number) => x + y, double: (x: number) => x * 2 };
[1, 2, 3].map(utils.process); // Noncompliant {{Do not pass function directly to `.map(…)`.}}
class MyClass { constructor(public value: number) {} }
[1, 2, 3].map(MyClass); // Noncompliant
const arr: number[] = [];
arr.forEach(cond ? addIndex : (x) => x); // Noncompliant

const double = (x: number) => x * 2;
[1, 2, 3].map(double);
function square(n: number): number { return n * n; }
[1, 2, 3].map(square);
[1, 2, 3].map(utils.double);
[1, 0, 2].filter(Boolean);
[1, 2, 3].map(x => x * 2);
declare const tree: any;
tree.find(addIndex);
[1, 2, 3].map(unknownFn);
function isNum(x: unknown, i: number): x is number { return true; }
[1, 2].filter(isNum);
async function f() { await [1].map(addIndex); }
[1].map(addIndex, thisArg, extra);
const rest = (...args: number[]) => args;
[1].map(rest);
"#,
        );
    }
}
