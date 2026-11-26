//! A small YAML document model built from tree-sitter-yaml.

use std::cell::RefCell;

use tree_sitter::{Node, Parser};
use uc_core::Span;

thread_local! {
    static PARSER: RefCell<Parser> = RefCell::new({
        let mut p = Parser::new();
        p.set_language(&tree_sitter_yaml::LANGUAGE.into()).expect("tree-sitter-yaml");
        p
    });
}

#[derive(Debug)]
pub(crate) struct Y {
    pub span: Span,
    pub v: V,
}

#[derive(Debug)]
pub(crate) enum V {
    Map(Vec<Pair>),
    Seq(Vec<Y>),
    Scalar {
        text: String,
    },
    /// An alias (`*name`): value unknown.
    Alias,
    Null,
}

#[derive(Debug)]
pub(crate) struct Pair {
    pub key: String,
    pub key_span: Span,
    pub val: Y,
}

pub(crate) struct Doc {
    pub root: Y,
    /// The document has syntax errors: rules stay silent on it.
    pub has_error: bool,
}

pub(crate) struct Parsed {
    pub docs: Vec<Doc>,
    pub comments: Vec<Span>,
}

const NULL: Y = Y { span: Span { start: 0, end: 0 }, v: V::Null };

impl Y {
    /// The value of `key` in a mapping (the last one when duplicated, like Kubernetes).
    pub fn get(&self, key: &str) -> Option<&Y> {
        match &self.v {
            V::Map(p) => p.iter().rev().find(|p| p.key == key).map(|p| &p.val),
            _ => None,
        }
    }

    /// The pair of `key` in a mapping.
    pub fn pair(&self, key: &str) -> Option<&Pair> {
        match &self.v {
            V::Map(p) => p.iter().rev().find(|p| p.key == key),
            _ => None,
        }
    }

    pub fn path(&self, keys: &[&str]) -> Option<&Y> {
        keys.iter().try_fold(self, |y, k| y.get(k))
    }

    pub fn str(&self) -> Option<&str> {
        match &self.v {
            V::Scalar { text, .. } => Some(text),
            _ => None,
        }
    }

    pub fn seq(&self) -> &[Y] {
        match &self.v {
            V::Seq(s) => s,
            _ => &[],
        }
    }

    pub fn pairs(&self) -> &[Pair] {
        match &self.v {
            V::Map(p) => p,
            _ => &[],
        }
    }

    pub fn is_map(&self) -> bool {
        matches!(self.v, V::Map(_))
    }

    /// True for the boolean `true`.
    pub fn is_true(&self) -> bool {
        self.str().is_some_and(|s| matches!(s, "true" | "True" | "TRUE"))
    }

    pub fn is_false(&self) -> bool {
        self.str().is_some_and(|s| matches!(s, "false" | "False" | "FALSE"))
    }

    /// The raw source text of this node.
    pub fn raw<'s>(&self, src: &'s str) -> &'s str {
        src.get(self.span.start as usize..self.span.end as usize).unwrap_or("")
    }
}

/// Parses a YAML stream. `None` if tree-sitter gives up.
pub(crate) fn parse(src: &str) -> Option<Parsed> {
    let tree = PARSER.with_borrow_mut(|p| p.parse(src, None))?;
    let mut b = Builder { src, comments: Vec::new(), error: false };
    let mut docs = Vec::new();
    let root = tree.root_node();
    let mut c = root.walk();
    for child in root.children(&mut c) {
        match child.kind() {
            "document" => {
                b.error = child.has_error();
                let mut root = None;
                let mut cc = child.walk();
                for d in child.children(&mut cc) {
                    match d.kind() {
                        "block_node" | "flow_node" if root.is_none() => root = Some(b.node(d)),
                        "comment" => b.comment(d),
                        _ => b.visit_other(d),
                    }
                }
                docs.push(Doc { root: root.unwrap_or(NULL), has_error: b.error });
            }
            "comment" => b.comment(child),
            _ => b.visit_other(child),
        }
    }
    b.comments.sort_unstable();
    Some(Parsed { docs, comments: b.comments })
}

struct Builder<'s> {
    src: &'s str,
    comments: Vec<Span>,
    error: bool,
}

