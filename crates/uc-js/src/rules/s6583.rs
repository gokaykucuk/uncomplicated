//! Enum members should not mix value types
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

#[derive(PartialEq, Clone, Copy)]
enum Kind2 {
    Number,
    String,
}

fn member_type(ctx: &Ctx<'_, '_>, m: &TSEnumMember<'_>) -> Option<Kind2> {
    let Some(init) = &m.initializer else {
        return Some(Kind2::Number);
    };
    match strip_parens(init) {
        Expression::NumericLiteral(_) => Some(Kind2::Number),
        Expression::StringLiteral(_) | Expression::TemplateLiteral(_) => Some(Kind2::String),
        e => {
            let t = type_of(ctx, e);
            if t.is_number() {
                Some(Kind2::Number)
            } else if t.is_string() {
                Some(Kind2::String)
            } else {
                None
            }
        }
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::TSEnumDeclaration]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        if crate::helpers::batch_ts::no_type_info(ctx) {
            return;
        }
        let AstKind::TSEnumDeclaration(e) = node.kind() else {
            return;
        };
        let Some(first) = e.body.members.first() else {
            return;
        };
        // Merged enum declarations take their expected type from the first declaration.
        if let Some(sym) = e.id.symbol_id.get()
            && !ctx.scoping().symbol_redeclarations(sym).is_empty()
        {
            return;
        }
        let Some(desired) = member_type(ctx, first) else {
            return;
        };
        for m in &e.body.members {
            let Some(t) = member_type(ctx, m) else { return };
            if t != desired {
                let span = m.initializer.as_ref().map_or(m.span, |i| i.span());
                ctx.report(span, "Mixing number and string enums can be confusing.");
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn mixed() {
        check(
            "S6583",
            "a.ts",
            r#"
enum A {
  X = 0,
  Y = 'y', // Noncompliant {{Mixing number and string enums can be confusing.}}
  Z = 'z',
}
enum B {
  X = 'x',
  Y, // Noncompliant
}
enum C { X, Y = 1, Z = -1 }
enum D { X = 'x', Y = `y` }
enum E { X = 0, Y = foo(), Z = 'z' }
"#,
        );
    }
}
