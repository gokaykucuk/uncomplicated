//! Input bindings should not be aliased
use crate::helpers::batch_ts::{alias_allowed_by_selector, angular_bindings, metadata_entries};
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
            if is_reported(class, b.name, alias) {
                ctx.report(span, MSG);
            }
        }
        for (span, text) in metadata_entries(class, "inputs") {
            let compact: String = text.chars().filter(|c| !c.is_whitespace()).collect();
            let mut parts = compact.split(':');
            let name = parts.next().unwrap_or("");
            if let Some(alias) = parts.next()
                && is_reported(class, name, alias)
            {
                ctx.report(span, MSG);
            }
        }
    }
}

const MSG: &str = "Input bindings should not be aliased (https://angular.dev/guide/components/inputs#choosing-input-names)";

/// `aria-label` → `ariaLabel`.
fn kebab_to_camel(s: &str) -> String {
    let mut out = String::new();
    let mut upper = false;
    for c in s.chars() {
        if c == '-' {
            upper = true;
        } else if upper {
            out.extend(c.to_uppercase());
            upper = false;
        } else {
            out.push(c);
        }
    }
    out
}

fn is_reported(class: &Class<'_>, name: &str, alias: &str) -> bool {
    if alias.is_empty() {
        return false;
    }
    // ARIA attributes bound to their camel-cased property are allowed.
    if alias.starts_with("aria-") && name == kebab_to_camel(alias) {
        return false;
    }
    if alias == name {
        return true;
    }
    // Sonar: aliases that are reserved words can't be used as member names.
    !alias_allowed_by_selector(class, name, alias, true) && !RESERVED_WORDS.contains(&alias)
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
@Component({ selector: 'app-a', inputs: ['x: y', 'z'] }) // Noncompliant
class A {
  @Input('foo') bar: string; // Noncompliant {{Input bindings should not be aliased (https://angular.dev/guide/components/inputs#choosing-input-names)}}
  @Input('same') same: string; // Noncompliant
  @Input() plain: string;
  @Input('aria-label') ariaLabel: string;
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