fn sp(n: Node) -> Span {
    Span::new(n.start_byte() as u32, n.end_byte() as u32)
}

impl Builder<'_> {
    fn comment(&mut self, n: Node) {
        self.comments.push(sp(n));
    }

    /// A node outside the model: record its comments and errors.
    fn visit_other(&mut self, n: Node) {
        if n.kind() == "comment" {
            self.comment(n);
            return;
        }
        if n.is_error() || n.is_missing() {
            self.error = true;
        }
        let mut c = n.walk();
        for ch in n.children(&mut c) {
            self.visit_other(ch);
        }
    }

    fn text(&self, n: Node) -> &str {
        &self.src[n.start_byte()..n.end_byte()]
    }

    fn node(&mut self, n: Node) -> Y {
        let span = sp(n);
        match n.kind() {
            "block_node" | "flow_node" => {
                let mut content = None;
                let mut c = n.walk();
                for ch in n.children(&mut c) {
                    match ch.kind() {
                        "anchor" | "tag" => {}
                        "comment" => self.comment(ch),
                        _ if content.is_none() => content = Some(self.node(ch)),
                        _ => self.visit_other(ch),
                    }
                }
                content.unwrap_or(Y { span, v: V::Null })
            }
            "block_mapping" | "flow_mapping" => {
                let mut pairs = Vec::new();
                let mut c = n.walk();
                for ch in n.children(&mut c) {
                    match ch.kind() {
                        "block_mapping_pair" | "flow_pair" => {
                            let key = ch.child_by_field_name("key");
                            let val = ch.child_by_field_name("value");
                            let mut cc = ch.walk();
                            for x in ch.children(&mut cc) {
                                if x.kind() == "comment" {
                                    self.comment(x);
                                } else if x.is_error() || x.is_missing() {
                                    self.error = true;
                                }
                            }
                            let (key, key_span) = match key {
                                Some(k) => {
                                    let ky = self.node(k);
                                    let text = match &ky.v {
                                        V::Scalar { text, .. } => text.clone(),
                                        _ => self.text(k).to_string(),
                                    };
                                    (text, sp(k))
                                }
                                None => (String::new(), Span::new(span.start, span.start)),
                            };
                            let val = match val {
                                Some(v) => self.node(v),
                                None => Y { span: Span::new(key_span.end, key_span.end), v: V::Null },
                            };
                            pairs.push(Pair { key, key_span, val });
                        }
                        "flow_node" => {
                            // A key without a value in a flow mapping.
                            let k = self.node(ch);
                            let key = k.str().unwrap_or_default().to_string();
                            pairs.push(Pair { key, key_span: k.span, val: Y { span: k.span, v: V::Null } });
                        }
                        "comment" => self.comment(ch),
                        _ => self.visit_other(ch),
                    }
                }
                Y { span, v: V::Map(pairs) }
            }
            "block_sequence" | "flow_sequence" => {
                let mut items = Vec::new();
                let mut c = n.walk();
                for ch in n.children(&mut c) {
                    match ch.kind() {
                        "block_sequence_item" => {
                            let mut item = None;
                            let mut cc = ch.walk();
                            for x in ch.children(&mut cc) {
                                match x.kind() {
                                    "block_node" | "flow_node" if item.is_none() => item = Some(self.node(x)),
                                    "comment" => self.comment(x),
                                    _ => self.visit_other(x),
                                }
                            }
                            items.push(item.unwrap_or(Y { span: sp(ch), v: V::Null }));
                        }
                        "flow_node" => items.push(self.node(ch)),
                        "flow_pair" => {
                            let k = ch.child_by_field_name("key").map(|k| self.node(k));
                            let v = ch.child_by_field_name("value").map(|v| self.node(v));
                            let key = k.as_ref().and_then(|k| k.str()).unwrap_or_default().to_string();
                            let key_span = k.as_ref().map_or(sp(ch), |k| k.span);
                            let val = v.unwrap_or(Y { span: key_span, v: V::Null });
                            items.push(Y { span: sp(ch), v: V::Map(vec![Pair { key, key_span, val }]) });
                        }
                        "comment" => self.comment(ch),
                        _ => self.visit_other(ch),
                    }
                }
                Y { span, v: V::Seq(items) }
            }
            "plain_scalar" => {
                let t = self.text(n);
                let text = if t.contains('\n') { fold(t) } else { t.trim().to_string() };
                Y { span, v: V::Scalar { text } }
            }
            "single_quote_scalar" => {
                let t = self.text(n);
                let inner = t.get(1..t.len().saturating_sub(1)).unwrap_or("");
                Y { span, v: V::Scalar { text: inner.replace("''", "'") } }
            }
            "double_quote_scalar" => {
                let t = self.text(n);
                let inner = t.get(1..t.len().saturating_sub(1)).unwrap_or("");
                Y { span, v: V::Scalar { text: unescape(inner) } }
            }
            "block_scalar" => Y { span, v: V::Scalar { text: block_scalar(self.text(n)) } },
            "alias" => Y { span, v: V::Alias },
            _ => {
                self.visit_other(n);
                Y { span, v: V::Null }
            }
        }
    }
}

