//! Functions should not have identical implementations
use xxhash_rust::xxh3::Xxh3;

use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const MIN_LINES: u32 = 3;

struct Candidate<'s, 'a> {
    node: &'s AstNode<'a>,
    body: Span,
    hash: u64,
}

fn parent_skip_parens<'s, 'a>(ctx: &Ctx<'a, 's>, id: NodeId) -> AstKind<'a> {
    let mut p = ctx.parent(id);
    while let AstKind::ParenthesizedExpression(_) = p.kind() {
        p = ctx.parent(p.id());
    }
    p.kind()
}

/// Body span of a function Sonar considers (declarations, and function / arrow expressions
/// directly assigned to a variable or used as a method).
fn body_span(ctx: &Ctx<'_, '_>, node: &AstNode<'_>) -> Option<Span> {
    match node.kind() {
        AstKind::Function(f) => {
            let body = f.body.as_ref()?;
            let ok = f.is_declaration()
                || matches!(
                    parent_skip_parens(ctx, node.id()),
                    AstKind::VariableDeclarator(_) | AstKind::MethodDefinition(_)
                );
            ok.then_some(body.span)
        }
        AstKind::ArrowFunctionExpression(a) => {
            if !matches!(
                parent_skip_parens(ctx, node.id()),
                AstKind::VariableDeclarator(_)
            ) {
                return None;
            }
            Some(match &a.body {
                ArrowFunctionBody::FunctionBody(b) => b.span,
                body => match body.as_expression() {
                    Some(e) => strip_parens(e).span(),
                    None => body.span(),
                },
            })
        }
        _ => None,
    }
}

fn big_enough(ctx: &Ctx<'_, '_>, body: Span) -> bool {
    let mut tokens = ctx.tokens_in(body);
    if tokens.first().is_some_and(|t| ctx.text(t.span()) == "{") {
        tokens = &tokens[1..];
    }
    if tokens.last().is_some_and(|t| ctx.text(t.span()) == "}") {
        tokens = &tokens[..tokens.len() - 1];
    }
    match (tokens.first(), tokens.last()) {
        (Some(f), Some(l)) => ctx.line(l.end()) - ctx.line(f.start()) + 1 >= MIN_LINES,
        _ => false,
    }
}

fn token_hash(ctx: &Ctx<'_, '_>, body: Span) -> u64 {
    let mut h = Xxh3::new();
    for t in ctx.tokens_in(body) {
        h.update(ctx.text(t.span()).as_bytes());
        h.update(&[0]);
    }
    h.digest()
}

impl JsRule for Rule {
    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {
        let mut candidates: Vec<Candidate<'_, 'a>> = Vec::new();
        for node in ctx.nodes().iter() {
            if !matches!(
                node.kind(),
                AstKind::Function(_) | AstKind::ArrowFunctionExpression(_)
            ) {
                continue;
            }
            let Some(body) = body_span(ctx, node) else {
                continue;
            };
            if big_enough(ctx, body) {
                candidates.push(Candidate {
                    node,
                    body,
                    hash: token_hash(ctx, body),
                });
            }
        }
        let mut by_hash: FxHashMap<u64, Vec<usize>> = FxHashMap::default();
        for (i, c) in candidates.iter().enumerate() {
            let group = by_hash.entry(c.hash).or_default();
            if let Some(&j) = group
                .iter()
                .find(|&&j| ctx.same_tokens(candidates[j].body, c.body))
            {
                let original = &candidates[j];
                let line = ctx.line(original.node.kind().span().start);
                ctx.report_with(
                    function_report_span(ctx, c.node),
                    format!("Update this function so that its implementation is not identical to the one on line {line}."),
                    [(function_report_span(ctx, original.node), Some("Original implementation".to_string()))],
                );
            }
            group.push(i);
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn identical_functions() {
        check(
            "S4144",
            "a.js",
            r#"
function calculateCode() {
  doTheThing();
  doOtherThing();
  return code;
}
function getName() {      // Noncompliant {{Update this function so that its implementation is not identical to the one on line 2.}}
  doTheThing();
  doOtherThing();
  return code;
}
function small() {
  return code;
}
function small2() {
  return code;
}
const arrow = () => {     // Noncompliant
  doTheThing();
  doOtherThing();
  return code;
};
class A {
  m() {                   // Noncompliant
    doTheThing();
    doOtherThing();
    return code;
  }
}
function different() {
  doTheThing();
  doOtherThing();
  return other;
}
foo(function () {
  doTheThing();
  doOtherThing();
  return code;
});
"#,
        );
    }
}
