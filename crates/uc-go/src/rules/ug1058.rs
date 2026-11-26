//! Values assigned to local variables should be used (staticcheck SA4006, ineffassign).
//!
//! An assignment to a local variable followed, in the same straight-line block, by another
//! assignment, a `return` or the end of the variable's scope, with no read in between. Variables
//! captured by closures, address-taken, named results and range variables are ignored, as are
//! functions with labels or `goto`, zero-value initializations (`x := 0`, `err = nil`) and anything
//! that may jump out of the block (`break`, `continue`) before the overwrite.
use crate::helpers_extra::*;
use crate::prelude::*;
use crate::scope;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// Kind ids of the hot paths (`Node::kind()` converts the name on every call).
struct Kinds {
    func: Vec<bool>,
    literal: Vec<bool>,
    jump: Vec<bool>,
    list: Vec<bool>,
    assign: Vec<bool>,
    ret: Vec<bool>,
    comment: Vec<bool>,
    ident: Vec<bool>,
}

static KINDS: std::sync::LazyLock<Kinds> = std::sync::LazyLock::new(|| {
    let lang = &*crate::LANGUAGE;
    let set = |names: &[&str]| -> Vec<bool> {
        (0..lang.node_kind_count() as u16)
            .map(|id| lang.node_kind_is_named(id) && lang.node_kind_for_id(id).is_some_and(|k| names.contains(&k)))
            .collect()
    };
    Kinds {
        func: set(&["function_declaration", "method_declaration", "func_literal"]),
        literal: set(&["func_literal"]),
        jump: set(&["goto_statement", "labeled_statement"]),
        list: set(&["statement_list"]),
        assign: set(&["short_var_declaration", "assignment_statement"]),
        ret: set(&["return_statement"]),
        comment: set(&["comment"]),
        ident: set(&["identifier", "type_identifier", "package_identifier"]),
    }
});

#[inline]
fn is(set: &[bool], n: Nd<'_>) -> bool {
    set.get(n.kind_id() as usize).copied().unwrap_or(false)
}

/// Innermost function-like ancestor.
fn func_of<'t>(ctx: &Ctx<'t>, n: Nd<'_>) -> Option<Nd<'t>> {
    let k = &*KINDS;
    ctx.ancestors(n).find(|a| is(&k.func, *a))
}

/// Cheap pre-check on names only (no scopes): some assignment of the function is followed in its
/// block by an overwrite, a `return` or the end of the block before the name is mentioned again.
/// The real analysis only reports a subset of these.
fn may_have_dead_store(ctx: &Ctx<'_>, func: Nd<'_>) -> bool {
    let k = &*KINDS;
    let mut kids: Vec<Nd<'_>> = Vec::new();
    for list in descendants(ctx, func).iter().filter(|n| is(&k.list, **n)) {
        kids.clear();
        let mut c = list.walk();
        kids.extend(list.named_children(&mut c).filter(|s| !is(&k.comment, *s)));
        for (i, stmt) in kids.iter().enumerate() {
            if !is(&k.assign, *stmt) {
                continue;
            }
            let Some(l) = stmt.child_by_field_name("left") else { continue };
            for x in expr_list(l) {
                let name = ctx.text(x);
                if name == "_" || x.kind() != "identifier" {
                    continue;
                }
                // First later mention of the name in the block.
                let rest = ctx.tokens_in(*list);
                let from = rest.partition_point(|&t| ctx.nodes[t as usize].start_byte() < stmt.end_byte());
                let mut scanned = rest[from..].iter().map(|&t| ctx.nodes[t as usize]).filter(|t| is(&k.ident, *t));
                let mention = scanned.by_ref().take(4096).find(|t| ctx.text(*t) == name);
                let limit = match mention {
                    Some(m) => kids.partition_point(|s| s.end_byte() <= m.start_byte()),
                    None => kids.len(),
                };
                if kids[i + 1..limit.max(i + 1)].iter().any(|s| is(&k.ret, *s)) {
                    return true;
                }
                match kids.get(limit) {
                    Some(t) if mention.is_some() => {
                        if is(&k.assign, *t)
                            && t.child_by_field_name("left")
                                .is_some_and(|tl| expr_list(tl).iter().any(|e| ctx.text(*e) == name))
                        {
                            return true;
                        }
                    }
                    _ => {
                        // End of the block: only dead in the function body or the declaring block.
                        let body = ctx.parent(*list).and_then(|b| ctx.parent(b)).is_some_and(|f| is(&k.func, f));
                        let declared = kids[..=i].iter().any(|s| match s.kind() {
                            "short_var_declaration" => s
                                .child_by_field_name("left")
                                .is_some_and(|l| expr_list(l).iter().any(|e| ctx.text(*e) == name)),
                            "var_declaration" => descendants(ctx, *s).iter().any(|v| {
                                v.kind() == "var_spec" && {
                                    let mut c = v.walk();
                                    v.children_by_field_name("name", &mut c).any(|n| ctx.text(n) == name)
                                }
                            }),
                            _ => false,
                        });
                        if body || declared {
                            return true;
                        }
                    }
                }
            }
        }
    }
    false
}

