//! File permissions should not be set to world-accessible values
use crate::helpers::batch_security1::{call_fqn, unique_init};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const CHMOD_LIKE: &[&str] = &[
    "chmod",
    "chmodSync",
    "fchmod",
    "fchmodSync",
    "lchmod",
    "lchmodSync",
];

fn fs_const(name: &str) -> Option<i64> {
    Some(match name {
        "S_IRWXU" => 0o700,
        "S_IRUSR" => 0o400,
        "S_IWUSR" => 0o200,
        "S_IXUSR" => 0o100,
        "S_IRWXG" => 0o70,
        "S_IRGRP" => 0o40,
        "S_IWGRP" => 0o20,
        "S_IXGRP" => 0o10,
        "S_IRWXO" => 0o7,
        "S_IROTH" => 0o4,
        "S_IWOTH" => 0o2,
        "S_IXOTH" => 0o1,
        _ => return None,
    })
}

/// JavaScript `parseInt(s, 8)`: leading octal digits after optional whitespace and sign.
fn parse_int_octal(s: &str) -> Option<i64> {
    let s = s.trim_start();
    let (neg, s) = match s.as_bytes().first() {
        Some(b'-') => (true, &s[1..]),
        Some(b'+') => (false, &s[1..]),
        _ => (false, s),
    };
    let digits: &str = &s[..s.bytes().take_while(|b| (b'0'..=b'7').contains(b)).count()];
    if digits.is_empty() || digits.len() > 20 {
        return None;
    }
    let v = i64::from_str_radix(digits, 8).ok()?;
    Some(if neg { -v } else { v })
}

fn mode_of(ctx: &Ctx<'_, '_>, e: &Expression<'_>, depth: u8) -> Option<i64> {
    if depth > 6 {
        return None;
    }
    match strip_parens(e) {
        Expression::StaticMemberExpression(m) => {
            let Expression::StaticMemberExpression(obj) = &m.object else {
                return None;
            };
            if obj.property.name == "constants"
                && matches!(&obj.object, Expression::Identifier(fs) if fs.name == "fs")
            {
                fs_const(m.property.name.as_str())
            } else {
                None
            }
        }
        Expression::StringLiteral(s) => parse_int_octal(&s.value),
        Expression::NumericLiteral(n) => {
            let raw = n.raw.as_ref().map(|r| r.as_str()).unwrap_or("");
            if raw.starts_with('0') && !raw.starts_with("0o") {
                parse_int_octal(raw)
            } else if n.value.fract() == 0.0 && n.value.abs() < 1e15 {
                Some(n.value as i64)
            } else {
                None
            }
        }
        Expression::Identifier(id) => {
            let sym = ctx.symbol_of(id)?;
            mode_of(ctx, unique_init(ctx, sym)?, depth + 1)
        }
        Expression::BinaryExpression(b) if b.operator == BinaryOperator::BitwiseOR => {
            let l = mode_of(ctx, &b.left, depth + 1)?;
            let r = mode_of(ctx, &b.right, depth + 1)?;
            (l != 0 && r != 0).then_some(l | r)
        }
        _ => None,
    }
}

impl Rule {
    fn check(&self, ctx: &Ctx<'_, '_>, arg: Option<&Argument<'_>>, expected: i64) {
        let Some(e) = arg.and_then(|a| a.as_expression()) else {
            return;
        };
        if let Some(mode) = mode_of(ctx, e, 0)
            && mode % 8 != expected
        {
            ctx.report(e.span(), "Make sure this permission is safe.");
        }
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::CallExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::CallExpression(call) = node.kind() else {
            return;
        };
        match strip_parens(&call.callee) {
            Expression::StaticMemberExpression(m)
                if CHMOD_LIKE.contains(&m.property.name.as_str()) =>
            {
                self.check(ctx, call.arguments.first(), 0);
                self.check(ctx, call.arguments.get(1), 0);
            }
            _ => {
                if callee_name(call) == Some("umask")
                    && call_fqn(ctx, call).as_deref() == Some("process.umask")
                {
                    self.check(ctx, call.arguments.first(), 7);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn file_permissions() {
        check(
            "S2612",
            "a.js",
            r#"
const fs = require('fs');
fs.chmodSync("/tmp/fs", 0o777); // Noncompliant {{Make sure this permission is safe.}}
fs.chmodSync("/tmp/fs", 0o770);
fs.chmod("/tmp/fs", "777", cb); // Noncompliant
fs.chmod("/tmp/fs", "770", cb);
fs.chmodSync(file, fs.constants.S_IRWXU | fs.constants.S_IRWXO); // Noncompliant
fs.chmodSync(file, fs.constants.S_IRWXU | fs.constants.S_IRWXG);
const mode = 0o704;
fs.promises.chmod(file, mode); // Noncompliant
fs.lchmodSync(file, 0);
process.umask(0o000); // Noncompliant
process.umask(0o007);
process.umask(0o777);
fs.chmodSync(file, unknownMode);
"#,
        );
    }
}
