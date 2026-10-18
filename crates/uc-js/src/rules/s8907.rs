//! Native APIs should be preferred over Lodash and Underscore.js methods
use crate::helpers::batch_smell4::{fqn, require_module};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

struct Replacement {
    alternative: &'static str,
    reason: Option<&'static str>,
    example: Option<&'static str>,
}

const SLICE: &str = "Array.prototype.slice()";
const ENTRIES: &str = "Object.entries()";
const ARRAY_NULLISH: &str = "the library handles nullish values differently from the native API";
const COLLECTION: &str = "the library also accepts objects and nullish values";
const PREDICATE: &str =
    "the library also accepts objects, nullish values, and shorthand predicates";
const INCLUDES: &str = "the library also accepts strings, objects, and nullish values";
const STRING_INCLUDES: &str = "the library also accepts arrays, objects, and nullish values";

const fn cautious(alternative: &'static str, reason: &'static str) -> Replacement {
    Replacement {
        alternative,
        reason: Some(reason),
        example: None,
    }
}

const fn with_example(
    alternative: &'static str,
    reason: &'static str,
    example: &'static str,
) -> Replacement {
    Replacement {
        alternative,
        reason: Some(reason),
        example: Some(example),
    }
}

fn replacement(method: &str) -> Option<Replacement> {
    Some(match method {
        "assign" => cautious(
            "Object.assign()",
            "the library handles nullish targets differently from Object.assign",
        ),
        "concat" => cautious("Array.prototype.concat()", ARRAY_NULLISH),
        "contains" | "includes" => cautious("Array.prototype.includes()", INCLUDES),
        "drop" => with_example(SLICE, ARRAY_NULLISH, "`array.slice(n)`"),
        "dropRight" => with_example(
            SLICE,
            ARRAY_NULLISH,
            "`n === 0 ? array.slice() : array.slice(0, -n)`",
        ),
        "entries" | "pairs" | "toPairs" => cautious(ENTRIES, ARRAY_NULLISH),
        "every" => cautious("Array.prototype.every()", PREDICATE),
        "extendOwn" => cautious("Object.assign()", ARRAY_NULLISH),
        "fill" => cautious("Array.prototype.fill()", ARRAY_NULLISH),
        "filter" => cautious("Array.prototype.filter()", PREDICATE),
        "find" => cautious("Array.prototype.find()", PREDICATE),
        "findIndex" => cautious("Array.prototype.findIndex()", PREDICATE),
        "flatten" => cautious("Array.prototype.flat()", ARRAY_NULLISH),
        "indexOf" => cautious("Array.prototype.indexOf()", ARRAY_NULLISH),
        "isArray" => Replacement {
            alternative: "Array.isArray()",
            reason: None,
            example: None,
        },
        "isInteger" => Replacement {
            alternative: "Number.isInteger()",
            reason: None,
            example: None,
        },
        "join" => cautious("Array.prototype.join()", ARRAY_NULLISH),
        "keys" => cautious("Object.keys()", ARRAY_NULLISH),
        "lastIndexOf" => cautious("Array.prototype.lastIndexOf()", ARRAY_NULLISH),
        "map" => cautious("Array.prototype.map()", COLLECTION),
        "reduce" => cautious("Array.prototype.reduce()", PREDICATE),
        "reduceRight" => cautious("Array.prototype.reduceRight()", PREDICATE),
        "reverse" => cautious("Array.prototype.reverse()", ARRAY_NULLISH),
        "slice" => cautious(SLICE, ARRAY_NULLISH),
        "some" => cautious("Array.prototype.some()", PREDICATE),
        "takeRight" => with_example(SLICE, ARRAY_NULLISH, "`n === 0 ? [] : array.slice(-n)`"),
        "uniq" => with_example("Set", ARRAY_NULLISH, "`[...new Set(values)]`"),
        "values" => cautious("Object.values()", ARRAY_NULLISH),
        _ => return None,
    })
}

