//! Lexical scopes for Go: resolves identifiers inside a function to their declarations.

use rustc_hash::FxHashMap;
use tree_sitter::Node;

use crate::Ctx;

/// A declared local name.
#[derive(Debug, Clone, Copy)]
pub struct Decl<'t> {
    pub ident: Node<'t>,
    /// The declaring construct (parameter_declaration, short_var_declaration, var_spec, ...).
    pub decl: Node<'t>,
}

/// Local declarations and references of one function body.
#[derive(Debug, Default)]
pub struct FunctionScopes<'t> {
    pub decls: Vec<Decl<'t>>,
    /// For each declaration index, the identifier nodes referring to it.
    pub refs: Vec<Vec<Node<'t>>>,
    /// Identifiers that are assigned to (lhs of `=`, `op=`, `++`, `--`).
    pub writes: Vec<Vec<Node<'t>>>,
}

/// Builds scopes for a function-like node (declaration, method or literal).
pub fn function_scopes<'t>(ctx: &Ctx<'t>, func: Node<'t>) -> FunctionScopes<'t> {
    let mut b = Builder { ctx, out: FunctionScopes::default(), stack: vec![FxHashMap::default()] };
    if let Some(recv) = func.child_by_field_name("receiver") {
        b.params(recv);
    }
    if let Some(p) = func.child_by_field_name("parameters") {
        b.params(p);
    }
    if let Some(r) = func.child_by_field_name("result")
        && r.kind() == "parameter_list"
    {
        b.params(r);
    }
    if let Some(body) = func.child_by_field_name("body") {
        b.walk(body);
    }
    b.out
}

struct Builder<'c, 't> {
    ctx: &'c Ctx<'t>,
    out: FunctionScopes<'t>,
    stack: Vec<FxHashMap<&'t str, usize>>,
}

