//! Default type parameters should be omitted
use crate::helpers::batch_ts::strip_type_parens;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

fn ref_symbol(ctx: &Ctx<'_, '_>, id: &IdentifierReference<'_>) -> Option<SymbolId> {
    ctx.scoping().get_reference(id.reference_id()).symbol_id()
}

/// Syntactic type equality, for simple types whose identity doesn't depend on context.
fn same_type(ctx: &Ctx<'_, '_>, a: &TSType<'_>, b: &TSType<'_>) -> bool {
    let (a, b) = (strip_type_parens(a), strip_type_parens(b));
    match (a, b) {
        (TSType::TSStringKeyword(_), TSType::TSStringKeyword(_))
        | (TSType::TSNumberKeyword(_), TSType::TSNumberKeyword(_))
        | (TSType::TSBooleanKeyword(_), TSType::TSBooleanKeyword(_))
        | (TSType::TSBigIntKeyword(_), TSType::TSBigIntKeyword(_))
        | (TSType::TSSymbolKeyword(_), TSType::TSSymbolKeyword(_))
        | (TSType::TSAnyKeyword(_), TSType::TSAnyKeyword(_))
        | (TSType::TSUnknownKeyword(_), TSType::TSUnknownKeyword(_))
        | (TSType::TSNeverKeyword(_), TSType::TSNeverKeyword(_))
        | (TSType::TSVoidKeyword(_), TSType::TSVoidKeyword(_))
        | (TSType::TSNullKeyword(_), TSType::TSNullKeyword(_))
        | (TSType::TSUndefinedKeyword(_), TSType::TSUndefinedKeyword(_))
        | (TSType::TSObjectKeyword(_), TSType::TSObjectKeyword(_)) => true,
        (TSType::TSLiteralType(x), TSType::TSLiteralType(y)) => {
            ctx.text(x.span) == ctx.text(y.span)
        }
        (TSType::TSTypeReference(x), TSType::TSTypeReference(y))
            if x.type_arguments.is_none() && y.type_arguments.is_none() =>
        {
            match (&x.type_name, &y.type_name) {
                (TSTypeName::IdentifierReference(p), TSTypeName::IdentifierReference(q)) => {
                    match (ref_symbol(ctx, p), ref_symbol(ctx, q)) {
                        (Some(s), Some(t)) => {
                            s == t
                                && !matches!(ctx.symbol_decl(s).kind(), AstKind::TSTypeParameter(_))
                        }
                        (None, None) => p.name == q.name,
                        _ => false,
                    }
                }
                _ => false,
            }
        }
        _ => false,
    }
}

fn decl_type_params<'a>(
    kind: AstKind<'a>,
    for_call: bool,
) -> Option<&'a TSTypeParameterDeclaration<'a>> {
    match kind {
        AstKind::TSInterfaceDeclaration(i) if !for_call => i.type_parameters.as_deref(),
        AstKind::TSTypeAliasDeclaration(a) if !for_call => a.type_parameters.as_deref(),
        AstKind::Class(c) => c.type_parameters.as_deref(),
        AstKind::Function(f) if for_call => f.type_parameters.as_deref(),
        _ => None,
    }
}

fn check(
    ctx: &Ctx<'_, '_>,
    name: &IdentifierReference<'_>,
    args: &TSTypeParameterInstantiation<'_>,
    for_call: bool,
) {
    let Some(sym) = ref_symbol(ctx, name) else {
        return;
    };
    if !ctx.scoping().symbol_redeclarations(sym).is_empty() {
        return;
    }
    let Some(params) = decl_type_params(ctx.symbol_decl(sym).kind(), for_call) else {
        return;
    };
    let Some(last) = args.params.last() else {
        return;
    };
    let Some(param) = params.params.get(args.params.len() - 1) else {
        return;
    };
    let Some(default) = &param.default else {
        return;
    };
    if same_type(ctx, last, default) {
        ctx.report(
            last.span(),
            "This is the default value for this type parameter, so it can be omitted.",
        );
    }
}

fn ident_name<'b, 'a>(e: &'b Expression<'a>) -> Option<&'b IdentifierReference<'a>> {
    match strip_parens(e) {
        Expression::Identifier(id) => Some(id),
        _ => None,
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::TSTypeParameterInstantiation]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::TSTypeParameterInstantiation(args) = node.kind() else {
            return;
        };
        match ctx.parent_kind(node.id()) {
            AstKind::TSTypeReference(r) => {
                if let TSTypeName::IdentifierReference(id) = &r.type_name {
                    check(ctx, id, args, false);
                }
            }
            AstKind::TSInterfaceHeritage(h) => {
                if let TSTypeName::IdentifierReference(id) = &h.type_name {
                    check(ctx, id, args, false);
                }
            }
            AstKind::TSClassImplements(h) => {
                if let TSTypeName::IdentifierReference(id) = &h.expression {
                    check(ctx, id, args, false);
                }
            }
            AstKind::Class(c) => {
                if let Some(h) = &c.heritage
                    && h.type_arguments
                        .as_ref()
                        .is_some_and(|t| t.span == args.span)
                    && let Some(id) = ident_name(&h.expression)
                {
                    check(ctx, id, args, false);
                }
            }
            AstKind::CallExpression(c) => {
                if let Some(id) = ident_name(&c.callee) {
                    check(ctx, id, args, true);
                }
            }
            AstKind::NewExpression(n) => {
                if let Some(id) = ident_name(&n.callee) {
                    check(ctx, id, args, false);
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
    fn default_type_args() {
        check(
            "S4157",
            "a.ts",
            r#"
function f<T = number>() {}
f<number>(); // Noncompliant {{This is the default value for this type parameter, so it can be omitted.}}
f<string>();
interface I<A, B = string> {}
let a: I<number, string>; // Noncompliant
let b: I<number, number>;
let c: I<string>;
class C<T = boolean> {}
new C<boolean>(); // Noncompliant
class D extends C<boolean> {} // Noncompliant
type Al<T = Foo> = T[];
let d: Al<Foo>; // Noncompliant
let e: Al<Bar>;
let g: Map<string, number>;
"#,
        );
    }
}
