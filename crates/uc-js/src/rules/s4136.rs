//! Method overloads should be grouped together
use std::borrow::Cow;

use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// Kind of member name, as typescript-eslint's `MemberNameType`.
#[derive(PartialEq, Eq, Clone, Copy, Hash)]
enum NameType {
    Normal,
    Private,
    Quoted,
    Expression,
}

#[derive(PartialEq, Eq, Clone, Hash)]
struct Method<'a> {
    name: Cow<'a, str>,
    name_type: NameType,
    is_static: bool,
    call_signature: bool,
}

impl<'a> Method<'a> {
    fn named(name: &'a str, call_signature: bool) -> Self {
        Method {
            name: Cow::Borrowed(name),
            name_type: NameType::Normal,
            is_static: false,
            call_signature,
        }
    }

    fn member(ctx: &Ctx<'a, '_>, key: &PropertyKey<'a>, is_static: bool) -> Self {
        let (name, name_type) = key_name(ctx, key);
        Method {
            name,
            name_type,
            is_static,
            call_signature: false,
        }
    }
}

fn is_identifier_name(v: &str) -> bool {
    v.chars()
        .next()
        .is_some_and(|c| c.is_alphabetic() || c == '_' || c == '$')
        && v.chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '$')
}

fn key_name<'a>(ctx: &Ctx<'a, '_>, key: &PropertyKey<'a>) -> (Cow<'a, str>, NameType) {
    let literal = |v: Cow<'a, str>| {
        if is_identifier_name(&v) {
            (v, NameType::Normal)
        } else {
            (Cow::Owned(format!("\"{v}\"")), NameType::Quoted)
        }
    };
    match key {
        PropertyKey::StaticIdentifier(id) => (Cow::Borrowed(id.name.as_str()), NameType::Normal),
        PropertyKey::Identifier(id) => (Cow::Borrowed(id.name.as_str()), NameType::Normal),
        PropertyKey::PrivateIdentifier(id) => {
            (Cow::Owned(format!("#{}", id.name)), NameType::Private)
        }
        PropertyKey::StringLiteral(s) => literal(Cow::Borrowed(s.value.as_str())),
        PropertyKey::NumericLiteral(n) => {
            literal(Cow::Owned(crate::helpers::batch_ts::fmt_number(n.value)))
        }
        k => (Cow::Borrowed(ctx.text(k.span())), NameType::Expression),
    }
}

fn function_method<'a>(f: &Function<'a>) -> Option<Method<'a>> {
    f.id.as_ref()
        .map(|id| Method::named(id.name.as_str(), false))
}

fn statement_method<'a>(s: &Statement<'a>) -> Option<Method<'a>> {
    match s {
        Statement::FunctionDeclaration(f) => function_method(f),
        Statement::ExportDeclaration(e) => match &e.declaration {
            Declaration::FunctionDeclaration(f) => function_method(f),
            _ => None,
        },
        Statement::ExportDefaultDeclaration(e) => match &e.declaration {
            ExportDefaultDeclarationKind::FunctionDeclaration(f) => function_method(f),
            _ => None,
        },
        _ => None,
    }
}

fn signature_method<'a>(ctx: &Ctx<'a, '_>, s: &TSSignature<'a>) -> Option<Method<'a>> {
    match s {
        TSSignature::TSMethodSignature(m) => Some(Method::member(ctx, &m.key, false)),
        TSSignature::TSCallSignatureDeclaration(_) => Some(Method::named("call", true)),
        TSSignature::TSConstructSignatureDeclaration(_) => Some(Method::named("new", false)),
        _ => None,
    }
}

fn check<'a>(ctx: &Ctx<'a, '_>, members: impl Iterator<Item = (Span, Option<Method<'a>>)>) {
    let mut seen: FxHashSet<Method<'a>> = FxHashSet::default();
    let mut last: Option<Method<'a>> = None;
    for (span, method) in members {
        let Some(method) = method else {
            last = None;
            continue;
        };
        let already = seen.contains(&method);
        if already && last.as_ref() != Some(&method) {
            ctx.report(
                span,
                format!(
                    "All {}{} signatures should be adjacent.",
                    if method.is_static { "static " } else { "" },
                    method.name
                ),
            );
        } else if !already {
            seen.insert(method.clone());
        }
        last = Some(method);
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[
            AstType::Program,
            AstType::BlockStatement,
            AstType::FunctionBody,
            AstType::TSModuleBlock,
            AstType::ClassBody,
            AstType::TSInterfaceBody,
            AstType::TSTypeLiteral,
        ]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        match node.kind() {
            AstKind::Program(p) => {
                check(ctx, p.body.iter().map(|s| (s.span(), statement_method(s))))
            }
            AstKind::BlockStatement(b) => {
                check(ctx, b.body.iter().map(|s| (s.span(), statement_method(s))))
            }
            AstKind::FunctionBody(b) => check(
                ctx,
                b.statements.iter().map(|s| (s.span(), statement_method(s))),
            ),
            AstKind::TSModuleBlock(b) => {
                check(ctx, b.body.iter().map(|s| (s.span(), statement_method(s))))
            }
            AstKind::ClassBody(b) => check(
                ctx,
                b.body.iter().map(|e| {
                    let m = match e {
                        ClassElement::MethodDefinition(m) => {
                            Some(Method::member(ctx, &m.key, m.r#static))
                        }
                        _ => None,
                    };
                    (e.span(), m)
                }),
            ),
            AstKind::TSInterfaceBody(b) => check(
                ctx,
                b.body.iter().map(|s| (s.span(), signature_method(ctx, s))),
            ),
            AstKind::TSTypeLiteral(l) => check(
                ctx,
                l.members
                    .iter()
                    .map(|s| (s.span(), signature_method(ctx, s))),
            ),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn adjacent() {
        check(
            "S4136",
            "a.ts",
            r#"
export function foo(s: string): void;
export function foo(n: number): void;
export function bar(): void {}
export function foo(sn: string | number): void {} // Noncompliant {{All foo signatures should be adjacent.}}

interface I {
  m(a: string): void;
  x: number;
  m(a: number): void; // Noncompliant {{All m signatures should be adjacent.}}
  (): void;
  n(): void;
  (a: string): void; // Noncompliant {{All call signatures should be adjacent.}}
}
class C {
  static s(): void;
  s(): void;
  static s(a?: any) {} // Noncompliant {{All static s signatures should be adjacent.}}
  s(a?: any) {} // Noncompliant
  get g() { return 1; }
  other() {}
  set g(v) {} // Noncompliant {{All g signatures should be adjacent.}}
}
class D {
  a(x: string): void;
  a(x: any) {}
  b() {}
}
type T = { f(): void; f(a: string): void };
"#,
        );
    }
}
