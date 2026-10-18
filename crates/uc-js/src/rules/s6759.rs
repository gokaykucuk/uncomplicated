//! React props should be read-only
//!
//! Without the TypeScript checker, only props types that can be resolved locally (type literals,
//! `Readonly<...>`, local interfaces and type aliases without heritage) are judged.
use crate::helpers::batch_react::*;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// `Some(true)` read-only, `Some(false)` mutable, `None` unknown.
fn members_readonly(members: &[TSSignature<'_>]) -> Option<bool> {
    for m in members {
        match m {
            TSSignature::TSPropertySignature(p) => {
                if !p.readonly {
                    return Some(false);
                }
            }
            TSSignature::TSMethodSignature(_) => return Some(false),
            TSSignature::TSIndexSignature(_) => {}
            _ => return None,
        }
    }
    Some(true)
}

fn type_readonly(ctx: &Ctx<'_, '_>, t: &TSType<'_>, depth: u8) -> Option<bool> {
    if depth > 4 {
        return None;
    }
    match t {
        TSType::TSTypeLiteral(l) => members_readonly(&l.members),
        TSType::TSParenthesizedType(p) => type_readonly(ctx, &p.type_annotation, depth + 1),
        TSType::TSStringKeyword(_)
        | TSType::TSNumberKeyword(_)
        | TSType::TSBooleanKeyword(_)
        | TSType::TSBigIntKeyword(_)
        | TSType::TSSymbolKeyword(_)
        | TSType::TSNullKeyword(_)
        | TSType::TSUndefinedKeyword(_) => Some(true),
        TSType::TSTypeReference(r) => {
            let TSTypeName::IdentifierReference(id) = &r.type_name else {
                return None;
            };
            if id.name == "Readonly" && r.type_arguments.is_some() {
                return Some(true);
            }
            let sym = ctx.scoping().get_reference(id.reference_id()).symbol_id()?;
            if ctx.scoping().symbol_declarations(sym).count() != 1 {
                return None;
            }
            match ctx.symbol_decl(sym).kind() {
                AstKind::TSInterfaceDeclaration(i)
                    if i.extends.is_empty() && i.type_parameters.is_none() =>
                {
                    members_readonly(&i.body.body)
                }
                AstKind::TSTypeAliasDeclaration(a) if a.type_parameters.is_none() => {
                    type_readonly(ctx, &a.type_annotation, depth + 1)
                }
                _ => None,
            }
        }
        _ => None,
    }
}

fn returns_jsx_value<'a>(ctx: &Ctx<'a, '_>, e: &Expression<'a>) -> bool {
    let e = strip_parens(e);
    if matches!(e, Expression::JSXElement(_) | Expression::JSXFragment(_)) {
        return true;
    }
    // getUniqueWriteUsageOrNode
    if let Expression::Identifier(id) = e
        && let Some(sym) = ctx.symbol_of(id)
        && let AstKind::VariableDeclarator(v) = ctx.symbol_decl(sym).kind()
        && !ctx
            .scoping()
            .get_resolved_references(sym)
            .any(|r| r.is_write())
        && let Some(init) = &v.init
    {
        return matches!(
            strip_parens(init),
            Expression::JSXElement(_) | Expression::JSXFragment(_)
        );
    }
    false
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::Function]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::Function(f) = node.kind() else {
            return;
        };
        if !f.is_function_declaration()
            || f.params.items.len() + usize::from(f.params.rest.is_some()) > 1
        {
            return;
        }
        let Some(name) = &f.id else { return };
        if !name
            .name
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_uppercase)
        {
            return;
        }
        let Some(body) = &f.body else { return };
        if !any_return(&body.statements, &mut |r| {
            r.argument
                .as_ref()
                .is_some_and(|e| returns_jsx_value(ctx, e))
        }) {
            return;
        }
        let Some(props) = f.params.items.first() else {
            return;
        };
        let Some(ann) = &props.type_annotation else {
            return;
        };
        if type_readonly(ctx, &ann.type_annotation, 0) == Some(false) {
            ctx.report(props.span, "Mark the props of the component as read-only.");
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn readonly_props() {
        check(
            "S6759",
            "a.tsx",
            r#"
interface FooProps { x: number; y: string; }
function Foo(props: FooProps) { // Noncompliant {{Mark the props of the component as read-only.}}
  return <div>{props.x}</div>;
}
type BarProps = { x: number; y: string; };
function Bar(props: BarProps) { // Noncompliant
  return <div>{props.x}</div>;
}
function Baz(props: Readonly<FooProps>) { return <div />; }
type QuxProps = { readonly x: number; readonly y: string; };
function Qux(props: QuxProps) { return <div />; }
function Corge(props: { readonly x: number }) { return <div />; }
function Inline({ x }: { x: number }) { // Noncompliant
  return <div>{x}</div>;
}
interface Ext extends FooProps { z: number; }
function E(props: Ext) { return <div />; }
function J(props: UnknownProps) { return <div />; }
function G(props: number) { return <div />; }
function lower(props: FooProps) { return <div />; }
function D(props: FooProps) { return 42; }
function V(props: FooProps) { // Noncompliant
  const el = <div />;
  return el;
}
"#,
        );
    }
}
