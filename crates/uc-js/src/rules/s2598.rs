//! File uploads should be restricted
use crate::helpers::batch_security1::{
    Prop, fqn, get_property, resolve_object, resolve_value, sec, src_mentions,
};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

fn report(
    ctx: &Ctx<'_, '_>,
    upload_dir: bool,
    keep_ext: bool,
    callee: Span,
    secondary: Option<(Span, Option<String>)>,
) {
    let message = match (keep_ext, upload_dir) {
        (true, true) => "Restrict the extension of uploaded files.",
        (false, false) => "Restrict folder destination of uploaded files.",
        (true, false) => "Restrict the extension and folder destination of uploaded files.",
        (false, true) => return,
    };
    ctx.report_with(callee, message, secondary);
}

fn bool_value(e: &Expression<'_>) -> bool {
    matches!(strip_parens(e), Expression::BooleanLiteral(b) if b.value)
}

impl Rule {
    fn check<'a>(
        &self,
        ctx: &Ctx<'a, '_>,
        node: &AstNode<'a>,
        callee: &Expression<'a>,
        args: &[Argument<'a>],
    ) {
        let Expression::Identifier(_) = strip_parens(callee) else {
            return;
        };
        let Some(q) = fqn(ctx, callee) else { return };
        match q.split('.').next() {
            Some("formidable") => self.check_formidable(ctx, node, callee.span(), args),
            Some("multer") => self.check_multer(ctx, callee.span(), args),
            _ => {}
        }
    }

    fn check_formidable<'a>(
        &self,
        ctx: &Ctx<'a, '_>,
        node: &AstNode<'a>,
        callee: Span,
        args: &[Argument<'a>],
    ) {
        if args.is_empty() {
            // `const form = formidable(); form.uploadDir = ...; form.keepExtensions = ...;`
            let sym = match ctx.parent(node.id()).kind() {
                AstKind::VariableDeclarator(v) => {
                    v.id.get_binding_identifier()
                        .and_then(|b| b.symbol_id.get())
                }
                AstKind::AssignmentExpression(a) => match &a.left {
                    AssignmentTarget::AssignmentTargetIdentifier(id) => ctx.symbol_of(id),
                    _ => None,
                },
                _ => None,
            };
            let Some(sym) = sym else { return };
            let mut upload_dir = false;
            let mut keep_ext = false;
            for r in ctx.scoping().get_resolved_references(sym) {
                let AstKind::StaticMemberExpression(m) = ctx.parent_kind(r.node_id()) else {
                    continue;
                };
                let member = ctx.parent(r.node_id());
                let AstKind::AssignmentExpression(a) = ctx.parent_kind(member.id()) else {
                    continue;
                };
                if a.left.span() != m.span {
                    continue;
                }
                match m.property.name.as_str() {
                    "uploadDir" => upload_dir = true,
                    "keepExtensions" => keep_ext = bool_value(&a.right),
                    _ => {}
                }
            }
            report(ctx, upload_dir, keep_ext, callee, None);
            return;
        }
        let Some(opts) = args[0].as_expression().and_then(|e| resolve_object(ctx, e)) else {
            return;
        };
        let upload_dir = get_property(ctx, opts, "uploadDir").found().is_some();
        let keep_ext = get_property(ctx, opts, "keepExtensions")
            .value()
            .is_some_and(bool_value);
        report(ctx, upload_dir, keep_ext, callee, None);
    }

    fn check_multer<'a>(&self, ctx: &Ctx<'a, '_>, callee: Span, args: &[Argument<'a>]) {
        let Some(opts) = args
            .first()
            .and_then(|a| a.as_expression())
            .and_then(|e| resolve_object(ctx, e))
        else {
            return;
        };
        let Some(storage) = get_property(ctx, opts, "storage").value() else {
            return;
        };
        let Expression::CallExpression(c) = resolve_value(ctx, storage) else {
            return;
        };
        let Expression::StaticMemberExpression(m) = strip_parens(&c.callee) else {
            return;
        };
        if m.property.name != "diskStorage" {
            return;
        }
        let Some(storage_opts) = c
            .arguments
            .first()
            .and_then(|a| a.as_expression())
            .and_then(|e| resolve_object(ctx, e))
        else {
            return;
        };
        if matches!(
            get_property(ctx, storage_opts, "destination"),
            Prop::Missing
        ) {
            report(
                ctx,
                false,
                false,
                callee,
                Some(sec(m.span, Some("no destination specified"))),
            );
        }
    }
}

impl JsRule for Rule {
    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {
        if !src_mentions(ctx, &["formidable", "multer"]) {
            return;
        }
        for node in ctx.nodes().iter() {
            match node.kind() {
                AstKind::CallExpression(c) => self.check(ctx, node, &c.callee, &c.arguments),
                AstKind::NewExpression(n) => self.check(ctx, node, &n.callee, &n.arguments),
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn file_uploads() {
        check(
            "S2598",
            "a.js",
            r#"
const formidable = require('formidable');
const form1 = formidable(); // Noncompliant {{Restrict folder destination of uploaded files.}}
const form2 = formidable({ uploadDir: '/uploads' });
const form3 = formidable({ keepExtensions: true }); // Noncompliant {{Restrict the extension and folder destination of uploaded files.}}
const form4 = formidable({ uploadDir: '/uploads', keepExtensions: true }); // Noncompliant {{Restrict the extension of uploaded files.}}
const form5 = formidable();
form5.uploadDir = '/uploads';
const { IncomingForm } = require('formidable');
const form6 = new IncomingForm(); // Noncompliant
form6.keepExtensions = false;
const multer = require('multer');
const storage = multer.diskStorage({ filename: f });
const upload = multer({ storage: storage }); // Noncompliant {{Restrict folder destination of uploaded files.}}
const upload2 = multer({ storage: multer.diskStorage({ destination: d }) });
"#,
        );
    }
}
