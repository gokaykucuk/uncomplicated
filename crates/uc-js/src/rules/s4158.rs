//! Empty collections should not be accessed or iterated
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const READING_METHODS: [&str; 32] = [
    "copyWithin",
    "pop",
    "reverse",
    "shift",
    "sort",
    "clear",
    "delete",
    "concat",
    "flat",
    "flatMap",
    "includes",
    "indexOf",
    "join",
    "lastIndexOf",
    "slice",
    "toSource",
    "toString",
    "toLocaleString",
    "get",
    "has",
    "entries",
    "every",
    "filter",
    "find",
    "findIndex",
    "forEach",
    "keys",
    "map",
    "reduce",
    "reduceRight",
    "some",
    "values",
];

fn is_empty_collection(e: &Expression<'_>) -> bool {
    let (callee, args) = match strip_parens(e) {
        Expression::ArrayExpression(a) => return a.elements.is_empty(),
        Expression::CallExpression(c) => (&c.callee, &c.arguments),
        Expression::NewExpression(n) => (&n.callee, &n.arguments),
        _ => return false,
    };
    matches!(callee, Expression::Identifier(id) if matches!(id.name.as_str(), "Array" | "Map" | "Set" | "WeakSet" | "WeakMap"))
        && args.is_empty()
}

/// Parent of a node, skipping only parentheses.
fn parent_no_parens<'s, 'a>(ctx: &Ctx<'a, 's>, mut id: NodeId) -> &'s AstNode<'a> {
    loop {
        let p = ctx.parent(id);
        if matches!(p.kind(), AstKind::ParenthesizedExpression(_)) && p.id() != NodeId::ROOT {
            id = p.id();
        } else {
            return p;
        }
    }
}

enum Usage {
    Reading,
    EmptyAssignment,
    Other,
}

fn classify(
    ctx: &Ctx<'_, '_>,
    node_id: NodeId,
    span: Span,
    is_write: bool,
    is_read: bool,
) -> Usage {
    if is_write && !is_read {
        // Nearest declarator or expression statement.
        for kind in ctx.nodes().ancestor_kinds(node_id) {
            match kind {
                AstKind::ExpressionStatement(es) => {
                    return match strip_parens(&es.expression) {
                        Expression::AssignmentExpression(a)
                            if a.left.span() == span && is_empty_collection(&a.right) =>
                        {
                            Usage::EmptyAssignment
                        }
                        _ => Usage::Other,
                    };
                }
                AstKind::VariableDeclarator(_) => return Usage::Other,
                _ => {}
            }
        }
        return Usage::Other;
    }
    let parent = parent_no_parens(ctx, node_id);
    match parent.kind() {
        AstKind::StaticMemberExpression(m) => {
            if READING_METHODS.contains(&m.property.name.as_str())
                && matches!(
                    parent_no_parens(ctx, parent.id()).kind(),
                    AstKind::CallExpression(_)
                )
            {
                return Usage::Reading;
            }
        }
        AstKind::ComputedMemberExpression(m) if !is_element_write(ctx, parent.id(), m.span) => {
            return Usage::Reading;
        }
        _ => {}
    }
    // `for (x of coll)` / `for (k in coll)`.
    for kind in ctx.nodes().ancestor_kinds(node_id) {
        match kind {
            AstKind::ForOfStatement(f) => {
                return if f.right.span() == span {
                    Usage::Reading
                } else {
                    Usage::Other
                };
            }
            AstKind::ForInStatement(f) => {
                return if f.right.span() == span {
                    Usage::Reading
                } else {
                    Usage::Other
                };
            }
            _ => {}
        }
    }
    Usage::Other
}

fn is_element_write(ctx: &Ctx<'_, '_>, member_id: NodeId, member: Span) -> bool {
    for kind in ctx.nodes().ancestor_kinds(member_id) {
        if let AstKind::AssignmentExpression(a) = kind {
            return matches!(
                a.operator,
                AssignmentOperator::Assign
                    | AssignmentOperator::LogicalOr
                    | AssignmentOperator::LogicalAnd
                    | AssignmentOperator::LogicalNullish
            ) && a.left.span().contains_inclusive(member);
        }
    }
    false
}