fn is_zero(ctx: &Ctx<'_>, v: Nd<'_>) -> bool {
    let v = strip_parens(v);
    match v.kind() {
        "nil" | "false" | "true" => true,
        "int_literal" | "float_literal" => ctx.text(v).bytes().all(|b| matches!(b, b'0' | b'.' | b'_' | b'x' | b'X')),
        "interpreted_string_literal" | "raw_string_literal" => ctx.text(v).len() == 2,
        "composite_literal" => v.child_by_field_name("body").is_some_and(|b| b.named_child_count() == 0),
        "unary_expression" => {
            v.child_by_field_name("operand").is_some_and(|o| o.kind() == "composite_literal" && is_zero(ctx, o))
        }
        // `T(0)`, `time.Duration(0)`
        "call_expression" => {
            let args = call_args(v);
            args.len() == 1
                && matches!(args[0].kind(), "int_literal" | "float_literal" | "nil")
                && is_zero(ctx, args[0])
        }
        _ => false,
    }
}

/// `buf = append(buf[:0], ...)`: written for the effect on the backing array shared with the caller.
fn refills(ctx: &Ctx<'_>, v: Nd<'_>, name: &str) -> bool {
    let v = strip_parens(v);
    v.kind() == "call_expression"
        && v.child_by_field_name("function").is_some_and(|f| ctx.text(f) == "append")
        && call_args(v).first().is_some_and(|a| {
            let a = strip_parens(*a);
            a.kind() == "slice_expression" && a.child_by_field_name("operand").is_some_and(|o| ctx.text(o) == name)
        })
}

/// The right-hand value assigned to `ident` by the assignment / declaration `stmt`, when there is a
/// one-to-one value (not a multi-value call).
fn assigned_value<'t>(stmt: Nd<'t>, ident: Nd<'_>) -> Option<Nd<'t>> {
    let (l, r) = match stmt.kind() {
        "short_var_declaration" | "assignment_statement" => {
            (stmt.child_by_field_name("left")?, stmt.child_by_field_name("right")?)
        }
        _ => return None,
    };
    let (ls, rs) = (expr_list(l), expr_list(r));
    let i = ls.iter().position(|x| x.id() == ident.id())?;
    (ls.len() == rs.len()).then(|| rs[i])
}

