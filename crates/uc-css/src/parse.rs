//! A tolerant single-pass parser for CSS, SCSS and Less that runs the rules as it goes.
//!
//! The structural scanner (`trivia`, `scan`, `string`, `url`) visits every byte once and splits the
//! source into statements: at-rules, rules (selector + block), declarations and other statements
//! (Less mixin calls and the like). Selectors and values are then re-scanned by small dedicated
//! walkers, which only look at their own span.

use std::hash::Hasher;

use rustc_hash::{FxHashMap, FxHasher};
use uc_core::{LineIndex, RuleSet};

use crate::data::*;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dialect {
    Css,
    Scss,
    Less,
}

/// A rule hit, in byte offsets.
pub struct Raw {
    pub key: &'static str,
    pub start: u32,
    pub end: u32,
    pub msg: String,
}

/// Which rules are on, and their options.
#[derive(Default)]
pub struct Config {
    s1116: bool,
    s1128: bool,
    s4647: bool,
    s4648: bool,
    s4649: bool,
    s4650: bool,
    s4651: bool,
    s4652: bool,
    s4653: bool,
    s4654: bool,
    s4655: bool,
    s4656: bool,
    s4657: bool,
    s4658: bool,
    s4659: bool,
    s4660: bool,
    s4661: bool,
    s4662: bool,
    s4663: bool,
    s4666: bool,
    s4667: bool,
    s4668: bool,
    s4670: bool,
    s5362: bool,
    ignore_props: Vec<String>,
    ignore_at: Vec<String>,
    ignore_pc: Vec<String>,
    ignore_pe: Vec<String>,
    ignore_types: Vec<String>,
    fallbacks: bool,
}

impl Config {
    pub fn new(rules: &RuleSet, is_test: bool) -> Self {
        let on = |k: &str| rules.on(k, is_test);
        let list = |k: &str, p: &str| -> Vec<String> {
            rules
                .get(k)
                .map(|r| r.params.list(p, &[]).into_iter().map(|s| s.trim().to_ascii_lowercase()).collect())
                .unwrap_or_default()
        };
        Self {
            s1116: on("S1116"),
            s1128: on("S1128"),
            s4647: on("S4647"),
            s4648: on("S4648"),
            s4649: on("S4649"),
            s4650: on("S4650"),
            s4651: on("S4651"),
            s4652: on("S4652"),
            s4653: on("S4653"),
            s4654: on("S4654"),
            s4655: on("S4655"),
            s4656: on("S4656"),
            s4657: on("S4657"),
            s4658: on("S4658"),
            s4659: on("S4659"),
            s4660: on("S4660"),
            s4661: on("S4661"),
            s4662: on("S4662"),
            s4663: on("S4663"),
            s4666: on("S4666"),
            s4667: on("S4667"),
            s4668: on("S4668"),
            s4670: on("S4670"),
            s5362: on("S5362"),
            ignore_props: list("S4654", "ignoreProperties"),
            ignore_at: list("S4662", "ignoreAtRules"),
            ignore_pc: list("S4659", "ignorePseudoClasses"),
            ignore_pe: list("S4660", "ignorePseudoElements"),
            ignore_types: list("S4670", "ignoreTypes"),
            fallbacks: rules.get("S4656").is_none_or(|r| r.params.bool("ignoreFallbacks", true)),
        }
    }

    /// Every rule on with default options (tests).
    #[cfg(test)]
    pub fn all() -> Self {
        Self::new(&RuleSet::all(uc_core::Repo::Css, crate::IMPLEMENTED), false)
    }

    pub fn any(&self) -> bool {
        self.s1116
            || self.s1128
            || self.s4647
            || self.s4648
            || self.s4649
            || self.s4650
            || self.s4651
            || self.s4652
            || self.s4653
            || self.s4654
            || self.s4655
            || self.s4656
            || self.s4657
            || self.s4658
            || self.s4659
            || self.s4660
            || self.s4661
            || self.s4662
            || self.s4663
            || self.s4666
            || self.s4667
            || self.s4668
            || self.s4670
            || self.s5362
    }
}

/// Parses `src` and returns the rule hits.
pub fn run(src: &str, d: Dialect, cfg: &Config, lines: &LineIndex) -> Vec<Raw> {
    let mut p = Parser {
        s: src,
        b: src.as_bytes(),
        n: src.len(),
        i: 0,
        d,
        c: cfg,
        lines,
        out: Vec::new(),
        frames: Vec::new(),
        imports: Vec::new(),
        norm: Vec::new(),
        parts: Vec::new(),
    };
    if p.b.starts_with(b"\xEF\xBB\xBF") {
        p.i = 3;
    }
    if cfg.s4667 && p.b[p.i..].iter().all(|&c| is_ws(c)) {
        p.rep("S4667", 0, 0, "Unexpected empty source".into());
        return p.out;
    }
    p.block(0, Kind::Root);
    p.out
}

const MAX_DEPTH: usize = 200;
const NONE: usize = usize::MAX;

#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
enum Kind {
    #[default]
    Root,
    Rule,
    AtRule,
    /// An unknown at-rule or one whose descriptors are author-defined: no property checks.
    Opaque,
    /// CSS modules `:export` / `:import`: arbitrary keys.
    Export,
    /// SCSS nested properties (`font: { family: x; }`).
    NestedProps,
    Keyframes,
    KeyframeSel,
    FontFace,
}

#[derive(Clone, Copy)]
struct Decl {
    ps: usize,
    pe: usize,
    vs: usize,
    ve: usize,
}

#[derive(Default)]
struct Frame {
    kind: Kind,
    decls: Vec<Decl>,
    sels: FxHashMap<u64, usize>,
    nodes: u32,
}

/// The end of a statement: the terminator (`{`, `;`, `}` or 0 at EOF) and the first top-level colon.
#[derive(Clone, Copy)]
struct End {
    pos: usize,
    term: u8,
    colon: usize,
}

struct Parser<'a> {
    s: &'a str,
    b: &'a [u8],
    n: usize,
    i: usize,
    d: Dialect,
    c: &'a Config,
    lines: &'a LineIndex<'a>,
    out: Vec<Raw>,
    frames: Vec<Frame>,
    imports: Vec<u64>,
    norm: Vec<u8>,
    parts: Vec<(usize, usize)>,
}

#[inline]
fn is_ws(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | b'\r' | b'\x0c')
}

#[inline]
fn is_ident_char(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'-' || c == b'_' || c >= 0x80
}

#[inline]
fn is_ident_start(c: u8) -> bool {
    c.is_ascii_alphabetic() || c == b'_' || c == b'-' || c >= 0x80 || c == b'\\'
}

#[inline]
fn has(set: &[&str], key: &[u8]) -> bool {
    // Inline byte comparison: keys are short, and a libc memcmp call per probe dominates.
    #[inline(always)]
    fn cmp(a: &[u8], b: &[u8]) -> std::cmp::Ordering {
        let n = a.len().min(b.len());
        for i in 0..n {
            if a[i] != b[i] {
                return a[i].cmp(&b[i]);
            }
        }
        a.len().cmp(&b.len())
    }
    set.binary_search_by(|p| cmp(p.as_bytes(), key)).is_ok()
}

/// ASCII-lowercases `s` into `buf`; `None` when too long or not ASCII.
#[inline]
fn lower<'b>(s: &[u8], buf: &'b mut [u8; 64]) -> Option<&'b [u8]> {
    if s.len() > 64 || !s.is_ascii() {
        return None;
    }
    for (d, &c) in buf.iter_mut().zip(s) {
        *d = c.to_ascii_lowercase();
    }
    Some(&buf[..s.len()])
}

fn listed(list: &[String], key: &[u8]) -> bool {
    list.iter().any(|s| s.as_bytes() == key)
}

/// `-webkit-foo` style vendor prefix (not a custom property).
#[inline]
fn vendor(s: &[u8]) -> bool {
    s.len() > 2 && s[0] == b'-' && s[1] != b'-'
}

/// Strips a vendor prefix: `-webkit-transition` -> (`-webkit-`, `transition`).
fn unprefix(s: &[u8]) -> (&[u8], &[u8]) {
    if vendor(s)
        && let Some(k) = s[1..].iter().position(|&c| c == b'-')
    {
        return s.split_at(k + 2);
    }
    (&s[..0], s)
}

