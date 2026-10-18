//! Receiver names should be consistent across methods (staticcheck ST1016).
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// `(receiver name node, base type name)` of a method.
pub(crate) fn receiver<'t>(ctx: &Ctx<'t>, m: Nd<'t>) -> Option<(Nd<'t>, &'t str)> {
    let list = m.child_by_field_name("receiver")?;
    let p = ctx
        .named_children(list)
        .into_iter()
        .find(|p| p.kind() == "parameter_declaration")?;
    let name = p.child_by_field_name("name")?;
    let mut t = p.child_by_field_name("type")?;
    loop {
        match t.kind() {
            "pointer_type" => t = t.named_child(0)?,
            "generic_type" => t = t.child_by_field_name("type")?,
            "parenthesized_type" => t = t.named_child(0)?,
            _ => break,
        }
    }
    (t.kind() == "type_identifier").then(|| (name, ctx.text(t)))
}

impl GoRule for Rule {
    fn run_once(&self, ctx: &Ctx<'_>) {
        let mut first: FxHashMap<&str, &str> = FxHashMap::default();
        for m in ctx.named_children(ctx.root) {
            if m.kind() != "method_declaration" {
                continue;
            }
            let Some((name, ty)) = receiver(ctx, m) else {
                continue;
            };
            let n = ctx.text(name);
            if n == "_" {
                continue;
            }
            match first.get(ty) {
                None => {
                    first.insert(ty, n);
                }
                Some(&prev) if prev != n => {
                    ctx.report(
                        name,
                        format!("Receiver name \"{n}\" should be consistent with \"{prev}\" used by other methods of \"{ty}\"."),
                    );
                }
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn rule() {
        check(
            "UG1041",
            r#"package p

type Server struct{}
type Client[T any] struct{}

func (s *Server) Start() {}
func (s Server) Name() string { return "" }
func (srv *Server) Stop() {} // Noncompliant {{Receiver name "srv" should be consistent with "s" used by other methods of "Server".}}
func (_ *Server) Ignore() {}
func (*Server) Anon() {}

func (c *Client[T]) Get() {}
func (cl *Client[T]) Put() {} // Noncompliant
"#,
        );
    }
}
