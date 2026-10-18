//! Fields that are only assigned in the constructor should be "readonly"
use crate::helpers::batch_ts::{MemberWrite, written_members};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// Start of a member's "head": its first token after the decorators.
fn head_start(ctx: &Ctx<'_, '_>, start: u32, decorators: &[Decorator<'_>]) -> u32 {
    match decorators.last() {
        Some(d) => token_after(ctx, d.span.end).map_or(start, |t| t.start),
        None => start,
    }
}

/// True if `id` is directly inside a class constructor (no nested function in between).
fn directly_in_constructor(ctx: &Ctx<'_, '_>, id: NodeId) -> bool {
    for a in ctx.ancestors(id) {
        match a.kind() {
            AstKind::Function(_) => {
                return matches!(ctx.parent_kind(a.id()),
                    AstKind::MethodDefinition(m) if m.kind == MethodDefinitionKind::Constructor);
            }
            AstKind::ArrowFunctionExpression(_)
            | AstKind::StaticBlock(_)
            | AstKind::PropertyDefinition(_) => {
                return false;
            }
            _ => {}
        }
    }
    false
}

/// Names of members that are written anywhere except directly through `this` in a constructor.
fn modified_names<'a>(ctx: &Ctx<'a, '_>) -> FxHashSet<std::borrow::Cow<'a, str>> {
    let mut out = FxHashSet::default();
    let mut writes: Vec<MemberWrite<'a, 'a>> = Vec::new();
    for node in ctx.nodes().iter() {
        if !matches!(
            node.kind(),
            AstKind::AssignmentExpression(_)
                | AstKind::UpdateExpression(_)
                | AstKind::UnaryExpression(_)
                | AstKind::ForInStatement(_)
                | AstKind::ForOfStatement(_)
        ) {
            continue;
        }
        writes.clear();
        written_members(node.kind(), &mut writes);
        // Upstream also treats `this.x[key] = v` as a modification of `x`.
        if let AstKind::AssignmentExpression(a) = node.kind()
            && a.operator == AssignmentOperator::Assign
            && let AssignmentTarget::ComputedMemberExpression(c) = &a.left
        {
            crate::helpers::batch_ts::expression_member(&c.object, &mut writes);
        }
        for w in writes.drain(..) {
            if matches!(strip_ts(w.object), Expression::ThisExpression(_))
                && directly_in_constructor(ctx, node.id())
            {
                continue;
            }
            out.insert(w.name);
        }
    }
    out
}

impl JsRule for Rule {
    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {
        if crate::helpers::batch_ts::no_type_info(ctx) {
            return;
        }
        if !ctx.nodes().contains(AstType::Class) {
            return;
        }
        let mut modified = None;
        for node in ctx.nodes().iter() {
            let AstKind::Class(class) = node.kind() else {
                continue;
            };
            // (secondary span, displayed name, lookup name)
            let mut candidates: Vec<(Span, String, String)> = Vec::new();
            for el in &class.body.body {
                match el {
                    ClassElement::PropertyDefinition(p) => {
                        if p.readonly
                            || p.declare
                            || p.computed
                            || p.r#type == PropertyDefinitionType::TSAbstractPropertyDefinition
                        {
                            continue;
                        }
                        let name = match &p.key {
                            PropertyKey::PrivateIdentifier(id) => format!("#{}", id.name),
                            PropertyKey::StaticIdentifier(id)
                                if p.accessibility == Some(TSAccessibility::Private) =>
                            {
                                id.name.to_string()
                            }
                            _ => continue,
                        };
                        let start = head_start(ctx, p.span.start, &p.decorators);
                        let head = Span::new(start, p.key.span().end.max(start));
                        candidates.push((head, ctx.text(p.key.span()).to_string(), name));
                    }
                    ClassElement::MethodDefinition(m)
                        if m.kind == MethodDefinitionKind::Constructor =>
                    {
                        for param in &m.value.params.items {
                            if param.accessibility != Some(TSAccessibility::Private)
                                || param.readonly
                            {
                                continue;
                            }
                            let BindingPattern::BindingIdentifier(id) = &param.pattern else {
                                continue;
                            };
                            // ESTree extends the parameter identifier over its type annotation.
                            let end = param
                                .type_annotation
                                .as_ref()
                                .map_or(id.span.end, |t| t.span.end);
                            let start = head_start(ctx, param.span.start, &param.decorators);
                            let head = Span::new(start, id.span.end.max(start));
                            candidates.push((
                                head,
                                ctx.text(Span::new(id.span.start, end)).to_string(),
                                id.name.to_string(),
                            ));
                        }
                    }
                    _ => {}
                }
            }
            if candidates.is_empty() {
                continue;
            }
            let modified = modified.get_or_insert_with(|| modified_names(ctx));
            let mut violations: Vec<(Span, String)> = candidates
                .into_iter()
                .filter(|(_, _, name)| !modified.contains(name.as_str()))
                .map(|(span, shown, _)| (span, shown))
                .collect();
            if violations.is_empty() {
                continue;
            }
            violations.sort_by_key(|(s, _)| s.start);
            let primary = match &class.id {
                Some(id) => id.span,
                None => find_token(ctx, class.span, Kind::Class).unwrap_or(class.span),
            };
            let msg = if violations.len() == 1 {
                "Mark this member as `readonly`."
            } else {
                "Mark these members as `readonly`."
            };
            ctx.report_with(
                primary,
                msg,
                violations.into_iter().map(|(span, name)| {
                    (
                        span,
                        Some(format!(
                            "Member '{name}' is never reassigned; mark it as `readonly`."
                        )),
                    )
                }),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn readonly() {
        check(
            "S2933",
            "a.ts",
            r#"
class Person { // Noncompliant {{Mark these members as `readonly`.}}
  private _birthYear: number;
  private name = 'x';
  #secret = 1;
  private age: number;
  readonly ok: number;
  public pub = 1;
  constructor(birthYear: number, private other: string, private mutated: string) {
    this._birthYear = birthYear;
    this.age = 1;
    this.mutated = 'a';
  }
  birthday() {
    this.age++;
    this.#secret = 2;
    [this.mutated] = ['b'];
  }
}
class Single { // Noncompliant {{Mark this member as `readonly`.}}
  private w = 1;
}
class Fine {
  private x = 1;
  private y = 2;
  constructor() {
    setTimeout(() => { this.x = 2; });
  }
  m() { delete this.y; }
}
const Anon = class { // Noncompliant
  private z = 1;
};
class Decorated { // Noncompliant
  @Input() private d = 1;
}
"#,
        );
    }
}