impl JsRule for Rule {
    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {
        let scoping = ctx.scoping();
        // Sonar parses files as modules first, so top-level variables live in a module scope
        // (not the global scope it skips) for nearly all files.
        for sym in scoping.symbol_ids() {
            let flags = scoping.symbol_flags(sym);
            if !flags.is_variable() {
                continue;
            }
            let decl = ctx.symbol_decl(sym);
            let AstKind::VariableDeclarator(v) = decl.kind() else {
                continue;
            };
            let mut empty_assigned = false;
            let mut writes = 0usize;
            if let Some(init) = &v.init {
                if !matches!(v.id, BindingPattern::BindingIdentifier(_))
                    || !is_empty_collection(init)
                {
                    continue;
                }
                empty_assigned = true;
                writes = 1;
            } else if matches!(
                ctx.nodes().parent_kind(decl.id()),
                AstKind::VariableDeclaration(_)
            ) && matches!(
                ctx.nodes().ancestor_kinds(decl.id()).nth(1),
                Some(AstKind::ForInStatement(_) | AstKind::ForOfStatement(_))
            ) {
                continue;
            }
            let refs: Vec<_> = scoping
                .get_resolved_references(sym)
                .filter(|r| r.is_value())
                .collect();
            if refs.len() + writes <= 1 {
                continue;
            }
            let mut readings: Vec<Span> = Vec::new();
            let mut bail = false;
            for r in refs {
                let id = r.node_id();
                let span = ctx.node(id).span();
                match classify(ctx, id, span, r.is_write(), r.is_read()) {
                    Usage::Reading => readings.push(span),
                    Usage::EmptyAssignment => empty_assigned = true,
                    Usage::Other => {
                        bail = true;
                        break;
                    }
                }
            }
            if bail || !empty_assigned {
                continue;
            }
            let name = scoping.symbol_name(sym);
            for s in readings {
                ctx.report(
                    s,
                    format!("Review this usage of \"{name}\" as it can only be empty here."),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn empty_collection() {
        check(
            "S4158",
            "a.ts",
            r#"
function ko() {
  const strings: string[] = [];
  strings.forEach(s => foo(s)); // Noncompliant {{Review this usage of "strings" as it can only be empty here.}}
  foo(strings[0]); // Noncompliant
  for (const s of strings) {} // Noncompliant
  const m = new Map();
  m.get("x"); // Noncompliant
  let later;
  later = [];
  later.map(x => x); // Noncompliant
}
function okForNotEmptyInit() {
  const nonEmptyArray = [1, 2, 3];
  foo(nonEmptyArray[2]);
  nonEmptyArray.forEach(item => console.log());
}
function okLatelyWritten() {
  const okLatelyWritten: number[] = [];
  okLatelyWritten.push(1);
  okLatelyWritten.forEach(item => console.log());
}
function ctor() {
  const a = new Array(1, 2, 3);
  a.forEach(item => console.log());
}
function params(parameterArray: number[]) {
  foo(parameterArray[1]);
}
function asArgument() {
  const array: number[] = [];
  foo(array);
  array.forEach(x => x);
}
function reassigned() {
  let overwritten = [];
  overwritten = other;
  overwritten.forEach(x => x);
}
function elementWrite() {
  const arr: number[] = [];
  arr[1] = 42;
  foo(arr[1]);
}
function destructuringWrite() {
  const arr: number[] = [];
  [, arr[1]] = [42, 42];
  foo(arr[1]);
}
function ternary(flag: boolean) {
  const a1: number[] = [];
  const a2: number[] = [];
  (flag ? a1 : a2).push(1);
  a1.forEach(x => x);
}
"#,
        );
    }
}
