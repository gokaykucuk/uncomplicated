//! ARIA properties in DOM elements should have valid values
use crate::helpers::batch_a11y::*;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

fn token_matches(prop: &AriaProp, value: &Val<'_>) -> bool {
    match value {
        Val::Str(s) => {
            let lower = s.to_ascii_lowercase();
            prop.values
                .iter()
                .any(|(v, is_bool)| !is_bool && *v == lower)
        }
        Val::Bool(b) => prop
            .values
            .iter()
            .any(|(v, is_bool)| *is_bool && *v == if *b { "true" } else { "false" }),
        _ => false,
    }
}

fn is_valid(prop: &AriaProp, value: &Val<'_>) -> bool {
    match prop.ty {
        AriaType::Boolean => matches!(value, Val::Bool(_)),
        AriaType::Str | AriaType::Id => matches!(value, Val::Str(_)),
        AriaType::Tristate => matches!(value, Val::Bool(_)) || value.is_str("mixed"),
        AriaType::Integer | AriaType::Number => {
            !matches!(value, Val::Bool(_)) && value.to_number().is_some_and(|n| !n.is_nan())
        }
        AriaType::Token => token_matches(prop, value),
        AriaType::IdList => matches!(value, Val::Str(_)),
        AriaType::TokenList => match value {
            Val::Str(s) => s.split(' ').all(|t| {
                let t = t.to_ascii_lowercase();
                prop.values.iter().any(|(v, is_bool)| !is_bool && *v == t)
            }),
            _ => false,
        },
    }
}

fn message(name: &str, prop: &AriaProp) -> String {
    let values = || {
        prop.values
            .iter()
            .map(|(v, _)| *v)
            .collect::<Vec<_>>()
            .join(",")
    };
    match prop.ty {
        AriaType::Tristate => {
            format!("The value for {name} must be a boolean or the string \"mixed\".")
        }
        AriaType::Token => format!(
            "The value for {name} must be a single token from the following: {}.",
            values()
        ),
        AriaType::TokenList => {
            format!(
                "The value for {name} must be a list of one or more tokens from the following: {}.",
                values()
            )
        }
        AriaType::IdList => {
            format!(
                "The value for {name} must be a list of strings that represent DOM element IDs (idlist)"
            )
        }
        AriaType::Id => {
            format!("The value for {name} must be a string that represents a DOM element ID")
        }
        AriaType::Boolean => format!("The value for {name} must be a boolean."),
        AriaType::Str => format!("The value for {name} must be a string."),
        AriaType::Integer => format!("The value for {name} must be a integer."),
        AriaType::Number => format!("The value for {name} must be a number."),
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::JSXAttribute]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::JSXAttribute(a) = node.kind() else {
            return;
        };
        let name = attr_name(a);
        if name.len() < 5 || !name[..5].eq_ignore_ascii_case("aria-") {
            return;
        }
        let Some(prop) = aria_prop(&name.to_ascii_lowercase()) else {
            return;
        };
        let p = attr_prop(a);
        if prop_value(&p).is_nullish() {
            return;
        }
        let value = literal_value(&p);
        // Template literals with substitutions are not static values: stay silent on them.
        if value == Val::Dynamic || is_interpolated(&p) {
            return;
        }
        if is_valid(prop, &value) || (prop.allow_undefined && value == Val::Undefined) {
            return;
        }
        ctx.report(a.span, message(name, prop));
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn aria_proptypes() {
        check(
            "S6793",
            "a.jsx",
            r#"
<div aria-hidden="yes" />; // Noncompliant {{The value for aria-hidden must be a boolean.}}
<div aria-pressed="maybe" />; // Noncompliant {{The value for aria-pressed must be a boolean or the string "mixed".}}
<div aria-level="high" />; // Noncompliant {{The value for aria-level must be a integer.}}
<div aria-valuenow={true} />; // Noncompliant {{The value for aria-valuenow must be a number.}}
<div aria-label={true} />; // Noncompliant {{The value for aria-label must be a string.}}
<div aria-activedescendant={5} />; // Noncompliant {{The value for aria-activedescendant must be a string that represents a DOM element ID}}
<div aria-labelledby={5} />; // Noncompliant {{The value for aria-labelledby must be a list of strings that represent DOM element IDs (idlist)}}
<div aria-autocomplete="yes" />; // Noncompliant {{The value for aria-autocomplete must be a single token from the following: inline,list,both,none.}}
<div aria-relevant="additions foo" />; // Noncompliant {{The value for aria-relevant must be a list of one or more tokens from the following: additions,all,removals,text.}}
<div aria-haspopup="yes" />; // Noncompliant {{The value for aria-haspopup must be a single token from the following: false,true,menu,listbox,tree,grid,dialog.}}
<div aria-hidden="true" />;
<div aria-hidden={false} />;
<div aria-hidden />;
<div aria-pressed="mixed" />;
<div aria-level="2" />;
<div aria-level={2} />;
<div aria-valuenow={-1} />;
<div aria-label="Close" />;
<div aria-label={label} />;
<div aria-labelledby="a b" />;
<div aria-autocomplete="Inline" />;
<div aria-relevant="additions text" />;
<div aria-haspopup="true" />;
<div aria-haspopup="menu" />;
<div aria-expanded={undefined} />;
<div aria-expanded={null} />;
<div aria-foo="bar" />;
<div aria-hidden={`${x}`} />;
"#,
        );
    }
}