/// Substring test for the short spans looked at here (no searcher setup cost).
fn contains(h: &[u8], n: &[u8]) -> bool {
    let Some((&first, rest)) = n.split_first() else { return true };
    memchr::memchr_iter(first, h).any(|k| h[k + 1..].starts_with(rest))
}

/// `#{...}` (SCSS) or `@{...}` (Less) interpolation.
fn has_interp(b: &[u8]) -> bool {
    memchr::memchr_iter(b'{', b).any(|k| k > 0 && matches!(b[k - 1], b'#' | b'@'))
}

/// Index after the string starting at `i` (stops at a newline or `end` when unterminated).
fn skip_str(b: &[u8], i: usize, end: usize) -> usize {
    let q = b[i];
    let mut j = i + 1;
    while j < end {
        match b[j] {
            b'\\' => j += 2,
            c if c == q => return j + 1,
            b'\n' => return j,
            _ => j += 1,
        }
    }
    end
}

/// Index after the comment starting at `i` (`/*`).
fn comment_end(b: &[u8], i: usize, end: usize) -> usize {
    let s = (i + 2).min(end);
    memchr::memchr_iter(b'*', &b[s..end]).find(|&k| s + k + 1 < end && b[s + k + 1] == b'/').map_or(end, |k| s + k + 2)
}

fn eol(b: &[u8], i: usize, end: usize) -> usize {
    memchr::memchr(b'\n', &b[i..end]).map_or(end, |k| i + k)
}

/// Index of the `close` matching the opener at `i`, or `end`.
fn matching(b: &[u8], i: usize, end: usize, open: u8, close: u8) -> usize {
    let mut depth = 0u32;
    let mut j = i;
    while j < end {
        match b[j] {
            b'"' | b'\'' => {
                j = skip_str(b, j, end);
                continue;
            }
            b'\\' => j += 1,
            b'/' if b.get(j + 1) == Some(&b'*') => {
                j = comment_end(b, j, end);
                continue;
            }
            c if c == open => depth += 1,
            c if c == close => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return j;
                }
            }
            _ => {}
        }
        j += 1;
    }
    end
}

/// Index after the `}` matching the `{` at `i`.
fn skip_braces(b: &[u8], i: usize, end: usize) -> usize {
    (matching(b, i, end, b'{', b'}') + 1).min(end)
}

fn ident_end(b: &[u8], mut i: usize, end: usize) -> usize {
    while i < end {
        if b[i] == b'\\' {
            i += 2;
        } else if is_ident_char(b[i]) {
            i += 1;
        } else {
            break;
        }
    }
    i.min(end)
}

fn trim(b: &[u8], mut s: usize, mut e: usize) -> (usize, usize) {
    while s < e && is_ws(b[s]) {
        s += 1;
    }
    while e > s && is_ws(b[e - 1]) {
        e -= 1;
    }
    (s, e)
}

fn is_url(b: &[u8], i: usize) -> bool {
    b.len() >= i + 4 && b[i..i + 4].eq_ignore_ascii_case(b"url(") && (i == 0 || !is_ident_char(b[i - 1]))
}

