//! Angular built-in sanitization should not be disabled
use crate::helpers::batch_security2::*;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const BYPASS: [&str; 5] = [
    "bypassSecurityTrustHtml",
    "bypassSecurityTrustStyle",
    "bypassSecurityTrustScript",
    "bypassSecurityTrustUrl",
    "bypassSecurityTrustResourceUrl",
];

fn is_hardcoded<'a>(ctx: &Ctx<'a, '_>, e: &'a Expression<'a>, depth: u8) -> bool {
    if depth > 10 {
        return false;
    }
    let e = strip(e);
    match e {
        Expression::TemplateLiteral(t) => t.expressions.is_empty(),
        Expression::StringLiteral(_)
        | Expression::NumericLiteral(_)
        | Expression::BooleanLiteral(_)
        | Expression::NullLiteral(_)
        | Expression::RegExpLiteral(_)
        | Expression::BigIntLiteral(_) => true,
        Expression::Identifier(_) => {
            let r = resolve(ctx, e);
            !std::ptr::eq(r, e) && is_hardcoded(ctx, r, depth + 1)
        }
        Expression::StaticMemberExpression(m) => {
            let Some(obj) = resolve_object(ctx, &m.object) else {
                return false;
            };
            match get_property(ctx, obj, m.property.name.as_str()) {
                Prop::Found(p) => is_hardcoded(ctx, &p.value, depth + 1),
                _ => false,
            }
        }
        _ => false,
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
        let (name, span) = match strip(&call.callee) {
            Expression::StaticMemberExpression(m) => (m.property.name.as_str(), m.property.span),
            Expression::ComputedMemberExpression(m) => match &m.expression {
                Expression::StringLiteral(s) => (s.value.as_str(), s.span),
                _ => return,
            },
            _ => return,
        };
        if !BYPASS.contains(&name) || call.arguments.len() != 1 {
            return;
        }
        if let Some(a) = arg(&call.arguments, 0)
            && is_hardcoded(ctx, a, 0)
        {
            return;
        }
        ctx.report(
            span,
            "Make sure disabling Angular built-in sanitization is safe here.",
        );
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn rule() {
        check(
            "S6268",
            "a.ts",
            r#"
class C {
  constructor(private sanitizer: DomSanitizer) {}
  f(value: string) {
    this.sanitizer.bypassSecurityTrustHtml(value); // Noncompliant {{Make sure disabling Angular built-in sanitization is safe here.}}
    this.sanitizer.bypassSecurityTrustUrl(`${value}`); // Noncompliant
    this.sanitizer.bypassSecurityTrustHtml('<b>hi</b>');
    this.sanitizer.bypassSecurityTrustStyle(`color: red`);
    const html = '<b>x</b>';
    this.sanitizer.bypassSecurityTrustHtml(html);
    const cfg = { url: 'https://x' };
    this.sanitizer.bypassSecurityTrustResourceUrl(cfg.url);
    this.sanitizer.bypassSecurityTrustScript(a, b);
  }
}
"#,
        );
    }
}
