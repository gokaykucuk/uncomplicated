//! Function types should be preferred
use oxc_ast_visit::Visit;

use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// Span of the single member, if it is a call or construct signature with a return type.
fn signature_span(members: &[TSSignature<'_>]) -> Option<Span> {
    match members {
        [TSSignature::TSCallSignatureDeclaration(c)] if c.return_type.is_some() => Some(c.span),
        [TSSignature::TSConstructSignatureDeclaration(c)] if c.return_type.is_some() => {
            Some(c.span)
        }
        _ => None,
    }
}

/// Finds the first `this` type, not looking into nested type literals.
#[derive(Default)]
struct ThisType(Option<Span>);

impl<'a> Visit<'a> for ThisType {
    fn visit_ts_this_type(&mut self, t: &TSThisType) {
        if self.0.is_none() {
            self.0 = Some(t.span);
        }
    }
    fn visit_ts_type_literal(&mut self, _: &TSTypeLiteral<'a>) {}
}

fn message(kind: &str) -> String {
    format!("{kind} has only a call signature, you should use a function type instead.")
}

fn call_only(members: &[TSSignature<'_>]) -> bool {
    matches!(members, [TSSignature::TSCallSignatureDeclaration(_)])
}

/// True if the instantiation at `id` is the type argument list of a `defineEmits<...>()` call.
fn is_define_emits_args(ctx: &Ctx<'_, '_>, id: NodeId) -> bool {
    let AstKind::TSTypeParameterInstantiation(_) = ctx.node(id).kind() else {
        return false;
    };
    matches!(ctx.parent_kind(id), AstKind::CallExpression(c)
        if matches!(&c.callee, Expression::Identifier(f) if f.name == "defineEmits"))
}

/// Sonar exempts call signatures used to type Vue's `defineEmits` (in `.vue` files).
fn used_by_define_emits(
    ctx: &Ctx<'_, '_>,
    node: &AstNode<'_>,
    name: Option<&BindingIdentifier<'_>>,
) -> bool {
    if !ctx.file.path.ends_with(".vue") {
        return false;
    }
    match name {
        // Inline literal: `defineEmits<{ (e: 'close'): void }>()`.
        None => is_define_emits_args(ctx, ctx.parent(node.id()).id()),
        // Interface or alias referenced as `defineEmits<Emits>()`.
        Some(id) => id.symbol_id.get().is_some_and(|sym| {
            ctx.scoping().get_resolved_references(sym).any(|r| {
                let reference = ctx.parent(r.node_id());
                matches!(reference.kind(), AstKind::TSTypeReference(_))
                    && is_define_emits_args(ctx, ctx.parent(reference.id()).id())
            })
        }),
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::TSInterfaceDeclaration, AstType::TSTypeLiteral]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        match node.kind() {
            AstKind::TSInterfaceDeclaration(i) => {
                let one_supertype = match i.extends.as_slice() {
                    [] => false,
                    [h] => {
                        !matches!(&h.type_name, TSTypeName::IdentifierReference(id) if id.name == "Function")
                    }
                    _ => true,
                };
                if one_supertype {
                    return;
                }
                let Some(span) = signature_span(&i.body.body) else {
                    return;
                };
                let mut this = ThisType::default();
                this.visit_ts_interface_declaration(i);
                match this.0 {
                    Some(t) => ctx.report(
                        t,
                        format!(
                            "`this` refers to the function type '{}', did you intend to use a generic `this` parameter like `<Self>(this: Self, ...) => Self` instead?",
                            i.id.name
                        ),
                    ),
                    None if call_only(&i.body.body) && used_by_define_emits(ctx, node, Some(&i.id)) => {}
                    None => ctx.report(span, message("Interface")),
                }
            }
            AstKind::TSTypeLiteral(l) => {
                if let Some(span) = signature_span(&l.members) {
                    if call_only(&l.members) {
                        let alias = match ctx.parent_kind(node.id()) {
                            AstKind::TSTypeAliasDeclaration(a) => Some(&a.id),
                            _ => None,
                        };
                        if used_by_define_emits(ctx, node, alias) {
                            return;
                        }
                    }
                    ctx.report(span, message("Type literal"));
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn function_type() {
        check(
            "S6598",
            "a.ts",
            r#"
interface Example {
  (): string; // Noncompliant {{Interface has only a call signature, you should use a function type instead.}}
}
function foo(example: { (): number }): number { // Noncompliant {{Type literal has only a call signature, you should use a function type instead.}}
  return example();
}
interface F extends Function {
  (x: number): void; // Noncompliant
}
interface G extends H {
  (x: number): void;
}
interface Two {
  (): void;
  prop: string;
}
interface Self {
  (arg: string): this; // Noncompliant {{`this` refers to the function type 'Self', did you intend to use a generic `this` parameter like `<Self>(this: Self, ...) => Self` instead?}}
}
interface Ctor {
  new (): Foo; // Noncompliant
}
type T = () => void;
"#,
        );
    }

    #[test]
    fn vue_define_emits() {
        check(
            "S6598",
            "a.vue",
            r#"<script setup lang="ts">
const emit = defineEmits<{ (e: 'close'): void }>()
interface Emits { (e: 'save'): void }
defineEmits<Emits>()
type Other = { (x: number): void } // Noncompliant
</script>
"#,
        );
    }
}
