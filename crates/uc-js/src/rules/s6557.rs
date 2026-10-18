//! Ends of strings should be checked with "startsWith()" and "endsWith()"
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const STARTS: &str = "Use 'String#startsWith' method instead.";
const ENDS: &str = "Use the 'String#endsWith' method instead.";

fn is_equality(op: BinaryOperator) -> bool {
    matches!(
        op,
        BinaryOperator::Equality
            | BinaryOperator::Inequality
            | BinaryOperator::StrictEquality
            | BinaryOperator::StrictInequality
    )
}

fn is_number(e: &Expression<'_>, v: f64) -> bool {
    matches!(strip_parens(e), Expression::NumericLiteral(n) if n.value == v)
}

fn is_string(ctx: &Ctx<'_, '_>, e: &Expression<'_>) -> bool {
    type_of(ctx, e).is_string()
}

/// `x.length` where `x` has the same tokens as `obj`, or a string literal's length.
fn is_length_of(ctx: &Ctx<'_, '_>, e: &Expression<'_>, obj: &Expression<'_>) -> bool {
    match strip_parens(e) {
        Expression::StaticMemberExpression(m) => {
            m.property.name == "length"
                && ctx.same_tokens(strip_parens(&m.object).span(), strip_parens(obj).span())
        }
        Expression::NumericLiteral(n) => match strip_parens(obj) {
            Expression::StringLiteral(s) => s.value.encode_utf16().count() as f64 == n.value,
            _ => false,
        },
        _ => false,
    }
}

/// `obj.length - 1`.
fn is_last_index(ctx: &Ctx<'_, '_>, e: &Expression<'_>, obj: &Expression<'_>) -> bool {
    matches!(strip_parens(e), Expression::BinaryExpression(b)
        if b.operator == BinaryOperator::Subtraction && is_number(&b.right, 1.0)
            && matches!(strip_parens(&b.left), Expression::StaticMemberExpression(m)
                if m.property.name == "length" && ctx.same_tokens(strip_parens(&m.object).span(), strip_parens(obj).span())))
}

/// `-n`, or `obj.length - n`.
fn is_negative_index(ctx: &Ctx<'_, '_>, e: &Expression<'_>, obj: &Expression<'_>) -> bool {
    match strip_parens(e) {
        Expression::UnaryExpression(u) => {
            u.operator == UnaryOperator::UnaryNegation
                && matches!(strip_parens(&u.argument), Expression::NumericLiteral(_))
        }
        Expression::BinaryExpression(b) => {
            b.operator == BinaryOperator::Subtraction
                && matches!(strip_parens(&b.left), Expression::StaticMemberExpression(m)
                    if m.property.name == "length" && ctx.same_tokens(strip_parens(&m.object).span(), strip_parens(obj).span()))
        }
        _ => false,
    }
}

/// `/^literal/` or `/literal$/` (no `i` / `m` flags, only plain characters).
/// Pattern source and "has i/m/v flag" of a statically known regex: a literal, `new RegExp('..')`
/// with literal arguments, or a never-reassigned variable initialized with one of those.
fn static_regex<'a>(
    ctx: &Ctx<'a, '_>,
    e: &'a Expression<'a>,
    depth: u8,
) -> Option<(&'a str, bool)> {
    match strip_parens(e) {
        Expression::RegExpLiteral(r) => {
            let f = r.regex.flags;
            let bad = f.contains(RegExpFlags::I)
                || f.contains(RegExpFlags::M)
                || f.contains(RegExpFlags::V);
            Some((r.regex.pattern.text.as_str(), bad))
        }
        Expression::NewExpression(n) => {
            if !matches!(&n.callee, Expression::Identifier(id) if ctx.is_global(id, "RegExp")) {
                return None;
            }
            let src = n
                .arguments
                .first()?
                .as_expression()
                .and_then(string_value)?;
            let bad = match n.arguments.get(1) {
                None => false,
                Some(a) => a
                    .as_expression()
                    .and_then(string_value)?
                    .contains(['i', 'm', 'v']),
            };
            (n.arguments.len() <= 2).then_some((src, bad))
        }
        Expression::Identifier(id) if depth == 0 => {
            let sym = ctx.symbol_of(id)?;
            if ctx.scoping().symbol_is_mutated(sym) {
                return None;
            }
            match ctx.symbol_decl(sym).kind() {
                AstKind::VariableDeclarator(v) => static_regex(ctx, v.init.as_ref()?, 1),
                _ => None,
            }
        }
        _ => None,
    }
}

fn regex_anchor<'a>(ctx: &Ctx<'a, '_>, e: &'a Expression<'a>) -> Option<bool> {
    let (src, bad_flags) = static_regex(ctx, e, 0)?;
    if bad_flags {
        return None;
    }
    let starts = src.starts_with('^');
    let ends = src.ends_with('$') && !src.ends_with("\\$");
    if starts == ends {
        return None;
    }
    let body = if starts {
        &src[1..]
    } else {
        &src[..src.len() - 1]
    };
    if body.is_empty() {
        return None;
    }
    let mut chars = body.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => match chars.next() {
                Some(n) if !n.is_ascii_alphanumeric() => {}
                _ => return None,
            },
            '.' | '*' | '+' | '?' | '(' | ')' | '[' | ']' | '{' | '}' | '|' | '^' | '$' => {
                return None;
            }
            _ => {}
        }
    }
    Some(starts)
}