/// True if `stmt` assigns (`=` / `:=`) to `ident`-like targets directly (one of `writes` is a
/// top-level left-hand side of `stmt`).
fn direct_write(ctx: &Ctx<'_>, stmt: Nd<'_>, writes: &[Nd<'_>]) -> bool {
    let assigns = match stmt.kind() {
        "short_var_declaration" => true,
        "assignment_statement" => stmt.child_by_field_name("operator").is_some_and(|o| ctx.text(o) == "="),
        _ => false,
    };
    assigns
        && stmt
            .child_by_field_name("left")
            .is_some_and(|l| expr_list(l).iter().any(|e| writes.iter().any(|w| w.id() == e.id())))
}

/// A `break` / `continue` inside `stmt` that leaves `stmt` (jumps out of the straight-line block).
fn escapes(ctx: &Ctx<'_>, stmt: Nd<'_>) -> bool {
    descendants(ctx, stmt).iter().any(|n| {
        let target: &[&str] = match n.kind() {
            "break_statement" => {
                &["for_statement", "expression_switch_statement", "type_switch_statement", "select_statement"]
            }
            "continue_statement" => &["for_statement"],
            "goto_statement" => return true,
            _ => return false,
        };
        for a in ctx.ancestors(*n) {
            if a.id() == stmt.id() {
                return !target.contains(&stmt.kind());
            }
            if target.contains(&a.kind()) || is_function(a) {
                return false;
            }
        }
        true
    })
}

fn is_param_of(ctx: &Ctx<'_>, func: Nd<'_>, name: &str) -> bool {
    ["receiver", "parameters", "result"].iter().filter_map(|f| func.child_by_field_name(f)).any(|list| {
        list.kind() == "parameter_list"
            && param_decls(list).iter().any(|p| {
                let mut c = p.walk();
                p.children_by_field_name("name", &mut c).any(|n| ctx.text(n) == name)
            })
    })
}

/// A golangci-lint `//nolint` for ineffassign / staticcheck on the line, the line above, or above
/// the enclosing function.
fn nolint(ctx: &Ctx<'_>, at: Nd<'_>, func: Option<Nd<'_>>) -> bool {
    let line = ctx.line(at);
    let func_line = func.map_or(0, |f| ctx.line(f));
    ctx.comments.iter().any(|c| {
        let l = ctx.line(*c);
        if l != line && l + 1 != line && l + 1 != func_line {
            return false;
        }
        let Some(rest) = ctx.text(*c).strip_prefix("//nolint") else { return false };
        match rest.strip_prefix(':') {
            Some(list) => list
                .split_whitespace()
                .next()
                .unwrap_or("")
                .split(',')
                .any(|x| matches!(x, "ineffassign" | "staticcheck" | "wastedassign")),
            None => rest.is_empty() || rest.starts_with(' '),
        }
    })
}

fn within(n: Nd<'_>, outer: Nd<'_>) -> bool {
    outer.start_byte() <= n.start_byte() && n.end_byte() <= outer.end_byte()
}

impl GoRule for Rule {
    fn interests(&self) -> &'static [&'static str] {
        &["function_declaration", "method_declaration", "func_literal"]
    }

    fn run(&self, node: Nd<'_>, ctx: &Ctx<'_>) {
        // Function literals are handled with their enclosing declaration.
        if is(&KINDS.literal, node) && func_of(ctx, node).is_some() {
            return;
        }
        if node.child_by_field_name("body").is_none() {
            return;
        }
        if !may_have_dead_store(ctx, node) {
            return;
        }
        if descendants(ctx, node).iter().any(|n| is(&KINDS.jump, *n)) {
            return;
        }
        let scopes = scope::function_scopes(ctx, node);
        let mut names: Option<FxHashMap<&str, Vec<usize>>> = None;
        let mut lists: FxHashMap<usize, Vec<Nd<'_>>> = FxHashMap::default();
        let mut escaping: FxHashMap<(usize, usize), bool> = FxHashMap::default();
        for (i, d) in scopes.decls.iter().enumerate() {
            match d.decl.kind() {
                "short_var_declaration" | "var_spec" | "parameter_declaration" | "variadic_parameter_declaration" => {}
                _ => continue,
            }
            // Named results are read by `return` and by deferred functions.
            if d.decl.kind() == "parameter_declaration"
                && let Some(list) = ctx.parent(d.decl)
                && let Some(f) = ctx.parent(list)
                && f.child_by_field_name("result").is_some_and(|r| r.id() == list.id())
            {
                continue;
            }
            let home = func_of(ctx, d.ident);
            // `x, err := f()` at the top of a body reuses a parameter or named result `err`.
            if d.decl.kind() == "short_var_declaration"
                && let Some(h) = home
                && ctx
                    .parent(d.decl)
                    .and_then(|l| ctx.parent(l))
                    .and_then(|b| ctx.parent(b))
                    .is_some_and(|f| f.id() == h.id())
                && is_param_of(ctx, h, ctx.text(d.ident))
            {
                continue;
            }
            let refs = &scopes.refs[i];
            let writes = &scopes.writes[i];
            let captured = refs.iter().chain(writes).any(|r| func_of(ctx, *r).map(|f| f.id()) != home.map(|f| f.id()));
            let addressed = refs.iter().any(|r| {
                ctx.parent(*r).is_some_and(|p| p.kind() == "unary_expression" && ctx.text(p).starts_with('&'))
            });
            if captured || addressed {
                continue;
            }
            // Statement holding the declaration.
            let decl_stmt = match d.decl.kind() {
                "var_spec" => ctx.ancestors(d.decl).find(|a| a.kind() == "var_declaration"),
                "short_var_declaration" => Some(d.decl),
                _ => None,
            };
            let mut events: Vec<(Nd<'_>, Nd<'_>)> = Vec::new();
            if d.decl.kind() == "short_var_declaration" {
                events.push((d.ident, d.decl));
            }
            for w in writes {
                if let Some(stmt) = ctx.parent(*w).and_then(|l| ctx.parent(l))
                    && direct_write(ctx, stmt, std::slice::from_ref(w))
                {
                    events.push((*w, stmt));
                }
            }
            for (w, stmt) in events {
                let Some(list) = ctx.parent(stmt) else { continue };
                if list.kind() != "statement_list" {
                    continue;
                }
                match assigned_value(stmt, w) {
                    Some(v) if is_zero(ctx, v) || refills(ctx, v, ctx.text(w)) => continue,
                    _ => {}
                }
                let names = names.get_or_insert_with(|| name_positions(ctx, node));
                let kids = lists.entry(list.id()).or_insert_with(|| {
                    let mut c = list.walk();
                    list.named_children(&mut c).filter(|s| s.kind() != "comment").collect()
                });
                let Some(pos) = kids.iter().position(|k| k.id() == stmt.id()) else { continue };
                let name = ctx.text(w);
                let occurrences: &[usize] = names.get(name).map_or(&[], |v| &v[..]);
                // Next statement of the block mentioning the name (reads the scope builder may miss,
                // like map keys of composite literals, count as mentions).
                let after = occurrences.partition_point(|&p| p < stmt.end_byte());
                let touch = occurrences.get(after).and_then(|&p| {
                    let k = kids.partition_point(|k| k.end_byte() <= p);
                    (k < kids.len() && kids[k].start_byte() <= p).then_some(k)
                });
                let limit = touch.unwrap_or(kids.len());
                let mut dead = None;
                for (j, t) in kids.iter().enumerate().take(limit).skip(pos + 1) {
                    if t.kind() == "return_statement" {
                        dead = Some(true);
                        break;
                    }
                    if *escaping.entry((list.id(), j)).or_insert_with(|| escapes(ctx, *t)) {
                        dead = Some(false);
                        break;
                    }
                }
                let dead = match (dead, touch) {
                    (Some(d), _) => d,
                    (None, Some(k)) => {
                        let t = kids[k];
                        let mentions = occurrences[after..].partition_point(|&p| p < t.end_byte());
                        let written = writes.iter().filter(|x| within(**x, t)).count();
                        direct_write(ctx, t, writes) && mentions == written
                    }
                    (None, None) => {
                        // End of the block: dead if the variable's scope ends here or the function returns.
                        let declared_here =
                            decl_stmt.is_some_and(|s| ctx.parent(s).is_some_and(|p| p.id() == list.id()));
                        let body_end = ctx.parent(list).is_some_and(|b| {
                            b.kind() == "block" && ctx.parent(b).zip(home).is_some_and(|(f, h)| f.id() == h.id())
                        });
                        declared_here || body_end
                    }
                };
                if dead && !nolint(ctx, w, home) {
                    ctx.report(
                        w,
                        format!("Remove this useless assignment to \"{}\": the value is never read.", ctx.text(w)),
                    );
                }
            }
        }
    }
}

