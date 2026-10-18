//! Magic numbers should not be used
use crate::helpers::batch_smell1::{ancestors, parent_no_parens};
use crate::prelude::*;

pub struct Rule {
    ignore: Vec<f64>,
    ignore_default_values: bool,
    ignore_class_field_initial_values: bool,
    ignore_enums: bool,
    ignore_numeric_literal_types: bool,
    ignore_readonly_class_properties: bool,
}

fn parse_numbers(v: Option<&serde_json::Value>) -> Option<Vec<f64>> {
    match v? {
        serde_json::Value::Array(a) => Some(
            a.iter()
                .filter_map(|x| match x {
                    serde_json::Value::Number(n) => n.as_f64(),
                    serde_json::Value::String(s) => s.trim().parse().ok(),
                    _ => None,
                })
                .collect(),
        ),
        serde_json::Value::String(s) => Some(
            s.trim_matches(|c| c == '[' || c == ']')
                .split(',')
                .filter_map(|p| p.trim().parse::<f64>().ok())
                .collect(),
        ),
        serde_json::Value::Number(n) => n.as_f64().map(|f| vec![f]),
        _ => None,
    }
}

impl Rule {
    pub fn new(p: &Params) -> Self {
        Self {
            ignore: parse_numbers(p.get("ignore"))
                .unwrap_or_else(|| vec![0.0, 1.0, -1.0, 24.0, 60.0]),
            ignore_default_values: p.bool("ignoreDefaultValues", true),
            ignore_class_field_initial_values: p.bool("ignoreClassFieldInitialValues", true),
            ignore_enums: p.bool("ignoreEnums", true),
            ignore_numeric_literal_types: p.bool("ignoreNumericLiteralTypes", true),
            ignore_readonly_class_properties: p.bool("ignoreReadonlyClassProperties", true),
        }
    }
}