fn member<'a>(e: &'a Expression<'a>) -> Option<(&'a Expression<'a>, &'a str)> {
    match strip_parens(e) {
        Expression::StaticMemberExpression(m) => Some((&m.object, m.property.name.as_str())),
        _ => None,
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::BinaryExpression, AstType::CallExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        match node.kind() {
            AstKind::BinaryExpression(b) if is_equality(b.operator) => self.binary(ctx, b),
            AstKind::CallExpression(c) => {
                // `/^bar/.test(foo)`
                let Some((obj, "test")) = member(&c.callee) else {
                    return;
                };
                if c.arguments.len() != 1 {
                    return;
                }
                if let Some(starts) = regex_anchor(ctx, obj) {
                    ctx.report(c.span, if starts { STARTS } else { ENDS });
                }
            }
            _ => {}
        }
    }
}

impl Rule {
    fn binary<'a>(&self, ctx: &Ctx<'a, '_>, b: &'a BinaryExpression<'a>) {
        let left = strip_parens(&b.left);
        let report = |starts: bool| ctx.report(b.span, if starts { STARTS } else { ENDS });
        match left {
            // `foo[0] === 'a'`, `foo[foo.length - 1] === 'a'`
            Expression::ComputedMemberExpression(m) => {
                if !is_string(ctx, &m.object) {
                    return;
                }
                if is_last_index(ctx, &m.expression, &m.object) {
                    report(false);
                } else if is_number(&m.expression, 0.0) {
                    report(true);
                }
            }
            Expression::CallExpression(call) => {
                let Some((obj, name)) = member(&call.callee) else {
                    return;
                };
                let args: Vec<&'a Expression<'a>> = call
                    .arguments
                    .iter()
                    .filter_map(|a| a.as_expression())
                    .collect();
                if args.len() != call.arguments.len() {
                    return;
                }
                match name {
                    "charAt" if args.len() == 1 => {
                        if !is_string(ctx, obj) {
                            return;
                        }
                        if is_last_index(ctx, args[0], obj) {
                            report(false);
                        } else if is_number(args[0], 0.0) {
                            report(true);
                        }
                    }
                    "indexOf" if args.len() == 1 => {
                        if is_number(&b.right, 0.0) && is_string(ctx, obj) {
                            report(true);
                        }
                    }
                    "lastIndexOf" if args.len() == 1 => {
                        if let Expression::BinaryExpression(r) = strip_parens(&b.right)
                            && r.operator == BinaryOperator::Subtraction
                            && is_length_of(ctx, &r.left, obj)
                            && is_length_of(ctx, &r.right, args[0])
                            && is_string(ctx, obj)
                        {
                            report(false);
                        }
                    }
                    "match" if args.len() == 1 => {
                        if is_null(&b.right)
                            && is_string(ctx, obj)
                            && let Some(starts) = regex_anchor(ctx, args[0])
                        {
                            report(starts);
                        }
                    }
                    "slice" | "substring" => {
                        if !is_string(ctx, obj) {
                            return;
                        }
                        if name == "slice"
                            && args.len() == 1
                            && is_negative_index(ctx, args[0], obj)
                        {
                            report(false);
                        } else if args.len() == 2
                            && is_number(args[0], 0.0)
                            && is_length_of(ctx, args[1], &b.right)
                        {
                            report(true);
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn starts_ends_with() {
        check(
            "S6557",
            "a.ts",
            r#"
function f(s: string, t: string) {
  if (s[0] === 'a') {}                        // Noncompliant {{Use 'String#startsWith' method instead.}}
  if (s.charAt(s.length - 1) === 'a') {}      // Noncompliant {{Use the 'String#endsWith' method instead.}}
  if (s.indexOf(t) === 0) {}                  // Noncompliant
  if (s.lastIndexOf(t) === s.length - t.length) {} // Noncompliant
  if (s.match(/^foo/) !== null) {}            // Noncompliant
  if (/bar$/.test(s)) {}                      // Noncompliant
  if (s.slice(0, 3) === 'foo') {}             // Noncompliant
  if (s.slice(-3) === 'bar') {}               // Noncompliant
  if (s.indexOf(t) === 1) {}
  if (/^a.b/.test(s)) {}
  if (/^foo/i.test(s)) {}
  const re = new RegExp('^\\/rss\\/');
  if (re.test(s)) {}                          // Noncompliant
  if (new RegExp('^a', 'i').test(s)) {}
  if (s[1] === 'a') {}
  if (s.slice(0, 2) === 'foo') {}
}
function g(a: number[], x) {
  if (a[0] === 1) {}
  if (x.indexOf('a') === 0) {}
}
"#,
        );
    }
}
