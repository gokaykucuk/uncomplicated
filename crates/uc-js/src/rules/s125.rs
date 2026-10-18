//! Sections of code should not be commented out
use oxc_allocator::Allocator;
use oxc_parser::{ParseOptions, Parser, config::TokensParserConfig};
use oxc_span::SourceType;

use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const WRAPPER: &str = "_S125Wrapper";

// ---- code recognizer (probability that a line is JavaScript code) ------------------------------

fn detector(probability: f64, matches: usize) -> f64 {
    if matches == 0 {
        0.0
    } else {
        1.0 - (1.0 - probability).powi(matches as i32)
    }
}

fn ends_with_code_char(line: &str) -> usize {
    for c in line.chars().rev() {
        if matches!(c, '}' | ';' | '{') {
            return 1;
        }
        if !c.is_whitespace() && c != '*' && c != '/' {
            return 0;
        }
    }
    0
}

const STRONG_KEYWORDS: &[&str] = &["++", "||", "&&", "===", "?.", "??"];
const KEYWORDS: &[&str] = &[
    "public",
    "abstract",
    "class",
    "implements",
    "extends",
    "return",
    "throw",
    "private",
    "protected",
    "enum",
    "continue",
    "assert",
    "boolean",
    "this",
    "instanceof",
    "interface",
    "static",
    "void",
    "super",
    "true",
    "case:",
    "let",
    "const",
    "var",
    "async",
    "await",
    "break",
    "yield",
    "typeof",
    "import",
    "export",
];
const CONTAINS: &[&str] = &[
    "for(",
    "if(",
    "while(",
    "catch(",
    "switch(",
    "try{",
    "else{",
    "this.",
    "window.",
    "import '",
    "import \"",
    "require(",
];

fn keyword_matches(line: &str, keywords: &[&str]) -> usize {
    line.split([' ', '\t', '(', ')', ',', '{', '}'])
        .filter(|w| keywords.contains(w))
        .count()
}

fn contains_matches(line: &str) -> usize {
    // `line.replace(/\s+/, '')` only removes the first run of whitespace.
    let stripped: std::borrow::Cow<str> = match line.find(char::is_whitespace) {
        Some(i) => {
            let end = line[i..]
                .find(|c: char| !c.is_whitespace())
                .map_or(line.len(), |e| i + e);
            format!("{}{}", &line[..i], &line[end..]).into()
        }
        None => line.into(),
    };
    let mut n: usize = CONTAINS.iter().map(|p| stripped.matches(p).count()).sum();
    // /;\s+\/\// (non-global: at most one match)
    if let Some(i) = stripped.find(';') {
        let mut found = false;
        let mut from = i;
        while let Some(j) = stripped[from..].find(';') {
            let rest = &stripped[from + j + 1..];
            let ws = rest.len() - rest.trim_start().len();
            if ws > 0 && rest[ws..].starts_with("//") {
                found = true;
                break;
            }
            from += j + 1;
        }
        n += usize::from(found);
    }
    n
}

fn is_lower(c: char) -> bool {
    let mut l = c.to_lowercase();
    l.next() == Some(c) && l.next().is_none()
}

fn is_upper(c: char) -> bool {
    let mut u = c.to_uppercase();
    u.next() == Some(c) && u.next().is_none()
}

fn camel_case(line: &str) -> usize {
    let mut it = line.chars().peekable();
    while let Some(c) = it.next() {
        if let Some(&n) = it.peek()
            && is_lower(c)
            && is_upper(n)
        {
            return 1;
        }
    }
    0
}

fn recognition(line: &str) -> f64 {
    let ps = [
        detector(0.95, ends_with_code_char(line)),
        detector(0.7, keyword_matches(line, STRONG_KEYWORDS)),
        detector(0.3, keyword_matches(line, KEYWORDS)),
        detector(0.95, contains_matches(line)),
        detector(0.5, camel_case(line)),
    ];
    let mut p = 0.0;
    for d in ps {
        p = 1.0 - (1.0 - p) * (1.0 - d);
    }
    p
}

fn could_be_js_code(text: &str) -> bool {
    text.split('\n').any(|l| recognition(l) >= 0.9)
}

// ---- cheap textual filters ---------------------------------------------------------------------