fn fold(t: &str) -> String {
    t.lines().map(str::trim).filter(|l| !l.is_empty()).collect::<Vec<_>>().join(" ")
}

fn unescape(s: &str) -> String {
    if !s.contains('\\') {
        return if s.contains('\n') { fold(s) } else { s.to_string() };
    }
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match it.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('"') => out.push('"'),
            Some('\\') => out.push('\\'),
            Some('/') => out.push('/'),
            Some('\n') => {}
            Some(o) => {
                out.push('\\');
                out.push(o);
            }
            None => out.push('\\'),
        }
    }
    out
}

/// The content of a `|` / `>` block scalar, dedented (folding is approximated by spaces).
fn block_scalar(t: &str) -> String {
    let folded = t.starts_with('>');
    let mut lines = t.lines();
    lines.next();
    let body: Vec<&str> = lines.collect();
    let indent = body.iter().filter(|l| !l.trim().is_empty()).map(|l| l.len() - l.trim_start().len()).min();
    let indent = indent.unwrap_or(0);
    let parts = body.iter().map(|l| l.get(indent..).unwrap_or("").trim_end_matches('\r'));
    if folded {
        parts.filter(|l| !l.is_empty()).collect::<Vec<_>>().join(" ")
    } else {
        parts.collect::<Vec<_>>().join("\n")
    }
}

/// Mapping keys that occur more than once (merge keys `<<` excepted), recursively.
pub(crate) fn duplicate_keys<'y>(y: &'y Y, out: &mut Vec<(&'y Pair, &'y Pair)>) {
    match &y.v {
        V::Map(pairs) => {
            for (i, p) in pairs.iter().enumerate() {
                if p.key != "<<"
                    && !p.key.is_empty()
                    && let Some(first) = pairs[..i].iter().find(|q| q.key == p.key)
                {
                    out.push((first, p));
                }
                duplicate_keys(&p.val, out);
            }
        }
        V::Seq(items) => items.iter().for_each(|i| duplicate_keys(i, out)),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model() {
        let src = "# c\na: 1\nb:\n  - x\n  - 'y'\n  - {k: \"v\\n\"}\nc: |\n  line1\n  line2\nd:\n---\ne: *ref\n";
        let p = parse(src).unwrap();
        assert_eq!(p.docs.len(), 2);
        let r = &p.docs[0].root;
        assert_eq!(r.get("a").unwrap().str(), Some("1"));
        assert_eq!(r.get("b").unwrap().seq().len(), 3);
        assert_eq!(r.get("b").unwrap().seq()[1].str(), Some("y"));
        assert_eq!(r.get("b").unwrap().seq()[2].get("k").unwrap().str(), Some("v\n"));
        assert_eq!(r.get("c").unwrap().str(), Some("line1\nline2"));
        assert!(matches!(r.get("d").unwrap().v, V::Null));
        assert!(matches!(p.docs[1].root.get("e").unwrap().v, V::Alias));
        assert_eq!(p.comments.len(), 1);
        assert!(!p.docs[0].has_error);
    }

    #[test]
    fn duplicates() {
        let p = parse("a: 1\nb: 2\na: 3\n<<: *x\n<<: *y\n").unwrap();
        let mut d = Vec::new();
        duplicate_keys(&p.docs[0].root, &mut d);
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].1.key, "a");
    }
}
