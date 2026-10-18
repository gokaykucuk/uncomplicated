//! "Array.prototype.sort()" and "Array.prototype.toSorted()" should use a compare function
use crate::helpers::batch_bug1::{const_init, parent_skip_parens_only};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// Element type of an array, as far as we can tell.
#[derive(PartialEq, Eq)]
enum Elem {
    String,
    Other,
}

fn elem_of_types(types: impl Iterator<Item = Ty>) -> Option<Elem> {
    let mut all_string = true;
    let mut any = false;
    for t in types {
        any = true;
        if t.is_unknown() {
            return None;
        }
        if !t.is_string() {
            all_string = false;
        }
    }
    any.then_some(if all_string {
        Elem::String
    } else {
        Elem::Other
    })
}

fn elem_of_annotation(ctx: &Ctx<'_, '_>, t: &TSType<'_>) -> Option<Elem> {
    let elem = match t {
        TSType::TSArrayType(a) => &a.element_type,
        TSType::TSTypeReference(r) => {
            let TSTypeName::IdentifierReference(id) = &r.type_name else {
                return None;
            };
            if id.name != "Array"
                || ctx
                    .scoping()
                    .get_reference(id.reference_id())
                    .symbol_id()
                    .is_some()
            {
                return None;
            }
            r.type_arguments.as_ref()?.params.first()?
        }
        TSType::TSParenthesizedType(p) => return elem_of_annotation(ctx, &p.type_annotation),
        _ => return None,
    };
    elem_of_types(std::iter::once(types::from_annotation(ctx, elem, 0)))
}

/// Element type of an array-typed expression; `None` if it's not known to be an array.
fn array_elem(ctx: &Ctx<'_, '_>, e: &Expression<'_>, depth: u8) -> Option<Elem> {
    if depth > 4 {
        return None;
    }
    match strip_parens(e) {
        Expression::ArrayExpression(a) => {
            if a.elements.is_empty() {
                return None;
            }
            let mut types = Vec::with_capacity(a.elements.len());
            for el in &a.elements {
                types.push(type_of(ctx, el.as_expression()?));
            }
            elem_of_types(types.into_iter())
        }
        Expression::TSAsExpression(x) => elem_of_annotation(ctx, &x.type_annotation),
        Expression::Identifier(id) => {
            let sym = ctx.symbol_of(id)?;
            match ctx.symbol_decl(sym).kind() {
                AstKind::VariableDeclarator(v) => {
                    if let Some(ann) = &v.type_annotation {
                        return elem_of_annotation(ctx, &ann.type_annotation);
                    }
                    array_elem(ctx, const_init(ctx, sym)?, depth + 1)
                }
                AstKind::FormalParameter(p) => {
                    elem_of_annotation(ctx, &p.type_annotation.as_ref()?.type_annotation)
                }
                _ => None,
            }
        }
        Expression::CallExpression(c) => {
            let Expression::StaticMemberExpression(m) = strip_parens(&c.callee) else {
                return None;
            };
            let name = m.property.name.as_str();
            if let Expression::Identifier(obj) = strip_parens(&m.object)
                && obj.name == "Object"
                && ctx.symbol_of(obj).is_none()
            {
                return matches!(name, "keys" | "getOwnPropertyNames").then_some(Elem::String);
            }
            match name {
                "split" if type_of(ctx, &m.object).is_string() => Some(Elem::String),
                "filter" | "slice" | "concat" | "reverse" | "sort" | "toSorted" | "toReversed" => {
                    array_elem(ctx, &m.object, depth + 1)
                }
                _ => None,
            }
        }
        _ => None,
    }
}

/// `JSON.stringify(a.sort()) === ...` / `a.sort().join() === ...`: order normalization for comparison.
fn is_normalization_comparison(ctx: &Ctx<'_, '_>, call_node: &AstNode<'_>) -> bool {
    let parent = parent_skip_parens_only(ctx, call_node.id());
    let outer = match parent.kind() {
        AstKind::CallExpression(c) => {
            let is_stringify = matches!(strip_parens(&c.callee), Expression::StaticMemberExpression(m)
                if m.property.name == "stringify" && matches!(strip_parens(&m.object), Expression::Identifier(j) if j.name == "JSON"));
            if !is_stringify || c.arguments.len() != 1 {
                return false;
            }
            parent
        }
        AstKind::StaticMemberExpression(m) if m.property.name == "join" => {
            let call = parent_skip_parens_only(ctx, parent.id());
            if !matches!(call.kind(), AstKind::CallExpression(_)) {
                return false;
            }
            call
        }
        _ => return false,
    };
    matches!(parent_skip_parens_only(ctx, outer.id()).kind(), AstKind::BinaryExpression(b) if b.operator.is_equality())
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::CallExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::CallExpression(call) = node.kind() else {
            return;
        };
        if !call.arguments.is_empty() {
            return;
        }
        let (object, prop_span) = match &call.callee {
            Expression::StaticMemberExpression(m)
                if matches!(m.property.name.as_str(), "sort" | "toSorted") =>
            {
                (&m.object, m.property.span)
            }
            Expression::ComputedMemberExpression(m)
                if m.static_property_name()
                    .is_some_and(|n| n == "sort" || n == "toSorted") =>
            {
                (&m.object, m.expression.span())
            }
            _ => return,
        };
        let Some(elem) = array_elem(ctx, object, 0) else {
            return;
        };
        if is_normalization_comparison(ctx, node) {
            return;
        }
        let msg = match elem {
            Elem::String => {
                "Provide a compare function that depends on \"String.localeCompare\", to reliably sort elements alphabetically."
            }
            Elem::Other => "Provide a compare function to avoid sorting elements alphabetically.",
        };
        ctx.report(prop_span, msg);
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn sort_without_compare() {
        check(
            "S2871",
            "a.ts",
            r#"
const nums = [3, 1, 2];
nums.sort(); // Noncompliant {{Provide a compare function to avoid sorting elements alphabetically.}}
const strs: string[] = ["b", "a"];
strs.sort(); // Noncompliant {{Provide a compare function that depends on "String.localeCompare", to reliably sort elements alphabetically.}}
strs.toSorted(); // Noncompliant
Object.keys(obj).sort(); // Noncompliant
function f(a: Array<number>) { return a.sort(); } // Noncompliant
nums.sort((a, b) => a - b);
strs.sort((a, b) => a.localeCompare(b));
unknown.sort();
const typed = new Int32Array(3);
typed.sort();
if (JSON.stringify(strs.sort()) === JSON.stringify(other)) {}
if (strs.sort().join() === other.join()) {}
"#,
        );
    }
}