const METHODS: &[&str] = &[
    "assign",
    "concat",
    "contains",
    "drop",
    "dropRight",
    "entries",
    "every",
    "extendOwn",
    "fill",
    "filter",
    "find",
    "findIndex",
    "flatten",
    "includes",
    "indexOf",
    "isArray",
    "isInteger",
    "join",
    "keys",
    "lastIndexOf",
    "map",
    "pairs",
    "reduce",
    "reduceRight",
    "reverse",
    "slice",
    "some",
    "takeRight",
    "toPairs",
    "uniq",
    "values",
];

fn is_shape_dependent(method: &str) -> bool {
    matches!(
        method,
        "contains"
            | "every"
            | "filter"
            | "find"
            | "findIndex"
            | "includes"
            | "map"
            | "reduce"
            | "reduceRight"
            | "some"
    )
}

#[derive(PartialEq)]
enum Shape {
    Array,
    String,
    Unknown,
}

fn shape(ctx: &Ctx<'_, '_>, e: &Expression<'_>) -> Shape {
    match strip_parens(e) {
        Expression::ArrayExpression(_) => Shape::Array,
        Expression::StringLiteral(_) | Expression::TemplateLiteral(_) => Shape::String,
        e => {
            let t = type_of(ctx, e);
            if t.is_array() {
                Shape::Array
            } else if t.is_string() {
                Shape::String
            } else {
                Shape::Unknown
            }
        }
    }
}

/// Follows `const` aliases to the expression they hold.
fn constant<'a>(ctx: &Ctx<'a, '_>, e: &'a Expression<'a>, depth: u8) -> &'a Expression<'a> {
    let Expression::Identifier(id) = strip_parens(e) else {
        return strip_parens(e);
    };
    if depth > 4 {
        return e;
    }
    let Some(sym) = ctx.symbol_of(id) else {
        return e;
    };
    let decl = ctx.symbol_decl(sym);
    let AstKind::VariableDeclarator(v) = decl.kind() else {
        return e;
    };
    let is_const = matches!(ctx.parent_kind(decl.id()),
        AstKind::VariableDeclaration(d) if d.kind == VariableDeclarationKind::Const);
    match (&v.id, &v.init) {
        (BindingPattern::BindingIdentifier(_), Some(init)) if is_const => {
            constant(ctx, init, depth + 1)
        }
        _ => e,
    }
}

fn is_shorthand<'a>(ctx: &Ctx<'a, '_>, e: &'a Expression<'a>) -> bool {
    matches!(
        constant(ctx, e, 0),
        Expression::ArrayExpression(_)
            | Expression::StringLiteral(_)
            | Expression::NumericLiteral(_)
            | Expression::BooleanLiteral(_)
            | Expression::NullLiteral(_)
            | Expression::RegExpLiteral(_)
            | Expression::BigIntLiteral(_)
            | Expression::ObjectExpression(_)
            | Expression::TemplateLiteral(_)
    )
}

fn is_regexp<'a>(ctx: &Ctx<'a, '_>, e: &'a Expression<'a>) -> bool {
    match constant(ctx, e, 0) {
        Expression::RegExpLiteral(_) => true,
        Expression::CallExpression(c) => {
            matches!(&c.callee, Expression::Identifier(id) if id.name == "RegExp")
        }
        Expression::NewExpression(n) => {
            matches!(&n.callee, Expression::Identifier(id) if id.name == "RegExp")
        }
        _ => false,
    }
}

