//! Type constituents of unions and intersections should not be redundant
use crate::helpers::batch_ts::{fmt_number, strip_type_parens, type_span};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

#[derive(PartialEq, Eq, Clone, Copy)]
enum Prim {
    String,
    Number,
    BigInt,
    Boolean,
}

impl Prim {
    fn name(self) -> &'static str {
        match self {
            Prim::String => "string",
            Prim::Number => "number",
            Prim::BigInt => "bigint",
            Prim::Boolean => "boolean",
        }
    }
}

/// What a constituent is, as far as this rule is concerned.
enum Part {
    Any,
    Unknown,
    Never,
    Prim(Prim),
    Literal(Prim, String),
    /// A type reference we can't see through (might be a union, an error type, …).
    Opaque,
    Other,
}

/// A literal type and its display text, as the TypeScript checker prints it.
fn literal(t: &TSType<'_>) -> Option<(Prim, String)> {
    let TSType::TSLiteralType(l) = t else {
        return None;
    };
    match &l.literal {
        TSLiteral::StringLiteral(s) => Some((Prim::String, format!("{:?}", s.value.as_str()))),
        TSLiteral::NumericLiteral(n) => Some((Prim::Number, fmt_number(n.value))),
        TSLiteral::BigIntLiteral(b) => Some((Prim::BigInt, format!("{}n", b.value))),
        TSLiteral::BooleanLiteral(b) => Some((Prim::Boolean, b.value.to_string())),
        TSLiteral::UnaryExpression(u) if u.operator == UnaryOperator::UnaryNegation => {
            match &u.argument {
                Expression::NumericLiteral(n) => {
                    Some((Prim::Number, format!("-{}", fmt_number(n.value))))
                }
                _ => None,
            }
        }
        _ => None,
    }
}

/// Flattens nested unions into parts.
fn parts(ctx: &Ctx<'_, '_>, t: &TSType<'_>, out: &mut Vec<Part>) {
    let t = strip_type_parens(t);
    let part = match t {
        TSType::TSUnionType(u) => {
            for t in &u.types {
                parts(ctx, t, out);
            }
            return;
        }
        TSType::TSAnyKeyword(_) => Part::Any,
        TSType::TSUnknownKeyword(_) => Part::Unknown,
        TSType::TSNeverKeyword(_) => Part::Never,
        TSType::TSStringKeyword(_) => Part::Prim(Prim::String),
        TSType::TSNumberKeyword(_) => Part::Prim(Prim::Number),
        TSType::TSBigIntKeyword(_) => Part::Prim(Prim::BigInt),
        TSType::TSBooleanKeyword(_) => Part::Prim(Prim::Boolean),
        TSType::TSLiteralType(_) => match literal(t) {
            Some((p, s)) => Part::Literal(p, s),
            None => Part::Other,
        },
        TSType::TSTypeReference(r) => reference_part(ctx, r),
        _ => Part::Other,
    };
    out.push(part);
}

fn reference_part(ctx: &Ctx<'_, '_>, r: &TSTypeReference<'_>) -> Part {
    let TSTypeName::IdentifierReference(id) = &r.type_name else {
        return Part::Opaque;
    };
    let Some(sym) = ctx.scoping().get_reference(id.reference_id()).symbol_id() else {
        return Part::Opaque;
    };
    match ctx.symbol_decl(sym).kind() {
        AstKind::TSTypeAliasDeclaration(a)
            if a.type_parameters.is_none() && r.type_arguments.is_none() =>
        {
            match literal(strip_type_parens(&a.type_annotation)) {
                Some((p, s)) => Part::Literal(p, s),
                None => Part::Opaque,
            }
        }
        AstKind::TSInterfaceDeclaration(_) | AstKind::Class(_) => Part::Other,
        _ => Part::Opaque,
    }
}

