//! Input bindings should not be aliased
use crate::helpers::batch_ts::{alias_allowed_by_selector, angular_bindings};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const RESERVED_WORDS: &[&str] = &[
    "await",
    "break",
    "case",
    "catch",
    "class",
    "const",
    "continue",
    "debugger",
    "default",
    "delete",
    "do",
    "else",
    "enum",
    "export",
    "extends",
    "false",
    "finally",
    "for",
    "function",
    "if",
    "implements",
    "import",
    "in",
    "instanceof",
    "interface",
    "let",
    "new",
    "null",
    "package",
    "private",
    "protected",
    "public",
    "return",
    "static",
    "super",
    "switch",
    "this",
    "throw",
    "true",
    "try",
    "typeof",
    "var",
    "void",
    "while",
    "with",
    "yield",
];

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::Class]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::Class(class) = node.kind() else {
            return;
        };
        for b in angular_bindings(ctx, class, "Input", "input") {
            let Some((span, alias)) = b.alias else {
                continue;
            };
            if alias.is_empty()
                || (alias != b.name && alias_allowed_by_selector(class, b.name, alias))
            {
                continue;
            }
            if RESERVED_WORDS.contains(&alias) && alias != b.name {
                continue;
            }
            ctx.report(span, "Input bindings should not be aliased");
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn input_rename() {
        check(
            "S7649",
            "a.ts",
            r#"
import { input } from '@angular/core';
@Component({ selector: 'app-a' })
class A {
  @Input('foo') bar: string; // Noncompliant {{Input bindings should not be aliased}}
  @Input('same') same: string; // Noncompliant
  @Input() plain: string;
  @Input('class') className: string;
  sig = input('', { alias: 'other' }); // Noncompliant
  sig2 = input('');
}
@Directive({ selector: '[tooltip]' })
class D {
  @Input('tooltip') text: string;
  @Input('tooltipColor') color: string;
}
"#,
        );
    }
}
