//! "setState" should use a callback when referencing the previous state
use crate::helpers::batch_react::*;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const MSG: &str = "Use callback in setState when referencing the previous state.";

fn is_set_state(c: &CallExpression<'_>) -> bool {
    matches!(&c.callee, Expression::StaticMemberExpression(m)
        if m.property.name == "setState" && matches!(m.object, Expression::ThisExpression(_)))
}

fn is_this_state(kind: &AstKind<'_>) -> bool {
    matches!(kind, AstKind::StaticMemberExpression(m)
        if m.property.name == "state" && matches!(m.object, Expression::ThisExpression(_)))
}

fn in_class_component(ctx: &Ctx<'_, '_>, id: NodeId) -> bool {
    parent_es6_component(ctx, id).is_some() || parent_es5_component(ctx, id).is_some()
}

/// Is `span` inside the first argument of the `setState` call `c`?
fn in_first_arg(c: &CallExpression<'_>, span: Span) -> bool {
    c.arguments.first().is_some_and(|a| {
        let s = a.span();
        s.start <= span.start && span.end <= s.end
    })
}

enum Stop {
    /// Reached the first argument of a `setState` call (its node id).
    SetState(NodeId),
    /// Stored in a variable declarator (its node id).
    Var(NodeId),
    Other,
}

/// The upstream walk from a `this.state` member up the tree.
fn walk_from_state(ctx: &Ctx<'_, '_>, id: NodeId) -> Stop {
    let span = ctx.node(id).kind().span();
    for a in ancestors(ctx, id) {
        match a.kind() {
            AstKind::CallExpression(c) if is_set_state(c) && in_first_arg(c, span) => {
                return Stop::SetState(a.id());
            }
            AstKind::MethodDefinition(_) => return Stop::Other,
            AstKind::Function(f) if f.is_expression() => {
                if matches!(
                    ctx.parent_kind(a.id()),
                    AstKind::ObjectProperty(_)
                        | AstKind::MethodDefinition(_)
                        | AstKind::PropertyDefinition(_)
                ) {
                    return Stop::Other;
                }
            }
            AstKind::VariableDeclarator(_) => return Stop::Var(a.id()),
            _ => {}
        }
    }
    Stop::Other
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::CallExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::CallExpression(call) = node.kind() else {
            return;
        };
        if !is_set_state(call) {
            return;
        }
        let Some(arg) = call.arguments.first() else {
            return;
        };
        let arg_span = arg.span();
        if !in_class_component(ctx, node.id()) {
            return;
        }
        let nodes = ctx.nodes();
        let mut reported: Vec<Span> = Vec::new();
        let mut report = |span: Span| {
            if !reported.contains(&span) {
                reported.push(span);
                report_react(ctx, span, MSG);
            }
        };
        let mut i = node.id().index() + 1;
        while i < nodes.len() {
            let n = nodes.get_node(NodeId::from_usize(i));
            i += 1;
            let kind = n.kind();
            let sp = kind.span();
            if sp.start >= arg_span.end {
                break;
            }
            if sp.start < arg_span.start {
                continue;
            }
            if is_this_state(&kind) {
                if matches!(walk_from_state(ctx, n.id()), Stop::SetState(c) if c == node.id()) {
                    report(sp);
                }
                continue;
            }
            let AstKind::IdentifierReference(ident) = kind else {
                continue;
            };
            // Identifier used as a property value or member object (possibly inside a binary expression).
            let mut cur = n;
            let mut parent = ctx.parent(cur.id());
            while matches!(parent.kind(), AstKind::BinaryExpression(_)) {
                cur = parent;
                parent = ctx.parent(cur.id());
            }
            let cur_span = cur.kind().span();
            let ok = match parent.kind() {
                AstKind::ObjectProperty(p) => p.value.span() == cur_span,
                AstKind::StaticMemberExpression(m) => m.object.span() == cur_span,
                AstKind::ComputedMemberExpression(m) => m.object.span() == cur_span,
                _ => false,
            };
            if !ok {
                continue;
            }
            // The nearest setState whose first argument contains the identifier must be this call.
            let owner = ancestors(ctx, n.id()).find(|a| matches!(a.kind(), AstKind::CallExpression(c) if is_set_state(c) && in_first_arg(c, sp)));
            if owner.map(|o| o.id()) != Some(node.id()) {
                continue;
            }
            let Some(sym) = ctx.symbol_of(ident) else {
                continue;
            };
            let decl = ctx.symbol_decl(sym);
            let AstKind::VariableDeclarator(v) = decl.kind() else {
                continue;
            };
            if v.span.start > sp.start {
                continue;
            }
            match &v.id {
                BindingPattern::BindingIdentifier(_) => {
                    // `this.state` nodes of the initializer stored into this variable, in the same scope.
                    let mut j = decl.id().index() + 1;
                    while j < nodes.len() {
                        let m = nodes.get_node(NodeId::from_usize(j));
                        j += 1;
                        let msp = m.kind().span();
                        if msp.start >= v.span.end {
                            break;
                        }
                        if is_this_state(&m.kind())
                            && m.scope_id() == n.scope_id()
                            && matches!(walk_from_state(ctx, m.id()), Stop::Var(d) if d == decl.id())
                            && in_class_component(ctx, m.id())
                        {
                            report(msp);
                        }
                    }
                }
                BindingPattern::ObjectPattern(p) => {
                    if ident.name == "state"
                        && matches!(
                            v.init.as_ref().map(strip_parens),
                            Some(Expression::ThisExpression(_))
                        )
                        && decl.scope_id() == n.scope_id()
                        && let Some(prop) = p
                            .properties
                            .iter()
                            .find(|bp| property_key_name(&bp.key) == Some("state"))
                    {
                        report(prop.key.span());
                    }
                }
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn access_state() {
        check(
            "S6756",
            "a.jsx",
            r#"
class Foo extends React.Component {
  inc() {
    this.setState({ value: this.state.value + 1 }); // Noncompliant {{Use callback in setState when referencing the previous state.}}
    this.setState(prev => ({ value: prev.value + 1 }));
    const value = this.state.value; // Noncompliant
    this.setState({ value: value + 1 });
    const { state } = this; // Noncompliant
    this.setState({ value: state.value + 1 });
    const other = this.state.other;
    this.setState({ value: 1 });
    this.setState({ value: compute(other) });
  }
  render() { return null; }
}
class NotComponent {
  m() { this.setState({ value: this.state.value + 1 }); }
}
"#,
        );
    }
}