fn resolve<'a>(
    ctx: &Ctx<'a, '_>,
    method: &str,
    call: &'a CallExpression<'a>,
) -> Option<Replacement> {
    let rep = replacement(method)?;
    if !is_shape_dependent(method) {
        return Some(rep);
    }
    let callback = !matches!(method, "contains" | "includes");
    if callback {
        let iteratee = call.arguments.get(1)?.as_expression()?;
        if is_shorthand(ctx, iteratee) {
            return None;
        }
    }
    let collection = call.arguments.first()?.as_expression()?;
    match shape(ctx, collection) {
        Shape::Array => Some(rep),
        Shape::String if !callback => {
            let search = call.arguments.get(1);
            if search.is_some_and(|s| {
                s.is_spread() || s.as_expression().is_some_and(|e| is_regexp(ctx, e))
            }) {
                return None;
            }
            Some(Replacement {
                alternative: "String.prototype.includes()",
                reason: Some(STRING_INCLUDES),
                example: None,
            })
        }
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
        let (syntactic, report_span) = match &call.callee {
            Expression::StaticMemberExpression(m) => {
                if !METHODS.contains(&m.property.name.as_str()) {
                    return;
                }
                if matches!(strip_parens(&m.object), Expression::CallExpression(_))
                    && require_module(&m.object).is_none()
                {
                    return;
                }
                (Some(m.property.name.as_str()), m.property.span)
            }
            Expression::Identifier(id) => (None, id.span),
            Expression::CallExpression(c) => (None, c.span),
            _ => return,
        };
        let Some(name) = fqn(ctx, &call.callee) else {
            return;
        };
        let mut parts = name.split('.');
        let (Some(module), Some(qualifier), None) = (parts.next(), parts.next(), parts.next())
        else {
            return;
        };
        let library = match module {
            "lodash" | "lodash-es" => module,
            "underscore" => "underscore.js",
            _ => return,
        };
        let method = match syntactic {
            Some(s) => {
                if s != qualifier {
                    return;
                }
                s
            }
            None => match METHODS.iter().find(|m| m.eq_ignore_ascii_case(qualifier)) {
                Some(m) => *m,
                None => return,
            },
        };
        let Some(rep) = resolve(ctx, method, call) else {
            return;
        };
        let msg = match (rep.reason, rep.example) {
            (None, _) => format!(
                "Use {} instead of {method}() from {library}.",
                rep.alternative
            ),
            (Some(reason), None) => format!(
                "Consider {} instead of {method}() from {library}; check that the behavior is equivalent because {reason}.",
                rep.alternative
            ),
            (Some(reason), Some(example)) => format!(
                "Consider {} instead of {method}() from {library}; for example, use {example}. Check that the behavior is equivalent because {reason}.",
                rep.alternative
            ),
        };
        ctx.report(report_span, msg);
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn rule() {
        check(
            "S8907",
            "a.js",
            r#"
import _ from 'lodash';
import uniq from 'lodash/uniq';
import { isArray } from 'lodash-es';
const u = require('underscore');
_.isArray(x); // Noncompliant {{Use Array.isArray() instead of isArray() from lodash.}}
_.concat(items, callback); // Noncompliant {{Consider Array.prototype.concat() instead of concat() from lodash; check that the behavior is equivalent because the library handles nullish values differently from the native API.}}
uniq(values); // Noncompliant {{Consider Set instead of uniq() from lodash; for example, use `[...new Set(values)]`. Check that the behavior is equivalent because the library handles nullish values differently from the native API.}}
isArray(x); // Noncompliant {{Use Array.isArray() instead of isArray() from lodash-es.}}
_.map([item], item => item.label); // Noncompliant
u.isInteger(n); // Noncompliant {{Use Number.isInteger() instead of isInteger() from underscore.js.}}
_.includes('abc', 'b'); // Noncompliant {{Consider String.prototype.includes() instead of includes() from lodash; check that the behavior is equivalent because the library also accepts arrays, objects, and nullish values.}}
_.map(items, item => item.label);
_.map([item], 'label');
_.filter([item]);
_.includes('abc', /b/);
_.trim(name);
_.forEach([item], cb);
_(items).filter(x => x);
lodash(items).filter(item => item.enabled).value();
other.isArray(x);
const shadow = { map() {} };
shadow.map(items);
"#,
        );
    }
}
