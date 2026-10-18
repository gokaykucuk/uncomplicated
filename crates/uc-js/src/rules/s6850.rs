//! Heading elements should have accessible content
use crate::helpers::batch_a11y::*;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const MAX_DEPTH: u8 = 8;

/// Follows TS wrappers and sole-write variables.
fn resolve<'a>(ctx: &Ctx<'a, '_>, mut e: &'a Expression<'a>) -> &'a Expression<'a> {
    for _ in 0..MAX_DEPTH {
        e = strip_ts(e);
        let Expression::Identifier(id) = e else {
            return e;
        };
        match sole_init(ctx, id) {
            Some(init) => e = init,
            None => return e,
        }
    }
    e
}

fn is_proven_content<'a>(ctx: &Ctx<'a, '_>, e: &'a Expression<'a>) -> bool {
    match resolve(ctx, e) {
        Expression::StringLiteral(s) => !s.value.is_empty(),
        Expression::NumericLiteral(_) | Expression::BigIntLiteral(_) => true,
        _ => false,
    }
}

/// `Some(true)`: the spread provably supplies non-empty `children`; `Some(false)`: it provably
/// doesn't or can't be known; `None`: it carries no named prop at all.
fn spread_settles<'a>(ctx: &Ctx<'a, '_>, arg: &'a Expression<'a>, depth: u8) -> Option<bool> {
    let value = resolve(ctx, arg);
    if matches!(
        value,
        Expression::TemplateLiteral(_) | Expression::ArrayExpression(_)
    ) || value.is_literal()
    {
        return None;
    }
    let Expression::ObjectExpression(o) = value else {
        return Some(false);
    };
    if depth > MAX_DEPTH {
        return Some(false);
    }
    for p in o.properties.iter().rev() {
        match p {
            ObjectPropertyKind::SpreadProperty(s) => {
                if let Some(s) = spread_settles(ctx, &s.argument, depth + 1) {
                    return Some(s);
                }
            }
            ObjectPropertyKind::ObjectProperty(p) => {
                let is_children = match &p.key {
                    PropertyKey::StaticIdentifier(k) => !p.computed && k.name == "children",
                    PropertyKey::StringLiteral(s) => s.value == "children",
                    _ => false,
                };
                if is_children {
                    return Some(
                        p.kind == PropertyKind::Init
                            && !p.method
                            && is_proven_content(ctx, &p.value),
                    );
                }
                if p.computed && !matches!(p.key, PropertyKey::StringLiteral(_)) {
                    return Some(false);
                }
            }
        }
    }
    None
}

fn spread_proves_content<'a>(ctx: &Ctx<'a, '_>, el: &'a JSXOpeningElement<'a>) -> bool {
    for item in el.attributes.iter().rev() {
        if let JSXAttributeItem::SpreadAttribute(s) = item
            && let Some(settled) = spread_settles(ctx, &s.argument, 0)
        {
            return settled;
        }
    }
    false
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::JSXOpeningElement]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::JSXOpeningElement(el) = node.kind() else {
            return;
        };
        let tag = element_name(el);
        if !matches!(tag, "h1" | "h2" | "h3" | "h4" | "h5" | "h6") {
            return;
        }
        let Some(element) = owner_element(ctx, node.id()) else {
            return;
        };
        if has_accessible_child(element) || is_hidden_from_screen_reader(tag, el) {
            return;
        }
        if element.children.is_empty() && spread_proves_content(ctx, el) {
            return;
        }
        ctx.report(
            el.span,
            "Headings must have content and the content must be accessible by a screen reader.",
        );
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn heading_content() {
        check(
            "S6850",
            "a.tsx",
            r##"
<h1 {...{ children: 'Title' }} />;
<h1 {...{ 'children': 'Title' }} />;
const aliased = { children: 'Title' };
<h1 {...aliased} />;
const contentBase = { children: 'Hi' };
<h1 {...{ ...contentBase }} />;
const children = 'x';
<h1 {...{ children }} />;
<h1 {...{ children: 0 }} />;
<h1 {...{ ...props, children: 'Title' }} />;
<h1 {...{ children: 'Title' }} {...{ className: 'x' }} />;
<h1 {...{ children: 'Title' }} {...'text'} />;
<h1 {...{ children: null }} {...{ children: 'Title' }} />;
<h1 {...{ ['children']: 'Title' }} />;
<h1 {...{ children: 'Title', ['title']: 'x' }} />;
<h1 {...({ children: 'Title' } as const)} />;
<h1 children="Title" />;
<h1 children={null} />;
<h1 aria-hidden="true" />;
<h1 {...someProps}>{/* comment */}</h1>;
<h1>Title</h1>;
<h2>{title}</h2>;
<h3><span>x</span></h3>;
<h1 />; // Noncompliant {{Headings must have content and the content must be accessible by a screen reader.}}
<h2></h2>; // Noncompliant
<h1 {...{}} />; // Noncompliant
const emptyBase = {};
const aliasOfEmpty = emptyBase;
<h1 {...aliasOfEmpty} />; // Noncompliant
<h1 {...[]} />; // Noncompliant
<h1 {...{ children: null }} />; // Noncompliant
<h2 {...{ children: '' }} />; // Noncompliant
<h4 {...{ children: false }} />; // Noncompliant
<h1 {...{ children: props.children }} />; // Noncompliant
<h1 {...{ get children() { return 'Title'; } }} />; // Noncompliant
<h1 {...{ children() { return 'Title'; } }} />; // Noncompliant
<h1 {...{ children: 'x' }} {...{ children: null }} />; // Noncompliant
<h1 {...{ ...props, children: null }} />; // Noncompliant
<h1 {...{ children: 'Title' }}><span aria-hidden="true">x</span></h1>; // Noncompliant
<h1 {...this.props} />; // Noncompliant
<h1 {...{ children: 'Title', ...props }} />; // Noncompliant
let reassigned = { children: 'x' };
reassigned = { children: 'y' };
<h1 {...reassigned} />; // Noncompliant
function Stripped(props) {
  const { children: _unused, ...rest } = props;
  return <h1 {...rest} />; // Noncompliant
}
const computedKey = 'children';
<h1 {...{ children: 'Title', [computedKey]: null }} />; // Noncompliant
<h1 {...{ [children]: 'Title' }} />; // Noncompliant
<MyHeading />;
"##,
        );
    }
}
