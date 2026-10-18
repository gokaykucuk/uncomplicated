//! Names of regular expressions named groups should be used
use oxc_regular_expression::ast::Term;

use crate::helpers::batch_regex::{
    GroupRef, Node, Regex, const_init, for_each_regex, group_references, is_string_replace_call,
    lhs_symbol, regexp_callee, walk,
};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

struct Group {
    name: String,
    loc: Span,
    index: usize,
    used: bool,
}

struct Knowledge {
    node: Span,
    groups: Vec<Group>,
    /// Indexed backreferences (`\1`) to named groups: (location, group position in `groups`).
    indexed_refs: Vec<(Span, usize)>,
    matched: bool,
}

fn make_knowledge(re: &Regex<'_, '_>, node: Span) -> Knowledge {
    {
        let mut groups = Vec::new();
        let mut count = 0;
        let mut indexed = Vec::new();
        let mut named = Vec::new();
        walk(re.pattern, &mut |n, enter| {
            if !enter {
                return;
            }
            match n {
                Node::Term(Term::CapturingGroup(g)) => {
                    count += 1;
                    if let Some(name) = &g.name {
                        groups.push(Group {
                            name: name.to_string(),
                            loc: re.loc_or_node(g.span),
                            index: count,
                            used: false,
                        });
                    }
                }
                Node::Term(Term::IndexedReference(r)) => {
                    indexed.push((r.index as usize, re.loc_or_node(r.span)))
                }
                Node::Term(Term::NamedReference(r)) => named.push(r.name.to_string()),
                _ => {}
            }
        });
        let mut indexed_refs = Vec::new();
        for (index, loc) in indexed {
            if let Some(pos) = groups.iter().position(|g| g.index == index) {
                groups[pos].used = true;
                indexed_refs.push((loc, pos));
            }
        }
        for g in &mut groups {
            if named.contains(&g.name) {
                g.used = true;
            }
        }
        Knowledge {
            node,
            groups,
            indexed_refs,
            matched: false,
        }
    }
}

fn secondaries(
    k: &Knowledge,
    groups: impl Iterator<Item = usize>,
    label: &str,
) -> Vec<(Span, Option<String>)> {
    groups
        .map(|i| {
            (
                k.groups[i].loc,
                Some(format!("{label} '{}'", k.groups[i].name)),
            )
        })
        .collect()
}

struct State {
    knowledge: Vec<Knowledge>,
    by_node: FxHashMap<Span, usize>,
    bindings: FxHashMap<SymbolId, usize>,
}

impl State {
    fn find<'a>(&self, ctx: &Ctx<'a, '_>, e: &'a Expression<'a>, depth: u8) -> Option<usize> {
        let e = strip_parens(e);
        if depth < 5
            && let Expression::Identifier(id) = e
            && let Some(i) = const_init(ctx, id).and_then(|init| self.find(ctx, init, depth + 1))
        {
            return Some(i);
        }
        self.by_node.get(&e.span()).copied()
    }

    fn resolve(&self, ctx: &Ctx<'_, '_>, e: &Expression<'_>) -> Option<usize> {
        let Expression::Identifier(id) = strip_parens(e) else {
            return None;
        };
        self.bindings.get(&ctx.symbol_of(id)?).copied()
    }
}

/// Name nodes of `<matcher>.groups.<name>`, `<matcher>.groups['name']` or `{ name } = <matcher>.groups`,
/// given the parent of the `.groups` member.
fn group_name_nodes<'a>(parent: &AstNode<'a>, out: &mut Vec<(&'a str, Span)>) {
    match parent.kind() {
        AstKind::StaticMemberExpression(m) => out.push((m.property.name.as_str(), m.property.span)),
        AstKind::ComputedMemberExpression(m) => {
            if let Expression::StringLiteral(s) = &m.expression {
                out.push((s.value.as_str(), s.span));
            }
        }
        AstKind::VariableDeclarator(v) => {
            if let BindingPattern::ObjectPattern(p) = &v.id {
                for prop in &p.properties {
                    if let Some(k) = key_ident(&prop.key) {
                        out.push(k);
                    }
                }
            }
        }
        AstKind::AssignmentExpression(a) => {
            if let AssignmentTarget::ObjectAssignmentTarget(p) = &a.left {
                for prop in &p.properties {
                    match prop {
                        AssignmentTargetProperty::AssignmentTargetPropertyIdentifier(i) => {
                            out.push((i.binding.name.as_str(), i.binding.span));
                        }
                        AssignmentTargetProperty::AssignmentTargetPropertyProperty(p) => {
                            if let Some(k) = key_ident(&p.name) {
                                out.push(k);
                            }
                        }
                    }
                }
            }
        }
        _ => {}
    }
}

