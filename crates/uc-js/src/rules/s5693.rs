//! HTTP request content length should be limited
use crate::helpers::batch_security2::*;
use crate::prelude::*;

pub struct Rule {
    file_upload_limit: f64,
    standard_limit: f64,
}

impl Rule {
    pub fn new(p: &Params) -> Self {
        Self {
            file_upload_limit: f64::from(p.u32("fileUploadSizeLimit", 8_000_000)),
            standard_limit: f64::from(p.u32("standardSizeLimit", 2_000_000)),
        }
    }
}

const MESSAGE: &str = "Make sure the content length limit is safe here.";
const FORMIDABLE_DEFAULT: f64 = 200.0 * 1024.0 * 1024.0;
const BODY_PARSER_DEFAULT: f64 = 100.0 * 1024.0;

/// The `bytes` package's `parse`: `"100kb"` → 102400, `"12"` → 12.
fn parse_bytes(s: &str) -> Option<f64> {
    let t = s.trim();
    let lower = t.to_ascii_lowercase();
    for (unit, mul) in [
        ("kb", 1024f64),
        ("mb", 1024f64.powi(2)),
        ("gb", 1024f64.powi(3)),
        ("tb", 1024f64.powi(4)),
        ("pb", 1024f64.powi(5)),
    ] {
        if let Some(num) = lower.strip_suffix(unit) {
            let num = num.trim_end_matches(' ');
            let digits = num.strip_prefix(['-', '+']).unwrap_or(num);
            let valid = !digits.is_empty()
                && digits.split_once('.').map_or(
                    digits.bytes().all(|b| b.is_ascii_digit()),
                    |(i, f)| {
                        !i.is_empty()
                            && !f.is_empty()
                            && i.bytes().all(|b| b.is_ascii_digit())
                            && f.bytes().all(|b| b.is_ascii_digit())
                    },
                );
            if valid {
                return num.parse::<f64>().ok().map(|v| (v * mul).floor());
            }
        }
    }
    // parseInt(value, 10)
    let (sign, rest) = match t.as_bytes().first() {
        Some(b'-') => (-1.0, &t[1..]),
        Some(b'+') => (1.0, &t[1..]),
        _ => (1.0, t),
    };
    let end = rest
        .bytes()
        .position(|b| !b.is_ascii_digit())
        .unwrap_or(rest.len());
    rest[..end].parse::<f64>().ok().map(|v| sign * v)
}

fn size_value<'a>(ctx: &Ctx<'a, '_>, e: &'a Expression<'a>, depth: u8) -> Option<f64> {
    if depth > 16 {
        return None;
    }
    let e = resolve(ctx, e);
    match e {
        Expression::NumericLiteral(n) => Some(n.value),
        Expression::StringLiteral(s) => parse_bytes(&s.value),
        Expression::BinaryExpression(b) => {
            let l = size_value(ctx, &b.left, depth + 1)?;
            let r = size_value(ctx, &b.right, depth + 1)?;
            let v = match b.operator {
                BinaryOperator::Multiplication => l * r,
                BinaryOperator::Addition => l + r,
                BinaryOperator::Subtraction => l - r,
                BinaryOperator::Division => l / r,
                BinaryOperator::Exponential => l.powf(r),
                _ => return None,
            };
            v.is_finite().then_some(v)
        }
        _ => None,
    }
}

struct Pending {
    size: f64,
    span: Span,
}

impl Rule {
    fn report(&self, ctx: &Ctx<'_, '_>, span: Span, size: Option<f64>, standard: bool) {
        let limit = if standard {
            self.standard_limit
        } else {
            self.file_upload_limit
        };
        if size.is_none_or(|s| s == 0.0 || s > limit) {
            ctx.report(span, MESSAGE);
        }
    }

    fn check_size<'a>(
        &self,
        ctx: &Ctx<'a, '_>,
        call_span: Span,
        prop: Prop<'a>,
        default: Option<f64>,
        standard: bool,
    ) {
        match prop {
            Prop::Found(p) => {
                if let Some(v) = size_value(ctx, &p.value, 0)
                    && v != 0.0
                {
                    self.report(ctx, p.span, Some(v), standard);
                }
            }
            Prop::Missing => self.report(ctx, call_span, default, standard),
            // An unresolved spread may set the limit.
            Prop::Unknown => {}
        }
    }
}