/// Start bytes of the identifier tokens of a function, by name.
fn name_positions<'t>(ctx: &Ctx<'t>, func: Nd<'_>) -> FxHashMap<&'t str, Vec<usize>> {
    let mut m: FxHashMap<&'t str, Vec<usize>> = FxHashMap::default();
    for &i in ctx.tokens_in(func) {
        let n = ctx.nodes[i as usize];
        if matches!(n.kind(), "identifier" | "type_identifier" | "package_identifier") {
            m.entry(ctx.text(n)).or_default().push(n.start_byte());
        }
    }
    m
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn rule() {
        check(
            "UG1058",
            r#"package p

func f() (int, error) {
	x := compute() // Noncompliant {{Remove this useless assignment to "x": the value is never read.}}
	x = compute()
	use(x)
	a, err := g() // Noncompliant {{Remove this useless assignment to "err": the value is never read.}}
	b, err := g()
	if err != nil {
		return 0, err
	}
	y := compute()
	y = y + 1 // Noncompliant
	return a + b, nil
}

func ok(c bool) int {
	n := 0
	n = compute()
	s := ""
	if c {
		s = "x"
	}
	use(s)
	v := compute()
	if c {
		v = compute()
	}
	use(v)
	w := compute()
	func() { w = 2 }()
	p := compute()
	ptr(&p)
	p = 3
	for i := 0; i < 3; i++ {
		last := compute()
		if c {
			continue
		}
		last = compute()
		use(last)
	}
	acc := 1
	for c {
		use(acc)
		acc = compute()
	}
	return n
}

func named() (res int, err error) {
	res = compute()
	err = check()
	return
}

func tail() {
	z := compute()
	use(z)
	z = compute() // Noncompliant
}

func loopBody(xs []int) {
	for range xs {
		t := compute() // Noncompliant
		t = compute()  // Noncompliant
	}
}

func refill(data []byte, rest []byte) int {
	data = append(data[:0], rest...)
	return len(rest)
}

func reuse(args []string) (res []string, err error) {
	res, rest, err := split(args)
	use(rest)
	return
}

//nolint:ineffassign
func silenced(in, out any) {
	out = in
}

func ambiguous(m map[string]map[string]int, k string) int {
	def, ok := lookup(k)
	rec := m[def.Table][k]
	if !ok {
		return 0
	}
	return rec
}

func jump(c bool) {
	q := compute()
	for c {
		q = compute()
		if c {
			break
		}
		q = 1
	}
	use(q)
}
"#,
        );
    }
}