fn has_code_char(v: &str) -> bool {
    if v.bytes()
        .any(|b| matches!(b, b';' | b'{' | b'}' | b'(' | b')' | b'=' | b'<' | b'>'))
    {
        return true;
    }
    let word_start = |s: &str, i: usize| {
        i == 0 || !s.as_bytes()[i - 1].is_ascii_alphanumeric() && s.as_bytes()[i - 1] != b'_'
    };
    let mut from = 0;
    while let Some(i) = v[from..].find("import") {
        let at = from + i;
        let rest = &v[at + 6..];
        let ws = rest.len() - rest.trim_start().len();
        if word_start(v, at) && ws > 0 && rest[ws..].starts_with(['\'', '"']) {
            return true;
        }
        from = at + 6;
    }
    let mut from = 0;
    while let Some(i) = v[from..].find("export") {
        let at = from + i;
        if word_start(v, at) && v[at + 6..].starts_with(char::is_whitespace) {
            return true;
        }
        from = at + 6;
    }
    false
}

fn is_documentation_example(v: &str) -> bool {
    let first = v.split('\n').next().unwrap_or("");
    let mut s = first;
    while let Some(r) = s.strip_prefix("//") {
        s = r.trim_start();
    }
    let s = s.trim().to_ascii_lowercase();
    s.starts_with("e.g.")
        || s.starts_with("e.g:")
        || s.starts_with("example:")
        || s.starts_with("examples:")
        || (s.starts_with("for example")
            && !s[11..].starts_with(|c: char| c.is_alphanumeric() || c == '_'))
}

fn inject_missing_braces(v: &str) -> std::borrow::Cow<'_, str> {
    let (mut balance, mut min) = (0i32, 0i32);
    for c in v.chars() {
        if c == '{' {
            balance += 1;
        } else if c == '}' {
            balance -= 1;
            min = min.min(balance);
        }
    }
    let open = -min;
    let close = balance - min;
    if open > 0 || close > 0 {
        format!(
            "{}{v}{}",
            "{".repeat(open as usize),
            "}".repeat(close as usize)
        )
        .into()
    } else {
        v.into()
    }
}

// ---- parsing -----------------------------------------------------------------------------------

fn parse_options() -> ParseOptions {
    ParseOptions {
        allow_return_outside_function: true,
        ..ParseOptions::default()
    }
}

fn is_excluded_single(stmt: &Statement<'_>, tokens: &[oxc_parser::Token]) -> bool {
    match stmt {
        Statement::BreakStatement(_)
        | Statement::LabeledStatement(_)
        | Statement::ContinueStatement(_) => true,
        Statement::ReturnStatement(r) => r
            .argument
            .as_ref()
            .is_none_or(|a| matches!(strip_parens(a), Expression::Identifier(_))),
        Statement::ThrowStatement(t) => {
            matches!(strip_parens(&t.argument), Expression::Identifier(_))
        }
        Statement::ExpressionStatement(e) => {
            let ex = strip_parens(&e.expression);
            match ex {
                Expression::Identifier(_)
                | Expression::SequenceExpression(_)
                | Expression::StringLiteral(_)
                | Expression::NumericLiteral(_) => true,
                Expression::UnaryExpression(u)
                    if matches!(
                        u.operator,
                        UnaryOperator::UnaryPlus | UnaryOperator::UnaryNegation
                    ) =>
                {
                    true
                }
                _ => {
                    let span = e.span;
                    !tokens.iter().any(|t| {
                        t.kind() == Kind::Semicolon
                            && span.start <= t.start()
                            && t.end() <= span.end
                    })
                }
            }
        }
        _ => false,
    }
}

/// Parses the (pre-filtered) comment text and decides whether it is code.
fn contains_code(alloc: &mut Allocator, st: SourceType, value: &str) -> bool {
    alloc.reset();
    let ret = Parser::new(alloc, value, st)
        .with_options(parse_options())
        .with_config(TokensParserConfig)
        .parse();
    if !ret.fatal_error && ret.diagnostics.is_empty() {
        let p = &ret.program;
        let n = p.body.len() + p.directives.len();
        if n == 0 {
            return false;
        }
        if n == 1 {
            if !p.directives.is_empty() {
                return false; // a lone string literal
            }
            return !is_excluded_single(&p.body[0], &ret.tokens);
        }
        return true;
    }
    if !st.is_jsx() {
        return false;
    }
    let wrapped = format!("<{WRAPPER} {value} />");
    alloc.reset();
    let ret = Parser::new(alloc, &wrapped, st)
        .with_options(parse_options())
        .parse();
    if ret.fatal_error
        || !ret.diagnostics.is_empty()
        || ret.program.body.len() != 1
        || !ret.program.directives.is_empty()
    {
        return false;
    }
    let Statement::ExpressionStatement(e) = &ret.program.body[0] else {
        return false;
    };
    let Expression::JSXElement(el) = &e.expression else {
        return false;
    };
    let name = match &el.opening_element.name {
        JSXElementName::Identifier(n) => n.name.as_str(),
        JSXElementName::IdentifierReference(n) => n.name.as_str(),
        _ => return false,
    };
    if name != WRAPPER {
        return false;
    }
    let attrs = &el.opening_element.attributes;
    !attrs.is_empty()
        && attrs.iter().all(|a| match a {
            JSXAttributeItem::SpreadAttribute(_) => true,
            JSXAttributeItem::Attribute(a) => {
                matches!(a.name, JSXAttributeName::Identifier(_)) && a.value.is_some()
            }
        })
}