fn is_power(v: f64) -> bool {
    let is_int = |x: f64| x.is_finite() && x.fract() == 0.0;
    v > 0.0 && (is_int(v.log10()) || is_int(v.log2()))
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::NumericLiteral]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::NumericLiteral(lit) = node.kind() else {
            return;
        };
        if lit.value == 0.0 {
            return;
        }
        // Treat unary minus as part of the number.
        let mut full = node;
        let mut value = lit.value;
        let mut raw = ctx.text(lit.span).to_string();
        let mut parent = parent_no_parens(ctx, node.id());
        if let AstKind::UnaryExpression(u) = parent.kind()
            && u.operator == UnaryOperator::UnaryNegation
        {
            full = parent;
            value = -value;
            raw = format!("-{raw}");
            parent = parent_no_parens(ctx, parent.id());
        }
        // Sonar's pre-filters.
        if is_power(value) {
            return;
        }
        if ancestors(ctx, node.id()).any(|a| {
            matches!(
                a.kind(),
                AstKind::JSXElement(_)
                    | AstKind::JSXFragment(_)
                    | AstKind::JSXAttribute(_)
                    | AstKind::JSXExpressionContainer(_)
                    | AstKind::JSXOpeningElement(_)
                    | AstKind::JSXSpreadAttribute(_)
                    | AstKind::JSXSpreadChild(_)
            )
        }) {
            return;
        }
        match parent.kind() {
            AstKind::BinaryExpression(b)
                if matches!(
                    b.operator,
                    BinaryOperator::BitwiseAnd
                        | BinaryOperator::BitwiseOR
                        | BinaryOperator::BitwiseXOR
                        | BinaryOperator::ShiftLeft
                        | BinaryOperator::ShiftRight
                        | BinaryOperator::ShiftRightZeroFill
                ) =>
            {
                return;
            }
            AstKind::CallExpression(c)
                if c.arguments.len() >= 3
                    && dotted_path(&c.callee).as_deref() == Some("JSON.stringify") =>
            {
                return;
            }
            _ => {}
        }
        let report = || ctx.report(full.kind().span(), format!("No magic number: {raw}."));

        // typescript-eslint specific cases.
        let ts_allowed: Option<bool> = match parent.kind() {
            AstKind::TSEnumMember(_) => Some(self.ignore_enums),
            AstKind::TSLiteralType(_) => {
                let gp = ctx.parent(parent.id());
                let in_alias = match gp.kind() {
                    AstKind::TSTypeAliasDeclaration(_) => true,
                    AstKind::TSUnionType(_) => {
                        matches!(ctx.parent_kind(gp.id()), AstKind::TSTypeAliasDeclaration(_))
                    }
                    _ => false,
                };
                if in_alias {
                    Some(self.ignore_numeric_literal_types)
                } else {
                    let mut anc = parent;
                    while matches!(
                        ctx.parent_kind(anc.id()),
                        AstKind::TSUnionType(_) | AstKind::TSIntersectionType(_)
                    ) {
                        anc = ctx.parent(anc.id());
                    }
                    if matches!(ctx.parent_kind(anc.id()), AstKind::TSIndexedAccessType(_)) {
                        Some(false)
                    } else {
                        None
                    }
                }
            }
            AstKind::PropertyDefinition(p) if p.readonly => {
                Some(self.ignore_readonly_class_properties)
            }
            _ => None,
        };
        match ts_allowed {
            Some(true) => return,
            Some(false) => {
                report();
                return;
            }
            None => {}
        }

        // ESLint core `no-magic-numbers`.
        if self.ignore.contains(&value) {
            return;
        }
        let full_span = full.kind().span();
        match parent.kind() {
            AstKind::AssignmentPattern(a)
                if self.ignore_default_values && a.right.span() == full_span =>
            {
                return;
            }
            AstKind::FormalParameter(p)
                if self.ignore_default_values
                    && p.initializer
                        .as_ref()
                        .is_some_and(|i| i.span() == full_span) =>
            {
                return;
            }
            AstKind::AssignmentTargetWithDefault(a)
                if self.ignore_default_values && a.init.span() == full_span =>
            {
                return;
            }
            AstKind::PropertyDefinition(p)
                if self.ignore_class_field_initial_values
                    && p.value.as_ref().is_some_and(|v| v.span() == full_span) =>
            {
                return;
            }
            AstKind::CallExpression(c)
                if c.arguments.get(1).is_some_and(|a| a.span() == full_span)
                    && matches!(
                        dotted_path(&c.callee).as_deref(),
                        Some("parseInt" | "Number.parseInt")
                    ) =>
            {
                return;
            }
            AstKind::VariableDeclarator(_)
            | AstKind::ObjectExpression(_)
            | AstKind::ObjectProperty(_) => return,
            AstKind::AssignmentExpression(a) => {
                if matches!(a.left, AssignmentTarget::AssignmentTargetIdentifier(_)) {
                    report();
                }
                return;
            }
            _ => {}
        }
        report();
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn magic_numbers() {
        check(
            "S109",
            "a.tsx",
            r#"
const a = 42;
let b = 0;
foo(42);          // Noncompliant {{No magic number: 42.}}
x = 42;           // Noncompliant
this.x = 42;
const o = { a: 42 };
const arr = [42]; // Noncompliant
const t = 2 * 3;  // Noncompliant {{No magic number: 3.}}
y = -42;          // Noncompliant {{No magic number: -42.}}
z = -1;
const big = 1000 * 1024;
function f(p = 42) {}
class C { field = 42; readonly r = 42; static m() { return 42; } } // Noncompliant
enum E { A = 42 }
type T = 42 | 43;
let v: 7 = 7;     // Noncompliant
parseInt(s, 42);
const mask = flags & 0x3f;
const el = <div width={42}/>;
JSON.stringify(o, null, 3);
setTimeout(f, 24);
"#,
        );
    }
}
