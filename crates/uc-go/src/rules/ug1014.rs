//! Dynamic content should not bypass "html/template" escaping (gosec G203).
use crate::helpers_extra::*;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const TYPES: &[&str] = &["HTML", "HTMLAttr", "JS", "JSStr", "CSS", "URL", "Srcset"];

impl GoRule for Rule {
    fn interests(&self) -> &'static [&'static str] {
        &["call_expression"]
    }

    fn run(&self, node: Nd<'_>, ctx: &Ctx<'_>) {
        let Some(t) = pkg_call(ctx, node, &["html/template"], TYPES) else {
            return;
        };
        let args = call_args(node);
        let [arg] = args.as_slice() else { return };
        let arg = strip_parens(*arg);
        if is_literal_concat(ctx, arg)
            || (arg.kind() == "identifier" && file_consts(ctx).contains(&ctx.text(arg)))
        {
            return;
        }
        ctx.report(
            node,
            format!(
                "Make sure this value is safe: converting it to \"template.{t}\" disables escaping."
            ),
        );
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn rule() {
        check(
            "UG1014",
            r#"package p

import "html/template"

const banner = "<b>hi</b>"

func f(userInput string) {
	_ = template.HTML(userInput) // Noncompliant {{Make sure this value is safe: converting it to "template.HTML" disables escaping.}}
	_ = template.JS("x = " + userInput) // Noncompliant
	_ = template.HTML("<br>")
	_ = template.HTML(banner)
	_ = template.HTMLEscapeString(userInput)
}
"#,
        );
    }

    #[test]
    fn text_template_is_out_of_scope() {
        check(
            "UG1014",
            r#"package p

import "text/template"

func f(s string) {
	_ = template.HTML(s)
}
"#,
        );
    }
}
