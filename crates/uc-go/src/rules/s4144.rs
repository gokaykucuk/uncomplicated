//! Functions should not have identical implementations.
use crate::helpers_sonar::{body_items, is_semicolon, same_tokens_between, tokens_hash};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const MIN_STATEMENTS: usize = 2;

/// Wrapper nodes tree-sitter has and sonar-go's tree doesn't: functions are grouped by their
/// sonar-go parent.
fn is_wrapper(kind: &str) -> bool {
    matches!(
        kind,
        "expression_list" | "argument_list" | "literal_element" | "literal_value"
    )
}

type Range = Option<(usize, usize)>;

struct Func<'t> {
    node: Nd<'t>,
    parts: [Range; 4],
    hash: u64,
}

fn range(n: Option<Nd<'_>>) -> Range {
    n.map(|n| (n.start_byte(), n.end_byte()))
}

/// The parameter list, when it declares named parameters (unnamed ones are not parameters for
/// sonar-go).
fn named_params(n: Nd<'_>) -> Range {
    let list = n.child_by_field_name("parameters")?;
    let mut c = list.walk();
    let named = list
        .named_children(&mut c)
        .any(|p| p.child_by_field_name("name").is_some());
    named.then(|| (list.start_byte(), list.end_byte()))
}

fn equivalent(ctx: &Ctx<'_>, a: &[Range; 4], b: &[Range; 4]) -> bool {
    a.iter().zip(b).all(|(x, y)| match (x, y) {
        (None, None) => true,
        (Some(x), Some(y)) => same_tokens_between(ctx, *x, *y),
        _ => false,
    })
}

fn has_minimum_size(f: Nd<'_>) -> bool {
    f.child_by_field_name("body").is_some_and(|b| {
        body_items(b)
            .into_iter()
            .filter(|s| !is_semicolon(*s))
            .count()
            >= MIN_STATEMENTS
    })
}

impl GoRule for Rule {
    fn run_once(&self, ctx: &Ctx<'_>) {
        // Functions grouped by parent, then by a hash of their implementation.
        let mut groups: FxHashMap<(usize, u64), Vec<Func<'_>>> = FxHashMap::default();
        let mut order: Vec<(usize, u64)> = Vec::new();
        for &n in &ctx.nodes {
            if !helpers::is_function(n) || n.child_by_field_name("body").is_none() {
                continue;
            }
            let parent = ctx
                .ancestors(n)
                .find(|a| !is_wrapper(a.kind()))
                .map_or(usize::MAX, |p| p.id());
            let parts = [
                range(n.child_by_field_name("receiver")),
                named_params(n),
                range(n.child_by_field_name("type_parameters")),
                range(n.child_by_field_name("body")),
            ];
            let mut hash = 0u64;
            for (i, p) in parts.iter().enumerate() {
                let h = p.map_or(i as u64, |(s, e)| tokens_hash(ctx, s, e));
                hash = hash.rotate_left(17) ^ h;
            }
            let key = (parent, hash);
            let g = groups.entry(key).or_default();
            if g.is_empty() {
                order.push(key);
            }
            g.push(Func {
                node: n,
                parts,
                hash,
            });
        }
        for key in order {
            let funcs = &groups[&key];
            if funcs.len() < 2 {
                continue;
            }
            let mut reported = vec![false; funcs.len()];
            for i in 0..funcs.len() {
                for j in i + 1..funcs.len() {
                    if reported[j]
                        || funcs[i].hash != funcs[j].hash
                        || !has_minimum_size(funcs[j].node)
                        || !equivalent(ctx, &funcs[i].parts, &funcs[j].parts)
                    {
                        continue;
                    }
                    reported[j] = true;
                    report(ctx, funcs[i].node, funcs[j].node);
                }
                if reported.iter().skip(i + 1).all(|r| *r) {
                    break;
                }
            }
        }
    }
}

fn report(ctx: &Ctx<'_>, original: Nd<'_>, dup: Nd<'_>) {
    let line = ctx.line(original);
    let (msg, secondary) = match original.child_by_field_name("name") {
        Some(name) => (
            format!(
                "Update this function so that its implementation is not identical to \"{}\" on line {line}.",
                ctx.text(name)
            ),
            span(name),
        ),
        None => (
            format!(
                "Update this function so that its implementation is not identical to the one on line {line}."
            ),
            span(original),
        ),
    };
    let primary = dup
        .child_by_field_name("name")
        .map_or_else(|| span(dup), span);
    ctx.report_issue(
        primary,
        msg,
        vec![(secondary, Some("original implementation".to_string()))],
        None,
    );
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn duplicated_functions() {
        check(
            "S4144",
            r#"package p

func original(x int) {
	a(x)
	b(x)
}

func copy1(x int) { // Noncompliant {{Update this function so that its implementation is not identical to "original" on line 3.}}
	a(x)
	b(x)
}

func copy2(x int) { // Noncompliant {{Update this function so that its implementation is not identical to "original" on line 3.}}
	a(x)
	b(x)
}

func otherParams(y int) {
	a(x)
	b(x)
}

func (r T) method(x int) {
	a(x)
	b(x)
}

func short1() {
	a()
}

func short2() {
	a()
}

func g() {
	f1 := func() {
		a()
		b()
	}
	f2 := func() {
		a()
		b()
	}
	call(func() {
		a()
		b()
	}, func() { // Noncompliant {{Update this function so that its implementation is not identical to the one on line 45.}}
		a()
		b()
	})
}
"#,
        );
    }
}
