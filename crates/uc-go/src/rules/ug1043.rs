//! Exported functions should not return unexported types (revive unexported-return).
use super::ug1041::receiver;
use crate::helpers_extra::*;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

fn is_exported(name: &str) -> bool {
    name.starts_with(|c: char| c.is_uppercase())
}

impl GoRule for Rule {
    fn run_once(&self, ctx: &Ctx<'_>) {
        if package_name(ctx) == Some("main") {
            return;
        }
        let tops = ctx.named_children(ctx.root);
        let mut types: FxHashSet<&str> = FxHashSet::default();
        for t in tops.iter().filter(|t| t.kind() == "type_declaration") {
            for spec in ctx.named_children(*t) {
                if matches!(spec.kind(), "type_spec" | "type_alias")
                    && let Some(n) = spec.child_by_field_name("name")
                {
                    types.insert(ctx.text(n));
                }
            }
        }
        for f in &tops {
            if !matches!(f.kind(), "function_declaration" | "method_declaration") {
                continue;
            }
            let Some(name) = f.child_by_field_name("name").map(|n| ctx.text(n)) else {
                continue;
            };
            if !is_exported(name) {
                continue;
            }
            if f.kind() == "method_declaration"
                && !receiver(ctx, *f).is_some_and(|(_, t)| is_exported(t))
            {
                continue;
            }
            let Some(result) = f.child_by_field_name("result") else {
                continue;
            };
            let result_types: Vec<Nd<'_>> = if result.kind() == "parameter_list" {
                param_decls(result)
                    .iter()
                    .filter_map(|p| p.child_by_field_name("type"))
                    .collect()
            } else {
                vec![result]
            };
            for t in result_types {
                let base = if t.kind() == "pointer_type" {
                    t.named_child(0).unwrap_or(t)
                } else {
                    t
                };
                if base.kind() != "type_identifier" {
                    continue;
                }
                let tn = ctx.text(base);
                if !is_exported(tn) && types.contains(tn) {
                    ctx.report(
                        t,
                        format!("Exported function \"{name}\" returns unexported type \"{tn}\"; export the type or return an interface."),
                    );
                }
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
            "UG1043",
            r#"package lib

type client struct{}
type Server struct{}
type handler func()

func NewClient() *client { return nil } // Noncompliant {{Exported function "NewClient" returns unexported type "client"; export the type or return an interface.}}
func Make() (handler, error) { return nil, nil } // Noncompliant
func NewServer() *Server { return nil }
func newClient() *client { return nil }
func Count() int { return 0 }
func (s *Server) Client() client { return client{} } // Noncompliant
func (c *client) Self() *client { return c }
"#,
        );
        check(
            "UG1043",
            r#"package main

type client struct{}

func NewClient() *client { return nil }
"#,
        );
    }
}
