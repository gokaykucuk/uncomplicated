//! Output bindings should not be named "on" or prefixed with "on"
use crate::helpers::batch_ts::{angular_bindings, metadata_entries};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// `/^on(([^a-z])|(?=$))/`: `on`, `onSave` and `on_x`, but not `online`.
fn has_on_prefix(name: &str) -> bool {
    match name.strip_prefix("on") {
        Some(rest) => rest.chars().next().is_none_or(|c| !c.is_ascii_lowercase()),
        None => false,
    }
}

/// angular-eslint checks `name` and `name: alias` texts (whitespace removed).
fn text_has_on_prefix(text: &str) -> bool {
    let compact: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    let mut parts = compact.split(':');
    has_on_prefix(parts.next().unwrap_or("")) || parts.next().is_some_and(has_on_prefix)
}

const MSG: &str = "Output bindings, including aliases, should not be named \"on\", nor prefixed with it (https://angular.dev/guide/components/outputs#choosing-event-names)";

/// True if the member is preceded by a JSDoc block containing `@deprecated`.
fn deprecated(ctx: &Ctx<'_, '_>, member_start: u32) -> bool {
    let comments = ctx.comments();
    let i = comments.partition_point(|c| c.span.end <= member_start);
    i > 0 && {
        let c = &comments[i - 1];
        let text = ctx.comment_text(c);
        c.is_block() && text.starts_with('*') && text.contains("@deprecated")
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::Class]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::Class(class) = node.kind() else {
            return;
        };
        for b in angular_bindings(ctx, class, "Output", "output") {
            // Sonar: `onRefresh = output({ alias: 'refresh' })` exposes a compliant public name;
            // deprecated members kept for compatibility are exempted too.
            let alias_compliant = b.alias.is_some_and(|(_, a)| !has_on_prefix(a));
            if has_on_prefix(b.name) && !alias_compliant && !deprecated(ctx, b.member_span.start) {
                ctx.report(b.key, MSG);
            }
            if let Some((span, alias)) = b.alias
                && has_on_prefix(alias)
            {
                ctx.report(span, MSG);
            }
        }
        for (span, text) in metadata_entries(class, "outputs") {
            if text_has_on_prefix(text) {
                ctx.report(span, MSG);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn on_prefix() {
        check(
            "S7652",
            "a.ts",
            r#"
@Component({ selector: 'app-a', outputs: ['onFoo', 'bar'] }) // Noncompliant
class A {
  @Output() onChange = new EventEmitter(); // Noncompliant {{Output bindings, including aliases, should not be named "on", nor prefixed with it (https://angular.dev/guide/components/outputs#choosing-event-names)}}
  @Output() on = new EventEmitter(); // Noncompliant
  @Output() online = new EventEmitter();
  @Output('onSave') save = new EventEmitter(); // Noncompliant
  onRefresh = output({ alias: 'refresh' });
  onClose = output(); // Noncompliant
  /** @deprecated use `closed` */
  onOld = output();
  onPlain = 1;
}
"#,
        );
    }
}