fn key_ident<'a>(k: &'a PropertyKey<'a>) -> Option<(&'a str, Span)> {
    match k {
        PropertyKey::StaticIdentifier(i) => Some((i.name.as_str(), i.span)),
        PropertyKey::Identifier(i) => Some((i.name.as_str(), i.span)),
        _ => None,
    }
}

fn parent_skip_non_null<'s, 'a>(ctx: &Ctx<'a, 's>, id: NodeId) -> &'s AstNode<'a> {
    let mut p = ctx.parent(id);
    while matches!(p.kind(), AstKind::TSNonNullExpression(_)) && p.id() != NodeId::ROOT {
        p = ctx.parent(p.id());
    }
    p
}

/// Numeric value of an index expression (literal or constant).
fn index_value<'a>(ctx: &Ctx<'a, '_>, e: &'a Expression<'a>) -> Option<f64> {
    match strip_parens(e) {
        Expression::NumericLiteral(n) => Some(n.value),
        Expression::Identifier(id) => match const_init(ctx, id).map(strip_parens) {
            Some(Expression::NumericLiteral(n)) => Some(n.value),
            _ => None,
        },
        _ => None,
    }
}

impl JsRule for Rule {
    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {
        let mut st = State {
            knowledge: Vec::new(),
            by_node: FxHashMap::default(),
            bindings: FxHashMap::default(),
        };
        // Pass 1: every regex of the file.
        for node in ctx.nodes().iter() {
            // The node SonarJS attaches the regex to: the literal, the constructor call, or the
            // implicit regex argument of `str.match(...)`.
            let span = match node.kind() {
                AstKind::RegExpLiteral(l) => l.span,
                AstKind::NewExpression(n) => n.span,
                AstKind::CallExpression(c) if regexp_callee(ctx, &c.callee, false) => c.span,
                AstKind::CallExpression(c) => {
                    match c.arguments.first().and_then(Argument::as_expression) {
                        Some(arg) => strip_parens(arg).span(),
                        None => continue,
                    }
                }
                _ => continue,
            };
            if st.by_node.contains_key(&span) {
                continue;
            }
            for_each_regex(node, ctx, |re| {
                st.by_node.insert(span, st.knowledge.len());
                st.knowledge.push(make_knowledge(re, span));
            });
        }
        if st.knowledge.is_empty() {
            return;
        }
        // Pass 2: matchers and group references.
        for node in ctx.nodes().iter() {
            match node.kind() {
                AstKind::CallExpression(c) => {
                    let Expression::StaticMemberExpression(m) = &c.callee else {
                        continue;
                    };
                    let Some(arg0) = c.arguments.first().and_then(Argument::as_expression) else {
                        continue;
                    };
                    if let Some(matcher) = lhs_symbol(ctx, node.id()) {
                        let method = m.property.name.as_str();
                        let pattern = if matches!(method, "match" | "matchAll")
                            && type_of(ctx, &m.object).is_string()
                        {
                            Some(arg0)
                        } else if method == "exec" && type_of(ctx, arg0).is_string() {
                            Some(&m.object)
                        } else {
                            None
                        };
                        if let Some(i) = pattern.and_then(|p| st.find(ctx, p, 0)) {
                            st.knowledge[i].matched = true;
                            st.bindings.insert(matcher, i);
                        }
                    }
                    if is_string_replace_call(ctx, c) {
                        let Some(substr) = c.arguments[1].as_expression() else {
                            continue;
                        };
                        let Some(i) = st.find(ctx, arg0, 0) else {
                            continue;
                        };
                        let Expression::StringLiteral(s) = strip_parens(substr) else {
                            continue;
                        };
                        let refs = group_references(s.value.as_str());
                        let k = &mut st.knowledge[i];
                        let mut indexed = Vec::new();
                        for (pos, g) in k.groups.iter_mut().enumerate() {
                            for r in &refs {
                                match r {
                                    GroupRef::Name { name, .. } if *name == g.name => g.used = true,
                                    GroupRef::Index {
                                        index: Some(ix), ..
                                    } if *ix == g.index => {
                                        g.used = true;
                                        if !indexed.contains(&pos) {
                                            indexed.push(pos);
                                        }
                                    }
                                    _ => {}
                                }
                            }
                        }
                        if !indexed.is_empty() {
                            let sec = secondaries(k, indexed.into_iter(), "Group");
                            ctx.report_with(
                                substr.span(),
                                "Directly use the group names instead of their numbers.",
                                sec,
                            );
                        }
                    }
                }
                AstKind::ComputedMemberExpression(m) => {
                    let Some(i) = st.resolve(ctx, &m.object) else {
                        continue;
                    };
                    let Some(v) = index_value(ctx, &m.expression) else {
                        continue;
                    };
                    let k = &mut st.knowledge[i];
                    if let Some(pos) = k.groups.iter().position(|g| g.index as f64 == v) {
                        k.groups[pos].used = true;
                        let msg = format!(
                            "Directly use '{}' instead of its group number.",
                            k.groups[pos].name
                        );
                        let sec = secondaries(k, std::iter::once(pos), "Group");
                        ctx.report_with(m.expression.span(), msg, sec);
                    }
                }
                AstKind::StaticMemberExpression(m) => {
                    let Some(i) = st.resolve(ctx, &m.object) else {
                        continue;
                    };
                    let parent = parent_skip_non_null(ctx, node.id());
                    let mut names = Vec::new();
                    match m.property.name.as_str() {
                        "groups" => group_name_nodes(parent, &mut names),
                        "indices" => {
                            if let AstKind::StaticMemberExpression(p) = parent.kind()
                                && p.property.name == "groups"
                            {
                                group_name_nodes(
                                    parent_skip_non_null(ctx, parent.id()),
                                    &mut names,
                                );
                            }
                        }
                        _ => continue,
                    }
                    let k = &mut st.knowledge[i];
                    for (name, span) in names {
                        if let Some(g) = k.groups.iter_mut().find(|g| g.name == name) {
                            g.used = true;
                        } else {
                            let sec = secondaries(k, 0..k.groups.len(), "Named group");
                            ctx.report_with(
                                span,
                                format!(
                                    "There is no group named '{name}' in the regular expression."
                                ),
                                sec,
                            );
                        }
                    }
                }
                _ => {}
            }
        }
        for k in &st.knowledge {
            if k.matched {
                let unused: Vec<usize> =
                    (0..k.groups.len()).filter(|&i| !k.groups[i].used).collect();
                if !unused.is_empty() {
                    let sec = secondaries(k, unused.into_iter(), "Named group");
                    ctx.report_with(
                        k.node,
                        "Use the named groups of this regex or remove the names.",
                        sec,
                    );
                }
            }
            for &(loc, pos) in &k.indexed_refs {
                let msg = format!(
                    "Directly use '{}' instead of its group number.",
                    k.groups[pos].name
                );
                ctx.report_with(loc, msg, secondaries(k, std::iter::once(pos), "Group"));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn named_groups() {
        check(
            "S5860",
            "a.js",
            r#"
const p1 = /(?<foo>\w)(?<bar>\w)/;          // Noncompliant {{Use the named groups of this regex or remove the names.}}
const m1 = 'str'.match(p1);
m1.groups.bar;

const p2 = /(?<foo>\w)/;
const m2 = 'str'.match(p2);
m2[1];                                       // Noncompliant {{Directly use 'foo' instead of its group number.}}
m2[2];

const p3 = /(?<foo>\w)/;
const m3 = p3.exec('str');
m3.indices.groups.foo;
m3.indices.groups.bar;                       // Noncompliant {{There is no group named 'bar' in the regular expression.}}

const p4 = /(?<foo>\w)/;
'str'.replace(p4, '$1');                     // Noncompliant {{Directly use the group names instead of their numbers.}}

/(?<foo>\w)\1(?<bar>\w)\k<bar>/;            // Noncompliant {{Directly use 'foo' instead of its group number.}}

const p5 = /(?<foo>\w)(?<bar>\w)/;
const m5 = 'str'.match(p5);
const { foo, bar } = m5.groups;

const p6 = /(?<foo>\w)/;
const m6 = 'str'.matchAll(p6);
m6.groups['foo'];

/(?<foo>\w)/;
const p7 = /(?<foo>\w)/;
p7.test('str');
const m8 = 'str'.matchAll(unknownPattern);
m8.groups.foo;
const p9 = /(?<foo>\w)/;
'str'.replace(p9, '$<foo>');
"#,
        );
    }
}
