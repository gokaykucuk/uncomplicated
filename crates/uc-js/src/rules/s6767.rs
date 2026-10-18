//! Unused React typed props should be removed
//!
//! Only components whose props are fully tracked (no spreads, rest elements or props passed
//! around) and declared by a propTypes object literal or a local TypeScript type are checked.
use crate::helpers::batch_react::*;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// Props whose own shape upstream skips (`PropTypes.shape`, `exact`, nested type literals, `never`).
fn skipped_prop_type(ctx: &Ctx<'_, '_>, span: Span) -> bool {
    let text = ctx.text(span);
    text.contains("PropTypes.shape") || text.contains("PropTypes.exact")
}

fn is_ts_skipped(t: &TSSignature<'_>) -> bool {
    let ann = match t {
        TSSignature::TSPropertySignature(p) => p.type_annotation.as_ref(),
        _ => None,
    };
    ann.is_some_and(|a| {
        matches!(
            a.type_annotation,
            TSType::TSTypeLiteral(_) | TSType::TSNeverKeyword(_)
        )
    })
}

/// The TS type is referenced only by this component (a shared props type may be used elsewhere).
/// The TS type is only referenced from within this component (a props type shared by several
/// components may be used by the others).
fn single_use(ctx: &Ctx<'_, '_>, t: &TSType<'_>, comp: &ComponentInfo<'_>) -> bool {
    let TSType::TSTypeReference(r) = t else {
        return true;
    };
    let TSTypeName::IdentifierReference(id) = &r.type_name else {
        return false;
    };
    let Some(sym) = ctx.scoping().get_reference(id.reference_id()).symbol_id() else {
        return false;
    };
    // The component's own extent, including `const Foo: FC<Props> = ...`.
    let mut extent = ctx.node(comp.node).kind().span();
    if let AstKind::VariableDeclarator(v) = estree_parent(ctx, comp.node).kind() {
        extent = v.span;
    }
    ctx.scoping().get_resolved_references(sym).all(|r| {
        let sp = ctx.node(r.node_id()).kind().span();
        extent.start <= sp.start && sp.end <= extent.end
    })
}

fn ts_members<'a>(ctx: &Ctx<'a, '_>, t: &'a TSType<'a>) -> Option<&'a [TSSignature<'a>]> {
    match t {
        TSType::TSTypeLiteral(l) => Some(&l.members),
        TSType::TSTypeReference(r) => {
            let TSTypeName::IdentifierReference(id) = &r.type_name else {
                return None;
            };
            let sym = ctx.scoping().get_reference(id.reference_id()).symbol_id()?;
            match ctx.symbol_decl(sym).kind() {
                AstKind::TSInterfaceDeclaration(i) => Some(&i.body.body),
                AstKind::TSTypeAliasDeclaration(a) => match &a.type_annotation {
                    TSType::TSTypeLiteral(l) => Some(&l.members),
                    _ => None,
                },
                _ => None,
            }
        }
        _ => None,
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[
            AstType::Function,
            AstType::ArrowFunctionExpression,
            AstType::Class,
        ]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let Some(comp) = component_info(ctx, node) else {
            return;
        };
        if comp
            .class
            .is_some_and(|c| c.r#abstract || !c.decorators.is_empty())
        {
            return;
        }
        // Function components without a props parameter.
        if let Some(f) = comp.func {
            let has_param = match ctx.node(f).kind() {
                AstKind::Function(f) => !f.params.items.is_empty(),
                AstKind::ArrowFunctionExpression(a) => !a.params.items.is_empty(),
                _ => false,
            };
            if !has_param {
                return;
            }
        }
        let (declared, skip): (Vec<DeclaredProp<'a>>, Vec<Span>) =
            match component_static(ctx, &comp, "propTypes") {
                Some(None) => return,
                Some(Some(o)) => {
                    let Some(keys) = prop_types_keys(o) else {
                        return;
                    };
                    let skip = keys
                        .iter()
                        .filter(|k| skipped_prop_type(ctx, k.span))
                        .map(|k| k.span)
                        .collect();
                    (keys, skip)
                }
                None => {
                    let Some(t) = ts_props_type(ctx, &comp) else {
                        return;
                    };
                    if !single_use(ctx, t, &comp) {
                        return;
                    }
                    let Some(keys) = ts_props(ctx, t, 0) else {
                        return;
                    };
                    let skip = ts_members(ctx, t)
                        .map(|ms| {
                            ms.iter()
                                .filter(|m| is_ts_skipped(m))
                                .map(|m| m.span())
                                .collect()
                        })
                        .unwrap_or_default();
                    (keys, skip)
                }
            };
        if declared.is_empty() {
            return;
        }
        let usages = prop_usages(ctx, &comp);
        if usages.escaped {
            return;
        }
        for d in &declared {
            if skip.contains(&d.span) {
                continue;
            }
            let used = usages
                .list
                .iter()
                .any(|u| u.names.last().is_some_and(|n| n == d.name));
            if !used {
                report_react(
                    ctx,
                    d.key_span,
                    format!("'{}' PropType is defined but prop is never used", d.name),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn unused_props_ts() {
        check(
            "S6767",
            "a.tsx",
            r#"
import React from "react";
interface Props {
  used: string;
  unused: number; // Noncompliant {{'unused' PropType is defined but prop is never used}}
}
function Foo({ used }: Props) {
  return <div>{used}</div>;
}
type BarProps = { a: string; b?: string; nested: { x: number } };
const Bar = (props: BarProps) => <div>{props.a}</div>; // Noncompliant@-1
interface Spread { a: string; b: string; }
function Baz(props: Spread) { return <Child {...props} />; }
interface Rest { a: string; b: string; }
function Qux({ a, ...rest }: Rest) { return <div>{a}</div>; }
interface Shared { a: string; b: string; }
function S1({ a }: Shared) { return <div>{a}</div>; }
function S2({ b }: Shared) { return <div>{b}</div>; }
"#,
        );
    }

    #[test]
    fn unused_prop_types_js() {
        check(
            "S6767",
            "a.jsx",
            r#"
import React from "react";
function Hello(props) {
  return <div>{props.name}</div>;
}
Hello.propTypes = {
  name: PropTypes.string,
  age: PropTypes.number, // Noncompliant {{'age' PropType is defined but prop is never used}}
  shape: PropTypes.shape({ a: PropTypes.string }),
};
class K extends React.Component {
  static propTypes = { title: PropTypes.string, other: PropTypes.string }; // Noncompliant
  render() { return <div>{this.props.title}</div>; }
}
"#,
        );
    }
}