/// Splits `[s, e)` at top-level commas.
fn split_commas(b: &[u8], s: usize, e: usize, out: &mut Vec<(usize, usize)>) {
    out.clear();
    let mut depth = 0u32;
    let mut start = s;
    let mut i = s;
    while i < e {
        match b[i] {
            b'"' | b'\'' => {
                i = skip_str(b, i, e);
                continue;
            }
            b'\\' => i += 1,
            b'/' if b.get(i + 1) == Some(&b'*') => {
                i = comment_end(b, i, e);
                continue;
            }
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth = depth.saturating_sub(1),
            b',' if depth == 0 => {
                out.push((start, i));
                start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    out.push((start, e));
}

/// `!important` at the end of a value: (value end without it, position of `!`).
fn strip_important(b: &[u8], vs: usize, ve: usize) -> (usize, Option<usize>) {
    let (_, e) = trim(b, vs, ve);
    if e >= vs + 9 && b[e - 9..e].eq_ignore_ascii_case(b"important") {
        let (_, e2) = trim(b, vs, e - 9);
        if e2 > vs && b[e2 - 1] == b'!' {
            let (_, ve2) = trim(b, vs, e2 - 1);
            return (ve2, Some(e2 - 1));
        }
    }
    (e, None)
}

const SIDES: [&[u8]; 4] = [b"top", b"bottom", b"left", b"right"];

/// Longhands reset by a shorthand (unprefixed names).
fn longhands(short: &[u8]) -> &'static [&'static str] {
    match short {
        b"margin" => &["margin-top", "margin-right", "margin-bottom", "margin-left"],
        b"padding" => &["padding-top", "padding-right", "padding-bottom", "padding-left"],
        b"margin-block" => &["margin-block-start", "margin-block-end"],
        b"margin-inline" => &["margin-inline-start", "margin-inline-end"],
        b"padding-block" => &["padding-block-start", "padding-block-end"],
        b"padding-inline" => &["padding-inline-start", "padding-inline-end"],
        b"inset" => &["top", "right", "bottom", "left"],
        b"inset-block" => &["inset-block-start", "inset-block-end"],
        b"inset-inline" => &["inset-inline-start", "inset-inline-end"],
        b"background" => &[
            "background-image",
            "background-size",
            "background-position",
            "background-position-x",
            "background-position-y",
            "background-repeat",
            "background-origin",
            "background-clip",
            "background-attachment",
            "background-color",
        ],
        b"background-position" => &["background-position-x", "background-position-y"],
        b"border" => &[
            "border-top-width",
            "border-right-width",
            "border-bottom-width",
            "border-left-width",
            "border-top-style",
            "border-right-style",
            "border-bottom-style",
            "border-left-style",
            "border-top-color",
            "border-right-color",
            "border-bottom-color",
            "border-left-color",
            "border-width",
            "border-style",
            "border-color",
            "border-top",
            "border-right",
            "border-bottom",
            "border-left",
        ],
        b"border-top" => &["border-top-width", "border-top-style", "border-top-color"],
        b"border-right" => &["border-right-width", "border-right-style", "border-right-color"],
        b"border-bottom" => &["border-bottom-width", "border-bottom-style", "border-bottom-color"],
        b"border-left" => &["border-left-width", "border-left-style", "border-left-color"],
        b"border-width" => &["border-top-width", "border-right-width", "border-bottom-width", "border-left-width"],
        b"border-style" => &["border-top-style", "border-right-style", "border-bottom-style", "border-left-style"],
        b"border-color" => &["border-top-color", "border-right-color", "border-bottom-color", "border-left-color"],
        b"border-block" => &[
            "border-block-start",
            "border-block-end",
            "border-block-width",
            "border-block-style",
            "border-block-color",
        ],
        b"border-inline" => &[
            "border-inline-start",
            "border-inline-end",
            "border-inline-width",
            "border-inline-style",
            "border-inline-color",
        ],
        b"border-block-start" => &["border-block-start-width", "border-block-start-style", "border-block-start-color"],
        b"border-block-end" => &["border-block-end-width", "border-block-end-style", "border-block-end-color"],
        b"border-inline-start" => {
            &["border-inline-start-width", "border-inline-start-style", "border-inline-start-color"]
        }
        b"border-inline-end" => &["border-inline-end-width", "border-inline-end-style", "border-inline-end-color"],
        b"border-radius" => &[
            "border-top-left-radius",
            "border-top-right-radius",
            "border-bottom-right-radius",
            "border-bottom-left-radius",
        ],
        b"border-image" => &[
            "border-image-source",
            "border-image-slice",
            "border-image-width",
            "border-image-outset",
            "border-image-repeat",
        ],
        b"outline" => &["outline-color", "outline-style", "outline-width"],
        b"font" => {
            &["font-style", "font-variant", "font-weight", "font-stretch", "font-size", "line-height", "font-family"]
        }
        b"list-style" => &["list-style-type", "list-style-position", "list-style-image"],
        b"flex" => &["flex-grow", "flex-shrink", "flex-basis"],
        b"flex-flow" => &["flex-direction", "flex-wrap"],
        b"grid" => &[
            "grid-template-rows",
            "grid-template-columns",
            "grid-template-areas",
            "grid-auto-rows",
            "grid-auto-columns",
            "grid-auto-flow",
        ],
        b"grid-template" => &["grid-template-rows", "grid-template-columns", "grid-template-areas"],
        b"grid-area" => &["grid-row-start", "grid-column-start", "grid-row-end", "grid-column-end"],
        b"grid-row" => &["grid-row-start", "grid-row-end"],
        b"grid-column" => &["grid-column-start", "grid-column-end"],
        b"gap" => &["row-gap", "column-gap"],
        b"grid-gap" => &["grid-row-gap", "grid-column-gap"],
        b"place-content" => &["align-content", "justify-content"],
        b"place-items" => &["align-items", "justify-items"],
        b"place-self" => &["align-self", "justify-self"],
        b"transition" => {
            &["transition-property", "transition-duration", "transition-timing-function", "transition-delay"]
        }
        b"animation" => &[
            "animation-name",
            "animation-duration",
            "animation-timing-function",
            "animation-delay",
            "animation-iteration-count",
            "animation-direction",
            "animation-fill-mode",
            "animation-play-state",
        ],
        b"columns" => &["column-width", "column-count"],
        b"column-rule" => &["column-rule-width", "column-rule-style", "column-rule-color"],
        b"text-decoration" => &["text-decoration-line", "text-decoration-style", "text-decoration-color"],
        b"text-emphasis" => &["text-emphasis-style", "text-emphasis-color"],
        b"mask" => &[
            "mask-image",
            "mask-mode",
            "mask-repeat",
            "mask-position",
            "mask-clip",
            "mask-origin",
            "mask-size",
            "mask-composite",
        ],
        b"overflow" => &["overflow-x", "overflow-y"],
        b"overscroll-behavior" => &["overscroll-behavior-x", "overscroll-behavior-y"],
        b"offset" => &["offset-position", "offset-path", "offset-distance", "offset-rotate", "offset-anchor"],
        b"container" => &["container-name", "container-type"],
        b"scroll-margin" => &["scroll-margin-top", "scroll-margin-right", "scroll-margin-bottom", "scroll-margin-left"],
        b"scroll-padding" => {
            &["scroll-padding-top", "scroll-padding-right", "scroll-padding-bottom", "scroll-padding-left"]
        }
        _ => &[],
    }
}

fn is_math_fn(name: &[u8]) -> bool {
    matches!(
        name,
        b"calc"
            | b"-webkit-calc"
            | b"-moz-calc"
            | b"min"
            | b"max"
            | b"clamp"
            | b"round"
            | b"mod"
            | b"rem"
            | b"abs"
            | b"sign"
            | b"sin"
            | b"cos"
            | b"tan"
            | b"asin"
            | b"acos"
            | b"atan"
            | b"atan2"
            | b"pow"
            | b"sqrt"
            | b"hypot"
            | b"log"
            | b"exp"
    )
}

/// Pseudo-classes and pseudo-elements whose arguments are selectors.
fn selector_args(name: &[u8]) -> bool {
    matches!(
        name,
        b"not"
            | b"is"
            | b"where"
            | b"has"
            | b"matches"
            | b"any"
            | b"-webkit-any"
            | b"-moz-any"
            | b"host"
            | b"host-context"
            | b"global"
            | b"local"
            | b"deep"
            | b"slotted"
            | b"v-deep"
            | b"v-global"
            | b"v-slotted"
            | b"current"
            | b"past"
            | b"future"
    )
}

impl<'a> Parser<'a> {
    fn rep(&mut self, key: &'static str, s: usize, e: usize, msg: String) {
        self.out.push(Raw { key, start: s as u32, end: e as u32, msg });
    }

    fn text(&self, s: usize, e: usize) -> &'a str {
        self.s.get(s..e).unwrap_or("")
    }

    // ---- structure --------------------------------------------------------------------------

    /// Parses statements into frame `depth` until its `}` (consumed; returns true) or EOF.
    fn block(&mut self, depth: usize, kind: Kind) -> bool {
        if depth > MAX_DEPTH {
            // Pathological nesting: skip the block.
            let mut d = 1u32;
            while self.i < self.n {
                match self.b[self.i] {
                    b'{' => d += 1,
                    b'}' => {
                        d -= 1;
                        if d == 0 {
                            self.i += 1;
                            return true;
                        }
                    }
                    _ => {}
                }
                self.i += 1;
            }
            return false;
        }
        if self.frames.len() <= depth {
            self.frames.push(Frame::default());
        }
        {
            let f = &mut self.frames[depth];
            f.kind = kind;
            f.decls.clear();
            f.sels.clear();
            f.nodes = 0;
        }
        loop {
            self.trivia(depth);
            if self.i >= self.n {
                return false;
            }
            let c = self.b[self.i];
            match c {
                b'}' => {
                    self.i += 1;
                    if depth > 0 {
                        return true;
                    }
                }
                b';' => {
                    if self.c.s1116 {
                        self.rep("S1116", self.i, self.i + 1, "Unexpected extra semicolon".into());
                    }
                    self.i += 1;
                }
                _ => {
                    self.frames[depth].nodes += 1;
                    let at = c == b'@' && self.b.get(self.i + 1).is_some_and(|&c| c != b'{');
                    if at {
                        self.at_rule(depth);
                    } else {
                        self.statement(depth);
                    }
                }
            }
        }
    }

    /// Skips whitespace and comments at a statement start.
    fn trivia(&mut self, depth: usize) {
        let b = self.b;
        let n = self.n;
        loop {
            while self.i < n && is_ws(b[self.i]) {
                self.i += 1;
            }
            if self.i + 1 >= n || b[self.i] != b'/' {
                return;
            }
            match b[self.i + 1] {
                b'*' => {
                    let e = comment_end(b, self.i, n);
                    self.comment(self.i, e);
                    self.frames[depth].nodes += 1;
                    self.i = e;
                }
                b'/' => {
                    if self.d == Dialect::Css && self.c.s4668 {
                        self.rep("S4668", self.i, self.i + 2, "Unexpected double-slash CSS comment".into());
                    }
                    self.frames[depth].nodes += 1;
                    self.i = eol(b, self.i, n);
                }
                _ => return,
            }
        }
    }

    fn comment(&mut self, s: usize, e: usize) {
        if self.c.s4663 && e >= s + 4 && self.b[e - 2..e] == *b"*/" && self.b[s + 2..e - 2].iter().all(|&c| is_ws(c)) {
            self.rep("S4663", s, e, "Unexpected empty comment".into());
        }
    }

    /// A string at `i`; returns the index after it. Reports raw newlines (S4652).
    fn string(&mut self, i: usize) -> usize {
        let b = self.b;
        let q = b[i];
        let mut j = i + 1;
        let mut nl = NONE;
        while j < self.n {
            let c = b[j];
            if c == b'\\' {
                j += 2;
                continue;
            }
            if c == q {
                if nl != NONE && self.c.s4652 {
                    self.rep("S4652", i, j + 1, "Unexpected newline in string".into());
                }
                return j + 1;
            }
            if c == b'\n' {
                if nl == NONE {
                    nl = j;
                }
            } else if nl != NONE && matches!(c, b';' | b'{' | b'}') {
                // Not a multi-line string, just an unterminated one: stop at the line end.
                return nl;
            }
            j += 1;
        }
        if nl != NONE { nl } else { self.n }
    }

    /// `url(` contents starting at `j` (after the parenthesis); returns the index after `)`.
    fn url(&mut self, mut j: usize) -> usize {
        let b = self.b;
        while j < self.n {
            match b[j] {
                b'"' | b'\'' => {
                    j = self.string(j);
                    continue;
                }
                b')' => return j + 1,
                b'\\' => j += 1,
                b'#' if b.get(j + 1) == Some(&b'{') => {
                    j = skip_braces(b, j + 1, self.n);
                    continue;
                }
                b'\n' | b'{' | b'}' => return j,
                _ => {}
            }
            j += 1;
        }
        self.n
    }

    /// Scans a statement from `from` to its terminator.
    fn scan(&mut self, from: usize) -> End {
        let b = self.b;
        let n = self.n;
        let less = self.d == Dialect::Less;
        let css = self.d == Dialect::Css;
        let mut i = from;
        let mut paren = 0u32;
        let mut brack = 0u32;
        let mut colon = NONE;
        while i < n {
            match b[i] {
                b'"' | b'\'' => {
                    i = self.string(i);
                    continue;
                }
                b'/' if i + 1 < n && b[i + 1] == b'*' => {
                    let e = comment_end(b, i, n);
                    self.comment(i, e);
                    i = e;
                    continue;
                }
                b'/' if !css && i + 1 < n && b[i + 1] == b'/' => {
                    i = eol(b, i, n);
                    continue;
                }
                b'\\' => {
                    i += 2;
                    continue;
                }
                b'#' | b'@' if i + 1 < n && b[i + 1] == b'{' => {
                    i = skip_braces(b, i + 1, n);
                    continue;
                }
                b'(' => paren += 1,
                b')' => paren = paren.saturating_sub(1),
                b'[' => brack += 1,
                b']' => brack = brack.saturating_sub(1),
                b'u' | b'U' if is_url(b, i) => {
                    i = self.url(i + 4);
                    continue;
                }
                b':' => {
                    if paren == 0 && brack == 0 && colon == NONE {
                        colon = i;
                    }
                }
                b'{' => {
                    if less && paren > 0 {
                        i = skip_braces(b, i, n);
                        continue;
                    }
                    return End { pos: i, term: b'{', colon };
                }
                b';' => {
                    if !(less && paren > 0) {
                        return End { pos: i, term: b';', colon };
                    }
                }
                b'}' => return End { pos: i, term: b'}', colon },
                _ => {}
            }
            i += 1;
        }
        End { pos: n, term: 0, colon }
    }

    fn after(e: End) -> usize {
        if e.term == b';' { e.pos + 1 } else { e.pos }
    }

    /// A rule, declaration or other statement starting at `self.i`.
    fn statement(&mut self, depth: usize) {
        let start = self.i;
        let end = self.scan(start);
        if end.term == b'{' {
            let b = self.b;
            if b[start..].starts_with(b"--") && end.colon != NONE {
                // Custom property with a block value (`--mixin: { ... }`).
                self.i = skip_braces(b, end.pos, self.n);
                let rest = self.scan(self.i);
                self.i = Self::after(rest);
                return;
            }
            if self.d == Dialect::Scss && self.nested_prop(start, end.pos) {
                self.i = end.pos + 1;
                self.block(depth + 1, Kind::NestedProps);
                return;
            }
            self.rule(depth, start, end.pos);
            return;
        }
        if end.colon != NONE {
            self.decl(depth, start, end.colon, end.pos);
        }
        self.i = Self::after(end);
    }

    /// SCSS nested properties: `font: { ... }` or `margin: 0 { ... }`.
    fn nested_prop(&self, s: usize, e: usize) -> bool {
        let b = self.b;
        let Some(k) = b[s..e].iter().position(|&c| c == b':') else { return false };
        let name = &b[s..s + k];
        let next = b.get(s + k + 1).copied().unwrap_or(b' ');
        if !(is_ws(next) || next == b'{') || name.is_empty() {
            return false;
        }
        let mut buf = [0u8; 64];
        lower(name, &mut buf).is_some_and(|l| has(PROPERTIES, l))
    }

    fn rule(&mut self, depth: usize, start: usize, brace: usize) {
        let (ss, se) = trim(self.b, start, brace);
        let parent = self.frames[depth].kind;
        let sel = &self.b[ss..se];
        let kind = match parent {
            Kind::Keyframes => Kind::KeyframeSel,
            Kind::NestedProps => Kind::NestedProps,
            _ if sel == b":export" || sel.starts_with(b":import") => Kind::Export,
            _ => Kind::Rule,
        };
        if !matches!(parent, Kind::Keyframes | Kind::NestedProps) {
            self.selector(ss, se);
            self.dup_selector(depth, ss, se);
        }
        self.i = brace + 1;
        let closed = self.block(depth + 1, kind);
        if closed && self.c.s4658 && self.frames[depth + 1].nodes == 0 {
            self.rep("S4658", brace, brace + 1, "Unexpected empty block".into());
        }
    }

    fn at_rule(&mut self, depth: usize) {
        let b = self.b;
        let start = self.i;
        let ns = start + 1;
        let ne = ident_end(b, ns, self.n);
        if ne == ns {
            // `@@var`, `@(`...: not an at-rule.
            self.statement(depth);
            return;
        }
        if self.d == Dialect::Less {
            match b.get(ne) {
                Some(b':') => {
                    // Variable, or detached ruleset / map `@r: { ... }` (map keys are arbitrary).
                    let end = self.scan(ne + 1);
                    if end.term == b'{' {
                        self.i = end.pos + 1;
                        self.block(depth + 1, Kind::Opaque);
                        let mut j = self.i;
                        while j < self.n && is_ws(b[j]) {
                            j += 1;
                        }
                        if j < self.n && b[j] == b';' {
                            self.i = j + 1;
                        }
                    } else {
                        self.value(ne + 1, end.pos, b"", false);
                        self.i = Self::after(end);
                    }
                    return;
                }
                Some(b'(') => {
                    // Detached ruleset call `@r();`.
                    let end = self.scan(ne);
                    self.i = Self::after(end);
                    return;
                }
                _ => {}
            }
        }
        let end = self.scan(ne);
        let name = &b[ns..ne];
        let mut buf = [0u8; 64];
        let lname: &[u8] = lower(name, &mut buf).unwrap_or(b"");
        let known = has(AT_RULES, lname);
        if self.c.s4662 && !vendor(name) && !lname.is_empty() && !known && !listed(&self.c.ignore_at, lname) {
            self.rep("S4662", start, ne, format!("Unexpected unknown at-rule \"@{}\"", self.text(ns, ne)));
        }
        match lname {
            b"media" => self.media(ne, end.pos),
            b"import" => self.import(start, ne, end.pos),
            _ => {}
        }
        if end.term == b'{' {
            let kind = if lname.ends_with(b"keyframes") {
                Kind::Keyframes
            } else if lname == b"font-face" {
                Kind::FontFace
            } else if (!known && !vendor(name))
                || matches!(
                    lname,
                    b"font-feature-values"
                        | b"swash"
                        | b"ornaments"
                        | b"annotation"
                        | b"stylistic"
                        | b"styleset"
                        | b"character-variant"
                        | b"historical-forms"
                        | b"function"
                        | b"return"
                        | b"plugin"
                        | b"config"
                )
            {
                Kind::Opaque
            } else if self.frames[depth].kind == Kind::NestedProps {
                Kind::NestedProps
            } else {
                Kind::AtRule
            };
            let brace = end.pos;
            self.i = brace + 1;
            let closed = self.block(depth + 1, kind);
            if closed && self.c.s4658 && self.frames[depth + 1].nodes == 0 {
                self.rep("S4658", brace, brace + 1, "Unexpected empty block".into());
            }
        } else {
            self.i = Self::after(end);
        }
    }

    // ---- at-rule preludes -------------------------------------------------------------------

    fn import(&mut self, start: usize, s: usize, e: usize) {
        if !self.c.s1128 {
            return;
        }
        let b = self.b;
        let (s, e) = trim(b, s, e);
        let p = &b[s..e];
        if p.is_empty() || has_interp(p) {
            return;
        }
        let mut h = FxHasher::default();
        let mut url = (NONE, NONE);
        let mut i = s;
        while i < e {
            let c = b[i];
            if is_ws(c) {
                i += 1;
                continue;
            }
            if c == b'"' || c == b'\'' {
                let j = skip_str(b, i, e);
                if url.0 == NONE {
                    url = (i + 1, j.saturating_sub(1).max(i + 1));
                }
                h.write(&b[i + 1..j.saturating_sub(1).max(i + 1)]);
                i = j;
                continue;
            }
            if is_url(b, i) {
                let close = matching(b, i + 3, e, b'(', b')');
                let (us, ue) = trim(b, i + 4, close);
                let (us, ue) = if ue > us + 1 && matches!(b[us], b'"' | b'\'') { (us + 1, ue - 1) } else { (us, ue) };
                if url.0 == NONE {
                    url = (us, ue);
                }
                h.write(&b[us..ue]);
                i = close + 1;
                continue;
            }
            h.write_u8(c.to_ascii_lowercase());
            i += 1;
        }
        let key = h.finish();
        if self.imports.contains(&key) {
            let u = if url.0 != NONE { self.text(url.0, url.1) } else { self.text(s, e) };
            self.rep("S1128", start, e, format!("Unexpected duplicate @import rule {u}"));
        } else {
            self.imports.push(key);
        }
    }

    fn media(&mut self, s: usize, e: usize) {
        let b = self.b;
        if self.c.s4653 {
            self.value(s, e, b"", true);
        }
        if !self.c.s4661 {
            return;
        }
        let mut i = s;
        while i < e {
            match b[i] {
                b'"' | b'\'' => {
                    i = skip_str(b, i, e);
                    continue;
                }
                b'#' | b'@' if b.get(i + 1) == Some(&b'{') => {
                    i = skip_braces(b, i + 1, e);
                    continue;
                }
                b'(' => {
                    let mut k = i + 1;
                    while k < e && is_ws(b[k]) {
                        k += 1;
                    }
                    if k < e && (b[k].is_ascii_digit() || b[k] == b'.') {
                        // Range form `(400px <= width ...)`: the name follows the operator.
                        while k < e && !matches!(b[k], b'<' | b'>' | b'=' | b')' | b'(') {
                            k += 1;
                        }
                        while k < e && matches!(b[k], b'<' | b'>' | b'=') {
                            k += 1;
                        }
                        while k < e && is_ws(b[k]) {
                            k += 1;
                        }
                        let ke = ident_end(b, k, e);
                        if ke > k && b[k].is_ascii_alphabetic() {
                            self.media_feature(k, ke);
                        }
                    } else if k < e && b[k].is_ascii_alphabetic() {
                        let ke = ident_end(b, k, e);
                        let mut m = ke;
                        while m < e && is_ws(b[m]) {
                            m += 1;
                        }
                        if m < e && matches!(b[m], b':' | b')' | b'<' | b'>' | b'=') {
                            self.media_feature(k, ke);
                        }
                    }
                }
                _ => {}
            }
            i += 1;
        }
    }

    fn media_feature(&mut self, s: usize, e: usize) {
        let name = &self.b[s..e];
        if contains(name, b"\\") || contains(name, b"--") {
            return;
        }
        let mut buf = [0u8; 64];
        let Some(l) = lower(name, &mut buf) else { return };
        if has(MEDIA_FEATURES, l) || matches!(l, b"and" | b"or" | b"not" | b"only") {
            return;
        }
        let msg = format!("Unexpected unknown media feature name \"{}\"", self.text(s, e));
        self.rep("S4661", s, e, msg);
    }

    // ---- declarations -----------------------------------------------------------------------

    fn decl(&mut self, depth: usize, start: usize, colon: usize, end: usize) {
        let b = self.b;
        let (ps, mut pe) = trim(b, start, colon);
        if ps == pe {
            return;
        }
        let (vs, ve0) = trim(b, colon + 1, end);
        let (ve, imp) = strip_important(b, vs, ve0);
        let kind = self.frames[depth].kind;
        if let Some(bang) = imp
            && kind == Kind::KeyframeSel
            && self.c.s4655
        {
            self.rep("S4655", bang, ve0, "Unexpected !important".into());
        }
        if self.d == Dialect::Less {
            if b[..pe].ends_with(b"+_") {
                pe -= 2;
            } else if b[..pe].ends_with(b"+") {
                pe -= 1;
            }
        }
        let prop = &b[ps..pe];
        let custom = prop.starts_with(b"--");
        let var = custom || prop[0] == b'$' || prop[0] == b'@';
        // `*zoom` / `_height` are IE hacks, not properties.
        let standard = !var && prop[0] != b'_' && !has_interp(prop) && prop.iter().all(|&c| is_ident_char(c));
        let mut buf = [0u8; 64];
        let lprop: &[u8] = lower(prop, &mut buf).unwrap_or(b"");
        if kind == Kind::NestedProps || kind == Kind::Export {
            return;
        }
        if standard && !lprop.is_empty() && !matches!(kind, Kind::Root | Kind::Opaque) {
            if self.c.s4654
                && !vendor(lprop)
                && !lprop.starts_with(b"mso-")
                && !has(PROPERTIES, lprop)
                && !listed(&self.c.ignore_props, lprop)
            {
                self.rep("S4654", ps, pe, format!("Unexpected unknown property \"{}\"", self.text(ps, pe)));
            }
            if lprop != b"composes" {
                self.dup_decl(depth, Decl { ps, pe, vs, ve }, lprop);
            }
        }
        if !custom && !lprop.is_empty() {
            if lprop == b"font-family" || lprop == b"font" {
                self.font(vs, ve, lprop, kind == Kind::FontFace);
            }
            self.value(vs, ve, lprop, false);
        } else if !custom && var {
            self.value(vs, ve, b"", false);
        }
    }

    fn dup_decl(&mut self, depth: usize, d: Decl, lprop: &[u8]) {
        let b = self.b;
        let c = self.c;
        let mut hits: Vec<(&'static str, String)> = Vec::new();
        {
            let decls = &self.frames[depth].decls;
            if c.s4656
                && let Some(k) = decls.iter().rposition(|p| b[p.ps..p.pe].eq_ignore_ascii_case(lprop))
            {
                let prev = decls[k];
                let same = b[prev.vs..prev.ve] == b[d.vs..d.ve];
                if !(c.fallbacks && k + 1 == decls.len() && !same) {
                    hits.push(("S4656", format!("Unexpected duplicate \"{}\"", self.text(d.ps, d.pe))));
                }
            }
            if c.s4657 {
                let (prefix, base) = unprefix(lprop);
                for long in longhands(base) {
                    let found = decls.iter().find(|p| {
                        let q = &b[p.ps..p.pe];
                        q.len() == prefix.len() + long.len()
                            && q[..prefix.len()].eq_ignore_ascii_case(prefix)
                            && q[prefix.len()..].eq_ignore_ascii_case(long.as_bytes())
                    });
                    if let Some(p) = found {
                        hits.push((
                            "S4657",
                            format!(
                                "Unexpected shorthand \"{}\" after \"{}\"",
                                self.text(d.ps, d.pe),
                                self.text(p.ps, p.pe)
                            ),
                        ));
                    }
                }
            }
        }
        for (k, m) in hits {
            self.rep(k, d.ps, d.pe, m);
        }
        self.frames[depth].decls.push(d);
    }

    // ---- values -----------------------------------------------------------------------------

    /// Walks a value (or a media query prelude when `media`): hex colors, units, calc and
    /// gradients.
    fn value(&mut self, vs: usize, ve: usize, lprop: &[u8], media: bool) {
        let c = self.c;
        if !(c.s4647 || c.s4653 || c.s4650 || c.s5362 || c.s4651) {
            return;
        }
        let b = self.b;
        let v = &b[vs..ve];
        if v.len() >= 7 && v[..7].eq_ignore_ascii_case(b"progid:") || contains(v, b"expression(") {
            return;
        }
        let units = c.s4653 && lprop != b"unicode-range";
        let hex = c.s4647 && !media;
        let mut i = vs;
        while i < ve {
            let ch = b[i];
            match ch {
                b'"' | b'\'' => {
                    i = skip_str(b, i, ve);
                    continue;
                }
                b'/' if i + 1 < ve && b[i + 1] == b'*' => {
                    i = comment_end(b, i, ve);
                    continue;
                }
                b'/' if self.d != Dialect::Css && i + 1 < ve && b[i + 1] == b'/' => {
                    i = eol(b, i, ve);
                    continue;
                }
                b'\\' => {
                    i += 2;
                    continue;
                }
                b'#' => {
                    if b.get(i + 1) == Some(&b'{') {
                        i = skip_braces(b, i + 1, ve);
                        continue;
                    }
                    let mut j = i + 1;
                    while j < ve && b[j].is_ascii_alphanumeric() {
                        j += 1;
                    }
                    let clean = j == ve || !(is_ident_char(b[j]) || b[j] == b'\\' || b[j] == b'(');
                    if hex && j > i + 1 && clean {
                        let w = &b[i + 1..j];
                        if !matches!(w.len(), 3 | 4 | 6 | 8) || !w.iter().all(|c| c.is_ascii_hexdigit()) {
                            self.rep("S4647", i, j, format!("Unexpected invalid hex color \"{}\"", self.text(i, j)));
                        }
                    }
                    i = ident_end(b, j, ve);
                    continue;
                }
                b'$' | b'@' => {
                    if b.get(i + 1) == Some(&b'{') {
                        i = skip_braces(b, i + 1, ve);
                    } else {
                        i = ident_end(b, i + 1, ve);
                    }
                    continue;
                }
                b'0'..=b'9' | b'.' | b'+' | b'-' if self.num_start(i, vs, ve) => {
                    i = self.number(i, ve, units);
                    continue;
                }
                _ if is_ident_start(ch) => {
                    let mut j = ident_end(b, i, ve);
                    if j == i + 1 && (ch | 0x20) == b'u' && j < ve && b[j] == b'+' {
                        // Unicode range `U+0025-00FF`, `U+4??`.
                        j += 1;
                        while j < ve && (b[j].is_ascii_hexdigit() || b[j] == b'?' || b[j] == b'-') {
                            j += 1;
                        }
                        i = j;
                        continue;
                    }
                    if j < ve && b[j] == b'(' {
                        let close = matching(b, j, ve, b'(', b')');
                        let mut buf = [0u8; 64];
                        let name: &[u8] = lower(&b[i..j], &mut buf).unwrap_or(b"");
                        if name == b"url" {
                            i = (close + 1).min(ve);
                            continue;
                        }
                        if matches!(name, b"calc" | b"-webkit-calc" | b"-moz-calc") && (c.s4650 || c.s5362) {
                            self.calc(i, j + 1, close);
                        } else if matches!(name, b"linear-gradient" | b"repeating-linear-gradient") && c.s4651 {
                            self.gradient(j + 1, close);
                        }
                        i = j + 1;
                    } else {
                        i = j.max(i + 1);
                    }
                    continue;
                }
                _ => {}
            }
            i += 1;
        }
    }

    fn num_start(&self, i: usize, vs: usize, ve: usize) -> bool {
        let b = self.b;
        if i > vs {
            let p = b[i - 1];
            if p.is_ascii_alphanumeric() || matches!(p, b'_' | b'\\' | b'#' | b'$' | b'@' | b'.' | b'%') || p >= 0x80 {
                return false;
            }
        }
        let d = |k: usize| k < ve && b[k].is_ascii_digit();
        match b[i] {
            b'0'..=b'9' => true,
            b'.' => d(i + 1),
            _ => d(i + 1) || (i + 2 < ve && b[i + 1] == b'.' && d(i + 2)),
        }
    }

    /// Consumes a number and its unit; checks the unit.
    fn number(&mut self, i: usize, ve: usize, units: bool) -> usize {
        let b = self.b;
        let mut j = i;
        if matches!(b[j], b'+' | b'-') {
            j += 1;
        }
        while j < ve && b[j].is_ascii_digit() {
            j += 1;
        }
        if j + 1 < ve && b[j] == b'.' && b[j + 1].is_ascii_digit() {
            j += 1;
            while j < ve && b[j].is_ascii_digit() {
                j += 1;
            }
        }
        if j < ve && matches!(b[j], b'e' | b'E') {
            let k = if j + 1 < ve && matches!(b[j + 1], b'+' | b'-') { j + 2 } else { j + 1 };
            if k < ve && b[k].is_ascii_digit() {
                j = k;
                while j < ve && b[j].is_ascii_digit() {
                    j += 1;
                }
            }
        }
        if j < ve && b[j] == b'%' {
            return j + 1;
        }
        let us = j;
        while j < ve && b[j].is_ascii_alphabetic() {
            j += 1;
        }
        if j == us {
            return j;
        }
        // `10px-5px`, `2x3`, `1em\9`: ambiguous or hacks; skip the whole word.
        if j < ve && (is_ident_char(b[j]) || b[j] == b'\\' || b[j] == b'(') {
            return ident_end(b, j, ve);
        }
        if units {
            let mut buf = [0u8; 64];
            if let Some(u) = lower(&b[us..j], &mut buf)
                && !has(UNITS, u)
            {
                self.rep("S4653", i, j, format!("Unexpected unknown unit \"{}\"", self.text(us, j)));
            }
        }
        j
    }

    /// `calc()` arguments `[s, e)`: unspaced operators (S4650) and invalid expressions (S5362).
    fn calc(&mut self, fs: usize, s: usize, e: usize) {
        let b = self.b;
        let args = &b[s..e];
        if args.iter().any(|&c| matches!(c, b'$' | b'@' | b'~' | b'#' | b'"' | b'\'' | b'\\' | b'{' | b'`'))
            || contains(args, b"/*")
            || contains(args, b"//")
        {
            return;
        }
        #[derive(Clone, Copy, PartialEq)]
        enum Prev {
            Start,
            Operand,
            Op,
        }
        let mut prev = Prev::Start;
        // Stack of group starts; each entry is whether the group is non-empty.
        let mut groups: Vec<bool> = vec![false];
        let mut invalid: Option<(usize, &'static str)> = None;
        let mut spacing: Vec<(usize, String)> = Vec::new();
        let mut last_op = 0u8;
        let mut i = s;
        while i < e {
            let c = b[i];
            if is_ws(c) {
                i += 1;
                continue;
            }
            match c {
                b'(' => {
                    if prev == Prev::Operand {
                        return;
                    }
                    groups.push(false);
                    prev = Prev::Start;
                    i += 1;
                }
                b')' => {
                    let non_empty = groups.pop().unwrap_or(true);
                    if !non_empty || prev == Prev::Op {
                        invalid.get_or_insert((i, "Expected a valid expression"));
                    }
                    if groups.is_empty() {
                        return;
                    }
                    *groups.last_mut().unwrap() = true;
                    prev = Prev::Operand;
                    i += 1;
                }
                b',' => {
                    if prev != Prev::Operand {
                        invalid.get_or_insert((i, "Expected a valid expression"));
                    }
                    prev = Prev::Start;
                    i += 1;
                }
                b'*' | b'/' => {
                    if prev != Prev::Operand {
                        invalid.get_or_insert((i, "Expected a valid expression"));
                    }
                    prev = Prev::Op;
                    last_op = c;
                    i += 1;
                }
                b'+' | b'-' if prev == Prev::Operand => {
                    let before = is_ws(b[i - 1]);
                    let after = i + 1 < e && is_ws(b[i + 1]);
                    let op = c as char;
                    if !before {
                        spacing.push((i, format!("Expected single space before \"{op}\" operator")));
                    } else if !after {
                        spacing.push((i, format!("Expected single space after \"{op}\" operator")));
                    }
                    prev = Prev::Op;
                    last_op = c;
                    i += 1;
                }
                b'0'..=b'9' | b'.' | b'+' | b'-'
                    if {
                        let d = |k: usize| k < e && b[k].is_ascii_digit();
                        c.is_ascii_digit() || d(i + 1) || (i + 2 < e && b[i + 1] == b'.' && d(i + 2))
                    } =>
                {
                    if prev == Prev::Operand {
                        invalid.get_or_insert((i, "Expected a valid expression"));
                    }
                    let ns = i;
                    let mut j = i;
                    if matches!(b[j], b'+' | b'-') {
                        j += 1;
                    }
                    while j < e && (b[j].is_ascii_digit() || b[j] == b'.') {
                        j += 1;
                    }
                    let digits = &b[ns..j];
                    if j < e && matches!(b[j], b'e' | b'E') && j + 1 < e && b[j + 1].is_ascii_digit() {
                        j += 1;
                        while j < e && b[j].is_ascii_digit() {
                            j += 1;
                        }
                    }
                    if j < e && b[j] == b'%' {
                        j += 1;
                    } else {
                        while j < e && b[j].is_ascii_alphabetic() {
                            j += 1;
                        }
                        if j < e && (is_ident_char(b[j]) || b[j] == b'(') {
                            return; // `10px-5px` and similar: not tokenized like CSS here.
                        }
                    }
                    let zero = digits.iter().all(|&d| matches!(d, b'0' | b'.' | b'+' | b'-'));
                    if prev == Prev::Op && last_op == b'/' && zero {
                        invalid.get_or_insert((ns, "Unexpected division by zero"));
                    }
                    *groups.last_mut().unwrap() = true;
                    prev = Prev::Operand;
                    i = j;
                }
                _ if is_ident_start(c) => {
                    if prev == Prev::Operand {
                        invalid.get_or_insert((i, "Expected a valid expression"));
                    }
                    let j = ident_end(b, i, e);
                    if j < e && b[j] == b'(' {
                        let close = matching(b, j, e, b'(', b')');
                        let mut buf = [0u8; 64];
                        let name = lower(&b[i..j], &mut buf).unwrap_or(b"");
                        if is_math_fn(name) {
                            groups.push(false);
                            prev = Prev::Start;
                            i = j + 1;
                            continue;
                        }
                        i = (close + 1).min(e);
                    } else {
                        i = j.max(i + 1);
                    }
                    *groups.last_mut().unwrap() = true;
                    prev = Prev::Operand;
                }
                _ => return,
            }
        }
        if groups.len() != 1 {
            return;
        }
        if !groups[0] || prev == Prev::Op {
            invalid.get_or_insert((e, "Expected a valid expression"));
        }
        if self.c.s4650 {
            for (p, m) in spacing {
                self.rep("S4650", p, p + 1, m);
            }
        }
        if self.c.s5362
            && let Some((p, m)) = invalid
        {
            let _ = p;
            self.rep("S5362", fs, (e + 1).min(self.n), m.into());
        }
    }

    /// First argument of `linear-gradient()` `[s, e)`.
    fn gradient(&mut self, s: usize, e: usize) {
        let b = self.b;
        let mut parts = std::mem::take(&mut self.parts);
        split_commas(b, s, e, &mut parts);
        let (fs, fe) = trim(b, parts[0].0, parts[0].1);
        self.parts = parts;
        let arg = &b[fs..fe];
        if arg.is_empty() || arg.iter().any(|&c| matches!(c, b'$' | b'@' | b'#' | b'(' | b'~' | b'\\' | b'/')) {
            return;
        }
        let mut words: Vec<&[u8]> = arg.split(|&c| is_ws(c)).filter(|w| !w.is_empty()).collect();
        if let Some(k) = words.iter().position(|w| w.eq_ignore_ascii_case(b"in")) {
            words.truncate(k); // color interpolation method
        }
        if words.is_empty() {
            return;
        }
        let side = |w: &[u8]| SIDES.iter().any(|s| w.eq_ignore_ascii_case(s));
        let w0 = words[0];
        let numeric = w0[0].is_ascii_digit()
            || (w0.len() > 1 && matches!(w0[0], b'.' | b'-' | b'+') && (w0[1].is_ascii_digit() || w0[1] == b'.'));
        let bad = if numeric {
            let k = w0.iter().position(|c| c.is_ascii_alphabetic() || *c == b'%').unwrap_or(w0.len());
            let unit = &w0[k..];
            let zero = w0[..k].iter().all(|&c| matches!(c, b'0' | b'.' | b'-' | b'+'));
            words.len() != 1
                || !(["deg", "grad", "rad", "turn"].iter().any(|u| unit.eq_ignore_ascii_case(u.as_bytes()))
                    || (unit.is_empty() && zero))
        } else if w0.eq_ignore_ascii_case(b"to") {
            let rest = &words[1..];
            let vertical = |w: &[u8]| w.eq_ignore_ascii_case(b"top") || w.eq_ignore_ascii_case(b"bottom");
            match rest {
                [a] => !side(a),
                [a, c] => !(side(a) && side(c) && vertical(a) != vertical(c)),
                _ => true,
            }
        } else {
            words.iter().all(|w| side(w))
        };
        if bad {
            self.rep("S4651", fs, fe, "Unexpected nonstandard direction".into());
        }
    }

    /// `font-family` / `font` values: duplicate names (S4648), missing generic family (S4649).
    fn font(&mut self, vs: usize, ve: usize, lprop: &[u8], in_font_face: bool) {
        if !(self.c.s4648 || self.c.s4649) {
            return;
        }
        let b = self.b;
        let v = &b[vs..ve];
        let family = lprop == b"font-family";
        if v.is_empty()
            || v.iter().any(|&c| {
                matches!(c, b'$' | b'@' | b'(' | b'~' | b'\\' | b'#' | b'{' | b'`')
                    || (family && matches!(c, b'/' | b'+' | b'*'))
            })
        {
            return;
        }
        let mut parts = std::mem::take(&mut self.parts);
        split_commas(b, vs, ve, &mut parts);
        let mut fams: Vec<(usize, usize)> = Vec::with_capacity(parts.len());
        for (k, &(s, e)) in parts.iter().enumerate() {
            let (mut s, e) = trim(b, s, e);
            if k == 0 && lprop == b"font" {
                // Skip style/weight/size[/line-height]: the family starts after the size.
                let mut last = NONE;
                let mut i = s;
                while i < e {
                    if is_ws(b[i]) {
                        i += 1;
                        continue;
                    }
                    let ts = i;
                    if matches!(b[i], b'"' | b'\'') {
                        i = skip_str(b, i, e);
                    } else {
                        while i < e && !is_ws(b[i]) {
                            i += 1;
                        }
                    }
                    let t = &b[ts..i];
                    let mut buf = [0u8; 64];
                    let size = t[0].is_ascii_digit()
                        || t[0] == b'.'
                        || t.contains(&b'/')
                        || lower(t, &mut buf).is_some_and(|l| has(FONT_SIZE_KEYWORDS, l));
                    if size {
                        last = i;
                    }
                }
                if last == NONE {
                    self.parts = parts;
                    return;
                }
                s = last;
                let (s2, _) = trim(b, s, e);
                s = s2;
            }
            if s >= e {
                self.parts = parts;
                return;
            }
            fams.push((s, e));
        }
        self.parts = parts;
        let quoted = |s: usize| matches!(b[s], b'"' | b'\'');
        let keyword = |s: usize, e: usize| {
            let mut buf = [0u8; 64];
            !quoted(s) && lower(&b[s..e], &mut buf).is_some_and(|l| has(FONT_KEYWORDS, l))
        };
        if self.c.s4649 && !in_font_face && !fams.iter().any(|&(s, e)| keyword(s, e)) {
            let all_quoted_or_words = fams
                .iter()
                .all(|&(s, e)| quoted(s) || b[s..e].iter().all(|&c| is_ident_char(c) || c == b' ' || c == b'\t'));
            let system = fams.len() == 1
                && !quoted(fams[0].0)
                && ["caption", "icon", "menu", "message-box", "small-caption", "status-bar"]
                    .iter()
                    .any(|k| b[fams[0].0..fams[0].1].eq_ignore_ascii_case(k.as_bytes()));
            if all_quoted_or_words && !system {
                self.rep("S4649", vs, ve, "Unexpected missing generic font family".into());
            }
        }
        if self.c.s4648 {
            let name = |s: usize, e: usize| -> Vec<u8> {
                if quoted(s) {
                    b[s + 1..e.saturating_sub(1).max(s + 1)].to_vec()
                } else {
                    let mut out = Vec::new();
                    for w in b[s..e].split(|&c| is_ws(c)).filter(|w| !w.is_empty()) {
                        if !out.is_empty() {
                            out.push(b' ');
                        }
                        out.extend_from_slice(w);
                    }
                    out
                }
            };
            let mut seen_kw: Vec<Vec<u8>> = Vec::new();
            let mut seen: Vec<Vec<u8>> = Vec::new();
            for &(s, e) in &fams {
                let kw = keyword(s, e);
                let mut nm = name(s, e);
                let list = if kw {
                    nm.make_ascii_lowercase();
                    &mut seen_kw
                } else {
                    &mut seen
                };
                if list.contains(&nm) {
                    let shown = String::from_utf8_lossy(&name(s, e)).into_owned();
                    self.rep("S4648", s, e, format!("Unexpected duplicate name {shown}"));
                } else {
                    list.push(nm);
                }
            }
        }
    }

    // ---- selectors --------------------------------------------------------------------------

    fn selector(&mut self, s: usize, e: usize) {
        let c = self.c;
        if !(c.s4659 || c.s4660 || c.s4670) {
            return;
        }
        let sel = &self.b[s..e];
        if self.d == Dialect::Less && less_mixin(sel) {
            return;
        }
        let types = c.s4670 && !has_interp(sel);
        self.sel_walk(s, e, types);
    }

    fn sel_walk(&mut self, s: usize, e: usize, types: bool) {
        let b = self.b;
        let mut i = s;
        let mut compound = true;
        while i < e {
            let c = b[i];
            match c {
                b' ' | b'\t' | b'\n' | b'\r' | b'\x0c' | b'>' | b'+' | b'~' | b',' => {
                    compound = true;
                    i += 1;
                }
                b'/' => {
                    if b.get(i + 1) == Some(&b'*') {
                        i = comment_end(b, i, e);
                    } else if self.d != Dialect::Css && b.get(i + 1) == Some(&b'/') {
                        i = eol(b, i, e);
                    } else {
                        // `/deep/` combinator.
                        i = memchr::memchr(b'/', &b[i + 1..e]).map_or(e, |k| i + 1 + k + 1);
                        compound = true;
                    }
                }
                b'"' | b'\'' => i = skip_str(b, i, e),
                b'[' => {
                    i = (matching(b, i, e, b'[', b']') + 1).min(e);
                    compound = false;
                }
                b'.' | b'#' | b'%' | b'@' | b'$' => {
                    i += 1;
                    if i < e && b[i] == b'{' {
                        i = skip_braces(b, i, e);
                    } else {
                        i = ident_end(b, i, e);
                    }
                    compound = false;
                }
                b'&' | b'*' => {
                    i = ident_end(b, i + 1, e);
                    compound = false;
                }
                b'|' => {
                    i += 1;
                    compound = false;
                }
                b':' => {
                    let elem = b.get(i + 1) == Some(&b':');
                    let ns = i + if elem { 2 } else { 1 };
                    let ne = ident_end(b, ns, e);
                    if ne > ns {
                        self.pseudo(ns, ne, elem);
                    }
                    i = ne.max(i + 1);
                    compound = false;
                    if i < e && b[i] == b'(' {
                        let close = matching(b, i, e, b'(', b')');
                        let mut buf = [0u8; 64];
                        if lower(&b[ns..ne], &mut buf).is_some_and(selector_args) {
                            self.sel_walk(i + 1, close, types);
                        }
                        i = (close + 1).min(e);
                    }
                }
                b'(' => i = (matching(b, i, e, b'(', b')') + 1).min(e),
                b'!' => return,
                b'0'..=b'9' => {
                    while i < e && (b[i].is_ascii_alphanumeric() || b[i] == b'.' || b[i] == b'%') {
                        i += 1;
                    }
                    compound = false;
                }
                _ if is_ident_start(c) => {
                    let ne = ident_end(b, i, e);
                    let next = b.get(ne).copied().unwrap_or(b' ');
                    if self.d == Dialect::Less && b[i..ne] == *b"when" {
                        return;
                    }
                    if compound && types && ne > i && next != b'|' && next != b'(' {
                        self.type_sel(i, ne);
                    }
                    i = ne.max(i + 1);
                    compound = false;
                }
                _ => i += 1,
            }
        }
    }

    fn pseudo(&mut self, s: usize, e: usize, elem: bool) {
        let name = &self.b[s..e];
        if name[0] == b'-' || contains(name, b"\\") {
            return;
        }
        let mut buf = [0u8; 64];
        let Some(l) = lower(name, &mut buf) else { return };
        if elem {
            if self.c.s4660 && !has(PSEUDO_ELEMENTS, l) && !listed(&self.c.ignore_pe, l) {
                let msg = format!("Unexpected unknown pseudo-element selector \"::{}\"", self.text(s, e));
                self.rep("S4660", s - 2, e, msg);
            }
        } else if self.c.s4659 && !has(PSEUDO_CLASSES, l) && !has(PSEUDO_ELEMENTS, l) && !listed(&self.c.ignore_pc, l) {
            let msg = format!("Unexpected unknown pseudo-class selector \":{}\"", self.text(s, e));
            self.rep("S4659", s - 1, e, msg);
        }
    }

    fn type_sel(&mut self, s: usize, e: usize) {
        let name = &self.b[s..e];
        if name.contains(&b'-') || name.contains(&b'\\') || name[0] == b'_' {
            return;
        }
        let mut buf = [0u8; 64];
        let Some(l) = lower(name, &mut buf) else { return };
        if has(ELEMENTS, l) || matches!(l, b"from" | b"to") || listed(&self.c.ignore_types, l) {
            return;
        }
        let msg = format!("Unexpected unknown type selector \"{}\"", self.text(s, e));
        self.rep("S4670", s, e, msg);
    }

    fn dup_selector(&mut self, depth: usize, s: usize, e: usize) {
        if !self.c.s4666 {
            return;
        }
        let b = self.b;
        let sel = &b[s..e];
        if sel.is_empty() || has_interp(sel) || (self.d == Dialect::Less && less_mixin(sel)) {
            return;
        }
        let mut parts = std::mem::take(&mut self.parts);
        split_commas(b, s, e, &mut parts);
        let mut norm = std::mem::take(&mut self.norm);
        norm.clear();
        let mut ranges: Vec<(usize, usize)> = Vec::with_capacity(parts.len());
        for &(ps, pe) in &parts {
            let start = norm.len();
            let (ps, pe) = trim(b, ps, pe);
            let mut i = ps;
            let mut space = false;
            while i < pe {
                let c = b[i];
                if c == b'/' && b.get(i + 1) == Some(&b'*') {
                    i = comment_end(b, i, pe);
                    space = true;
                    continue;
                }
                if c == b'"' || c == b'\'' {
                    let j = skip_str(b, i, pe);
                    if space && norm.len() > start {
                        norm.push(b' ');
                    }
                    space = false;
                    norm.extend_from_slice(&b[i..j]);
                    i = j;
                    continue;
                }
                if is_ws(c) {
                    space = true;
                } else if matches!(c, b'>' | b'+' | b'~') {
                    norm.push(c);
                    space = false;
                } else {
                    if space && norm.len() > start && !matches!(norm.last(), Some(b'>' | b'+' | b'~')) {
                        norm.push(b' ');
                    }
                    space = false;
                    norm.push(c);
                }
                i += 1;
            }
            ranges.push((start, norm.len()));
        }
        ranges.sort_by(|a, c| norm[a.0..a.1].cmp(&norm[c.0..c.1]));
        let mut h = FxHasher::default();
        for &(a, c) in &ranges {
            h.write(&norm[a..c]);
            h.write_u8(0);
        }
        let key = h.finish();
        self.norm = norm;
        self.parts = parts;
        let first = self.frames[depth].sels.get(&key).copied();
        if let Some(first) = first {
            let shown: String = self.text(s, e).split_whitespace().collect::<Vec<_>>().join(" ");
            let line = self.lines.line(first as u32);
            self.rep("S4666", s, e, format!("Unexpected duplicate selector \"{shown}\", first used at line {line}"));
        } else {
            self.frames[depth].sels.insert(key, s);
        }
    }
}

/// Less mixin definitions and guarded rules: `.m(@a) {`, `.m() when (...) {`.
fn less_mixin(sel: &[u8]) -> bool {
    if contains(sel, b" when ") || contains(sel, b"when(") {
        return true;
    }
    let mut i = 0;
    while i < sel.len() {
        if matches!(sel[i], b'.' | b'#') {
            let e = ident_end(sel, i + 1, sel.len());
            if e > i + 1 && e < sel.len() && sel[e] == b'(' {
                return true;
            }
            i = e.max(i + 1);
        } else {
            i += 1;
        }
    }
    false
}
