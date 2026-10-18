//! Public "static" fields should be read-only
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::PropertyDefinition]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::PropertyDefinition(p) = node.kind() else {
            return;
        };
        if !ctx.file.path.ends_with(".ts") && !ctx.file.path.ends_with(".tsx") {
            return;
        }
        if !p.r#static || p.readonly {
            return;
        }
        if matches!(
            p.accessibility,
            Some(TSAccessibility::Private | TSAccessibility::Protected)
        ) {
            return;
        }
        ctx.report(p.key.span(), "Make this public static property readonly.");
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn public_static_readonly() {
        check(
            "S1444",
            "a.ts",
            r#"
class A {
  static foo = 1; // Noncompliant {{Make this public static property readonly.}}
  public static bar = 2; // Noncompliant
  static readonly baz = 3;
  private static qux = 4;
  protected static quux = 5;
  instance = 6;
}
"#,
        );
    }
}