impl JsRule for Rule {
    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {
        let comments = ctx.comments();
        if comments.is_empty() {
            return;
        }
        // Group consecutive line comments (on adjacent lines, with no code in between).
        let mut groups: Vec<(usize, usize)> = Vec::new(); // inclusive index range into `comments`
        let mut current: Vec<usize> = Vec::new();
        for (i, c) in comments.iter().enumerate() {
            if c.is_block() {
                groups.push((i, i));
                continue;
            }
            let adjacent = current.last().is_some_and(|&p| {
                let prev = &comments[p];
                let next_line = ctx.line(c.span.start);
                ctx.line(prev.span.start) + 1 == next_line
                    && token_after(ctx, prev.span.end).is_none_or(|t| ctx.line(t.start) > next_line)
            });
            if current.is_empty() || adjacent {
                current.push(i);
            } else {
                groups.push((current[0], *current.last().unwrap()));
                current = vec![i];
            }
        }
        if let (Some(&f), Some(&l)) = (current.first(), current.last()) {
            groups.push((f, l));
        }

        let st = if ctx.is_ts() {
            let tsx = ctx.file.path.ends_with(".tsx");
            SourceType::ts().with_jsx(tsx)
        } else {
            SourceType::mjs().with_jsx(true)
        };
        let mut alloc: Option<Allocator> = None;
        for (first, last) in groups {
            let value: std::borrow::Cow<str> = if first == last {
                ctx.comment_text(&comments[first]).into()
            } else {
                (first..=last)
                    .map(|i| ctx.comment_text(&comments[i]))
                    .collect::<Vec<_>>()
                    .join("\n")
                    .into()
            };
            let trimmed = value.trim();
            if trimmed == "}" || trimmed.is_empty() || is_documentation_example(trimmed) {
                continue;
            }
            let text = inject_missing_braces(trimmed);
            if !has_code_char(&text) || !could_be_js_code(&text) {
                continue;
            }
            let alloc = alloc.get_or_insert_with(Allocator::default);
            if contains_code(alloc, st, &text) {
                ctx.report(
                    Span::new(comments[first].span.start, comments[last].span.end),
                    "Remove this commented out code.",
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn commented_code() {
        check(
            "S125",
            "a.js",
            r#"
/* Noncompliant@+1 */
// foo();
s();
// This is a regular comment, nothing to see here.
s();
/* if (a) { b(); } */ // Noncompliant
s();
/* Noncompliant@+1 */
// const x = 1;
s();
// return x;
s();
/* Noncompliant@+1 */
// let y = compute();
//   y++;
s();
// e.g. foo();
s();
// TODO: fix this
s();
// }
s();
/**
 * Computes the value.
 * @param {number} a the input
 */
s();
// http://example.com/path?a=b
s();
// foo
s();
// bar(baz)
s();
// break;
s();
/* Noncompliant@+1 */
let a = 1; // a = 2;
"#,
        );
    }

    #[test]
    fn groups() {
        check(
            "S125",
            "b.jsx",
            r#"
/* Noncompliant@+1 */
// function f() {
//   return 1;
// }
s();
/* Noncompliant@+1 */
// <Foo bar={1} />;
s();
// see the docs (chapter 3) for more
s();
"#,
        );
    }

    #[test]
    fn jsx_attributes() {
        check(
            "S125",
            "c.tsx",
            r#"
const a = (
  <div>
    {/* Noncompliant@+1 */}
    {/*width={500}*/}
    {/* Noncompliant@+1 */}
    {/*isOpen={this.state.open}*/}
  </div>
);
/* Noncompliant@+1 */
// value={response.webhook ? response.webhook.response.body : response.inline.logs}
"#,
        );
    }
}