/// True if the union is the return type of a function or function type.
fn inside_return_type(ctx: &Ctx<'_, '_>, id: NodeId) -> bool {
    let parent = ctx.parent(id);
    let AstKind::TSTypeAnnotation(_) = parent.kind() else {
        return false;
    };
    matches!(
        ctx.parent_kind(parent.id()),
        AstKind::Function(_)
            | AstKind::ArrowFunctionExpression(_)
            | AstKind::TSFunctionType(_)
            | AstKind::TSConstructorType(_)
            | AstKind::TSMethodSignature(_)
            | AstKind::TSCallSignatureDeclaration(_)
            | AstKind::TSConstructSignatureDeclaration(_)
    )
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::TSUnionType, AstType::TSIntersectionType]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        if crate::helpers::batch_ts::no_type_info(ctx) {
            return;
        }
        match node.kind() {
            AstKind::TSUnionType(u) => {
                let never_ok = inside_return_type(ctx, node.id());
                let mut prims: Vec<Prim> = Vec::new();
                let mut lits: Vec<(Span, Prim, String)> = Vec::new();
                let mut ps = Vec::new();
                for t in &u.types {
                    let span = type_span(t);
                    ps.clear();
                    parts(ctx, t, &mut ps);
                    for p in ps.drain(..) {
                        match p {
                            Part::Any => ctx.report(
                                span,
                                "'any' overrides all other types in this union type.",
                            ),
                            Part::Unknown => ctx.report(
                                span,
                                "'unknown' overrides all other types in this union type.",
                            ),
                            Part::Never if !never_ok => ctx.report(
                                span,
                                "'never' is overridden by other types in this union type.",
                            ),
                            Part::Prim(p) => prims.push(p),
                            Part::Literal(p, s) => lits.push((span, p, s)),
                            _ => {}
                        }
                    }
                }
                // Group overridden literals by type node, then by primitive.
                let mut groups: Vec<(Span, Prim, Vec<String>)> = Vec::new();
                for (span, p, s) in lits {
                    if !prims.contains(&p) {
                        continue;
                    }
                    match groups.iter_mut().find(|(sp, q, _)| *sp == span && *q == p) {
                        Some(g) => g.2.push(s),
                        None => groups.push((span, p, vec![s])),
                    }
                }
                for (span, p, texts) in groups {
                    ctx.report(
                        span,
                        format!(
                            "{} is overridden by {} in this union type.",
                            texts.join(" | "),
                            p.name()
                        ),
                    );
                }
            }
            AstKind::TSIntersectionType(i) => {
                let mut prims: Vec<(Prim, Span)> = Vec::new();
                let mut lits: Vec<(Prim, String)> = Vec::new();
                let mut uncertain = false;
                let mut ps = Vec::new();
                for t in &i.types {
                    let span = type_span(t);
                    ps.clear();
                    parts(ctx, t, &mut ps);
                    if ps.len() >= 2 {
                        uncertain = true;
                    }
                    for p in ps.drain(..) {
                        match p {
                            Part::Any => ctx.report(
                                span,
                                "'any' overrides all other types in this intersection type.",
                            ),
                            Part::Never => ctx.report(
                                span,
                                "'never' overrides all other types in this intersection type.",
                            ),
                            Part::Unknown => ctx.report(
                                span,
                                "'unknown' is overridden by other types in this intersection type.",
                            ),
                            Part::Prim(p) => prims.push((p, span)),
                            Part::Literal(p, s) => lits.push((p, s)),
                            Part::Opaque => uncertain = true,
                            Part::Other => {}
                        }
                    }
                }
                if uncertain {
                    return;
                }
                for (p, span) in prims {
                    let texts: Vec<&str> = lits
                        .iter()
                        .filter(|(q, _)| *q == p)
                        .map(|(_, s)| s.as_str())
                        .collect();
                    if !texts.is_empty() {
                        ctx.report(
                            span,
                            format!(
                                "{} is overridden by the {} in this intersection type.",
                                p.name(),
                                texts.join(" | ")
                            ),
                        );
                    }
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
    fn redundant_constituents() {
        check(
            "S6571",
            "a.ts",
            r#"
const torrent: string | any = client.get(infoHash); // Noncompliant {{'any' overrides all other types in this union type.}}
function foo(arg: string | SomeUnknownType) {}
function bar(arg: number & SomeUnknownType) {}
type FALSE = false;
type ZERO = 0;
type A = number;
type B = A | any; // Noncompliant {{'any' overrides all other types in this union type.}}
type C = number | never; // Noncompliant {{'never' is overridden by other types in this union type.}}
type D = false & boolean; // Noncompliant {{boolean is overridden by the false in this intersection type.}}
type E = FALSE & boolean; // Noncompliant {{boolean is overridden by the false in this intersection type.}}
type F = number & any; // Noncompliant {{'any' overrides all other types in this intersection type.}}
type G = number & never; // Noncompliant {{'never' overrides all other types in this intersection type.}}
type H = 0 & number; // Noncompliant {{number is overridden by the 0 in this intersection type.}}
type I = ZERO & number; // Noncompliant {{number is overridden by the 0 in this intersection type.}}
type J = '' & string; // Noncompliant {{string is overridden by the "" in this intersection type.}}
type K = 0n & bigint; // Noncompliant {{bigint is overridden by the 0n in this intersection type.}}
const x: unknown | string = '42'; // Noncompliant {{'unknown' overrides all other types in this union type.}}
type hidden = unknown;
const y: hidden | string = '42';
type L = 'a' | string; // Noncompliant {{"a" is overridden by string in this union type.}}
type M = 'a' | 'b';
type N = ZERO | number; // Noncompliant {{0 is overridden by number in this union type.}}
function ret(): string | never { return ''; }
type O = Imported & string;
"#,
        );
    }
}