impl JsRule for Rule {
    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {
        if !["formidable", "multer", "body-parser"]
            .iter()
            .any(|m| src_contains(ctx, m))
        {
            return;
        }
        let mut forms: FxHashMap<SymbolId, Pending> = FxHashMap::default();
        let mut order: Vec<SymbolId> = Vec::new();
        for node in ctx.nodes().iter() {
            let (callee, args, span) = match node.kind() {
                AstKind::CallExpression(c) => (&c.callee, &c.arguments, c.span),
                AstKind::NewExpression(n) => (&n.callee, &n.arguments, n.span),
                AstKind::AssignmentExpression(a) => {
                    let AssignmentTarget::StaticMemberExpression(m) = &a.left else {
                        continue;
                    };
                    if m.property.name != "maxFileSize" {
                        continue;
                    }
                    let Expression::Identifier(id) = strip(&m.object) else {
                        continue;
                    };
                    let Some(sym) = ctx.symbol_of(id) else {
                        continue;
                    };
                    if let Some(pending) = forms.get_mut(&sym) {
                        match size_value(ctx, &a.right, 0) {
                            Some(v) if v != 0.0 => {
                                pending.size = v;
                                pending.span = a.span;
                            }
                            _ => {
                                forms.remove(&sym);
                            }
                        }
                    }
                    continue;
                }
                _ => continue,
            };
            let module_ident = match strip(callee) {
                Expression::StaticMemberExpression(m) => strip(&m.object),
                Expression::ComputedMemberExpression(m) => strip(&m.object),
                c @ Expression::Identifier(_) => c,
                _ => continue,
            };
            if !matches!(module_ident, Expression::Identifier(_)) {
                continue;
            }
            let Some(f) = fqn(ctx, module_ident) else {
                continue;
            };
            let module = f.split('.').next().unwrap_or("");
            match module {
                "formidable" => {
                    if args.is_empty() {
                        let lhs = match ctx.parent_kind(node.id()) {
                            AstKind::VariableDeclarator(d) => match &d.id {
                                BindingPattern::BindingIdentifier(b) => b.symbol_id.get(),
                                _ => None,
                            },
                            AstKind::AssignmentExpression(a) => match &a.left {
                                AssignmentTarget::AssignmentTargetIdentifier(id) => {
                                    ctx.symbol_of(id)
                                }
                                _ => None,
                            },
                            _ => None,
                        };
                        if let Some(sym) = lhs {
                            if !forms.contains_key(&sym) {
                                order.push(sym);
                            }
                            forms.insert(
                                sym,
                                Pending {
                                    size: FORMIDABLE_DEFAULT,
                                    span,
                                },
                            );
                        }
                        continue;
                    }
                    if let Some(options) = arg(args, 0).and_then(|a| object_of(ctx, a)) {
                        let p = get_property(ctx, options, "maxFileSize");
                        self.check_size(ctx, span, p, Some(FORMIDABLE_DEFAULT), false);
                    }
                }
                "multer" => {
                    if !matches!(strip(callee), Expression::Identifier(_)) || f != "multer" {
                        continue;
                    }
                    if args.is_empty() {
                        self.report(ctx, callee.span(), None, false);
                        continue;
                    }
                    let Some(options) = arg(args, 0).and_then(|a| object_of(ctx, a)) else {
                        continue;
                    };
                    match get_property(ctx, options, "limits") {
                        Prop::Found(limits) => {
                            if let Expression::ObjectExpression(lo) = strip(&limits.value) {
                                let p = get_property(ctx, lo, "fileSize");
                                self.check_size(ctx, span, p, None, false);
                            }
                        }
                        Prop::Missing => self.report(ctx, callee.span(), None, false),
                        Prop::Unknown => {}
                    }
                }
                "body-parser" => {
                    if args.is_empty() {
                        self.report(ctx, span, Some(BODY_PARSER_DEFAULT), true);
                        continue;
                    }
                    let Some(options) = arg(args, 0).and_then(|a| object_of(ctx, a)) else {
                        continue;
                    };
                    let p = get_property(ctx, options, "limits");
                    self.check_size(ctx, span, p, Some(BODY_PARSER_DEFAULT), true);
                }
                _ => {}
            }
        }
        for sym in order {
            if let Some(p) = forms.remove(&sym) {
                self.report(ctx, p.span, Some(p.size), false);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn bytes() {
        assert_eq!(super::parse_bytes("100kb"), Some(102400.0));
        assert_eq!(super::parse_bytes("1.5 MB"), Some(1572864.0));
        assert_eq!(super::parse_bytes("42"), Some(42.0));
        assert_eq!(super::parse_bytes("abc"), None);
    }

    #[test]
    fn rule() {
        check(
            "S5693",
            "a.js",
            r#"
const formidable = require('formidable');
const multer = require('multer');
const bodyParser = require('body-parser');
const form1 = new formidable.IncomingForm(); // Noncompliant {{Make sure the content length limit is safe here.}}
const form2 = new formidable.IncomingForm();
form2.maxFileSize = 4000000;
const form3 = new formidable.IncomingForm();
form3.maxFileSize = 10 * 1024 * 1024; // Noncompliant
formidable({ maxFileSize: 9000000 }); // Noncompliant
formidable({ maxFileSize: 8000000 });
formidable({ keepExtensions: true }); // Noncompliant
multer(); // Noncompliant
multer({ storage: s }); // Noncompliant
multer({ storage: s, limits: { fileSize: 8000000 } });
multer({ storage: s, limits: { fileSize: 9000000 } }); // Noncompliant
multer({ storage: s, limits: { files: 1 } }); // Noncompliant
bodyParser.json();
bodyParser.json({ limits: '3mb' }); // Noncompliant
bodyParser.json({ limits: '1mb' });
bodyParser.urlencoded({ extended: true });
"#,
        );
    }
}