impl<'t> Builder<'_, 't> {
    fn declare(&mut self, ident: Node<'t>, decl: Node<'t>) {
        let name = self.ctx.text(ident);
        if name == "_" {
            return;
        }
        let i = self.out.decls.len();
        self.out.decls.push(Decl { ident, decl });
        self.out.refs.push(Vec::new());
        self.out.writes.push(Vec::new());
        self.stack.last_mut().unwrap().insert(name, i);
    }

    fn lookup(&self, name: &str) -> Option<usize> {
        self.stack.iter().rev().find_map(|s| s.get(name).copied())
    }

    fn params(&mut self, list: Node<'t>) {
        let mut c = list.walk();
        for p in list.named_children(&mut c) {
            if matches!(p.kind(), "parameter_declaration" | "variadic_parameter_declaration") {
                let mut c2 = p.walk();
                for n in p.children_by_field_name("name", &mut c2) {
                    self.declare(n, p);
                }
            }
        }
    }

    fn push(&mut self) {
        self.stack.push(FxHashMap::default());
    }

    fn pop(&mut self) {
        self.stack.pop();
    }

    fn reference(&mut self, ident: Node<'t>, write: bool) {
        if let Some(i) = self.lookup(self.ctx.text(ident)) {
            if write {
                self.out.writes[i].push(ident);
            } else {
                self.out.refs[i].push(ident);
            }
        }
    }

    fn lhs(&mut self, list: Node<'t>) {
        let mut c = list.walk();
        for e in list.named_children(&mut c) {
            if e.kind() == "identifier" {
                self.reference(e, true);
            } else {
                self.walk(e);
            }
        }
    }

    fn walk(&mut self, n: Node<'t>) {
        match n.kind() {
            "block" | "if_statement" | "for_statement" | "expression_case" | "default_case" | "type_case"
            | "communication_case" | "expression_switch_statement" | "type_switch_statement" | "select_statement" => {
                self.push();
                if n.kind() == "type_switch_statement" {
                    // `switch v := x.(type)`: `v` is declared in every clause.
                    if let Some(alias) = n.child_by_field_name("alias") {
                        let mut c = alias.walk();
                        let names: Vec<_> = alias.named_children(&mut c).collect();
                        if let Some(v) = n.child_by_field_name("value") {
                            self.walk(v);
                        }
                        for name in names {
                            self.declare(name, n);
                        }
                        let mut c = n.walk();
                        for ch in n.named_children(&mut c) {
                            if ch.kind() == "type_case" || ch.kind() == "default_case" {
                                self.walk(ch);
                            }
                        }
                        self.pop();
                        return;
                    }
                }
                self.children(n);
                self.pop();
            }
            "short_var_declaration" => {
                if let Some(r) = n.child_by_field_name("right") {
                    self.walk(r);
                }
                if let Some(l) = n.child_by_field_name("left") {
                    let mut c = l.walk();
                    for id in l.named_children(&mut c) {
                        if id.kind() == "identifier" {
                            // Redeclaration in the same scope is an assignment.
                            let name = self.ctx.text(id);
                            if self.stack.last().unwrap().contains_key(name) {
                                self.reference(id, true);
                            } else {
                                self.declare(id, n);
                            }
                        }
                    }
                }
            }
            "var_spec" | "const_spec" => {
                if let Some(v) = n.child_by_field_name("value") {
                    self.walk(v);
                }
                let mut c = n.walk();
                for id in n.children_by_field_name("name", &mut c) {
                    self.declare(id, n);
                }
            }
            "range_clause" => {
                if let Some(r) = n.child_by_field_name("right") {
                    self.walk(r);
                }
                if let Some(l) = n.child_by_field_name("left") {
                    let is_define = {
                        let mut c = n.walk();
                        n.children(&mut c).any(|ch| ch.kind() == ":=")
                    };
                    if is_define {
                        let mut c = l.walk();
                        for id in l.named_children(&mut c) {
                            if id.kind() == "identifier" {
                                self.declare(id, n);
                            }
                        }
                    } else {
                        self.lhs(l);
                    }
                }
            }
            "receive_statement" if n.child_by_field_name("left").is_some() => {
                if let Some(r) = n.child_by_field_name("right") {
                    self.walk(r);
                }
                let l = n.child_by_field_name("left").unwrap();
                let mut c = l.walk();
                for id in l.named_children(&mut c) {
                    if id.kind() == "identifier" {
                        self.declare(id, n);
                    }
                }
            }
            "assignment_statement" => {
                if let Some(r) = n.child_by_field_name("right") {
                    self.walk(r);
                }
                if let Some(l) = n.child_by_field_name("left") {
                    self.lhs(l);
                }
            }
            "inc_statement" | "dec_statement" => {
                if let Some(e) = n.named_child(0) {
                    if e.kind() == "identifier" {
                        self.reference(e, true);
                    } else {
                        self.walk(e);
                    }
                }
            }
            "func_literal" => {
                // Closures see enclosing locals; their params shadow.
                self.push();
                if let Some(p) = n.child_by_field_name("parameters") {
                    self.params(p);
                }
                if let Some(r) = n.child_by_field_name("result")
                    && r.kind() == "parameter_list"
                {
                    self.params(r);
                }
                if let Some(b) = n.child_by_field_name("body") {
                    self.walk(b);
                }
                self.pop();
            }
            "identifier" => self.reference(n, false),
            "selector_expression" => {
                if let Some(o) = n.child_by_field_name("operand") {
                    self.walk(o);
                }
            }
            "keyed_element" => {
                // `T{Field: v}`: the key is a field name unless it resolves to a local.
                let mut c = n.walk();
                let kids: Vec<_> = n.named_children(&mut c).collect();
                for (i, k) in kids.iter().enumerate() {
                    if i == 0 && k.kind() == "literal_element" {
                        if let Some(id) = k.named_child(0)
                            && id.kind() == "identifier"
                        {
                            continue;
                        }
                    }
                    self.walk(*k);
                }
            }
            "labeled_statement" => {
                if let Some(s) = n.named_child(1) {
                    self.walk(s);
                }
            }
            _ => self.children(n),
        }
    }

    fn children(&mut self, n: Node<'t>) {
        let mut c = n.walk();
        let kids: Vec<_> = n.named_children(&mut c).collect();
        for k in kids {
            self.walk(k);
        }
    }
}
