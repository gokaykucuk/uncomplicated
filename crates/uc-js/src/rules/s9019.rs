//! Vue refs should not be used as operands
use crate::helpers::batch_bug2::vue_callee_name;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const REF_CREATORS: [&str; 6] = [
    "ref",
    "computed",
    "toRef",
    "customRef",
    "shallowRef",
    "defineModel",
];

/// The `vue` function that created the ref held by `sym`, if any.
fn ref_creator<'a>(ctx: &Ctx<'a, '_>, sym: SymbolId) -> Option<(&'a str, bool)> {
    let decl = ctx.symbol_decl(sym);
    let AstKind::VariableDeclarator(v) = decl.kind() else {
        return None;
    };
    let is_const = matches!(ctx.parent_kind(decl.id()), AstKind::VariableDeclaration(d) if d.kind == VariableDeclarationKind::Const);
    let Some(Expression::CallExpression(c)) = v.init.as_ref().map(strip_ts) else {
        return None;
    };
    let name = vue_callee_name(ctx, &c.callee)?;
    match &v.id {
        BindingPattern::BindingIdentifier(_) if REF_CREATORS.contains(&name) => {
            Some((name, is_const))
        }
        // `const { a } = toRefs(state)`
        BindingPattern::ObjectPattern(_) if name == "toRefs" => Some((name, is_const)),
        _ => None,
    }
}

/// `x = ref(...)`: reassigning a ref variable with a new ref is legitimate.
fn is_ref_reassignment<'a>(ctx: &Ctx<'a, '_>, a: &AssignmentExpression<'a>) -> bool {
    a.operator == AssignmentOperator::Assign
        && matches!(&a.right, Expression::CallExpression(c)
            if vue_callee_name(ctx, &c.callee).is_some_and(|n| REF_CREATORS.contains(&n) || n == "toRefs"))
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::IdentifierReference]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::IdentifierReference(id) = node.kind() else {
            return;
        };
        let Some(sym) = ctx.symbol_of(id) else { return };
        // Cheap syntactic filter before resolving the declaration.
        let parent = ctx.parent(node.id());
        let span = id.span;
        let relevant = match parent.kind() {
            AstKind::IfStatement(i) => i.test.span() == span,
            AstKind::SwitchStatement(s) => s.discriminant.span() == span,
            AstKind::UnaryExpression(_)
            | AstKind::UpdateExpression(_)
            | AstKind::BinaryExpression(_) => true,
            AstKind::TemplateLiteral(_) => true,
            AstKind::ConditionalExpression(c) => c.test.span() == span,
            AstKind::LogicalExpression(l) => l.left.span() == span,
            AstKind::AssignmentExpression(a) => {
                if a.operator == AssignmentOperator::Assign && a.left.span() != span {
                    false
                } else {
                    !(a.left.span() == span && is_ref_reassignment(ctx, a))
                }
            }
            AstKind::StaticMemberExpression(m) => {
                m.object.span() == span && !matches!(m.property.name.as_str(), "value" | "effect")
            }
            AstKind::ComputedMemberExpression(m) => {
                m.object.span() == span
                    && matches!(&m.expression, Expression::StringLiteral(s) if s.value != "value" && s.value != "effect")
            }
            _ => false,
        };
        if !relevant {
            return;
        }
        let Some((method, is_const)) = ref_creator(ctx, sym) else {
            return;
        };
        if matches!(parent.kind(), AstKind::LogicalExpression(_)) && !is_const {
            return;
        }
        ctx.report(
            span,
            format!("Add '.value' to read or write the value wrapped by '{method}()'."),
        );
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn ref_as_operand() {
        check(
            "S9019",
            "a.ts",
            r#"
import { ref, computed, toRef, reactive, toRefs, shallowRef } from 'vue';
let count = ref(0);
function increment() {
  count++; // Noncompliant {{Add '.value' to read or write the value wrapped by 'ref()'.}}
  count.value++;
}
const items = ref([]);
const hasItems = computed(() => items.value.length > 0);
if (hasItems) {} // Noncompliant {{Add '.value' to read or write the value wrapped by 'computed()'.}}
if (hasItems.value) {}
const state = reactive({ foo: 1 });
const foo = toRef(state, 'foo');
const doubled = foo * 2; // Noncompliant
const label = `Value: ${foo}`; // Noncompliant
let s = shallowRef(0);
s = 5; // Noncompliant
let { a } = toRefs(state);
a++; // Noncompliant
let isValid = ref(true);
function revalidate() { isValid = computed(() => true); }
const x = hasItems || 1; // Noncompliant
const y = 1 || hasItems;
const z = hasItems.foo; // Noncompliant
const w = foo;
const plain = 1;
if (plain) {}
"#,
        );
    }
}
