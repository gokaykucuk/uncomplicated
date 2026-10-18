//! Error parameters in catch clauses should follow a consistent naming convention
use crate::helpers::batch_unicorn::is_method_call;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const EXPECTED: &str = "error";

fn ends_with_ci(name: &str, suffix: &str) -> bool {
    name.len() >= suffix.len() && name[name.len() - suffix.len()..].eq_ignore_ascii_case(suffix)
}

/// Sonar's default `ignore` patterns plus unicorn's own `name` / suffix checks.
fn is_allowed(name: &str) -> bool {
    if name == EXPECTED || name.ends_with("error") || name.ends_with("Error") {
        return true;
    }
    if name == "e" || name == "ex" || name.starts_with('_') {
        return true;
    }
    if ends_with_ci(name, "exception") || ends_with_ci(name, "err") {
        return true;
    }
    if name.eq_ignore_ascii_case("cause") || name.eq_ignore_ascii_case("reason") {
        return true;
    }
    // ^\w\$\d+$
    let b = name.as_bytes();
    b.len() >= 3
        && (b[0].is_ascii_alphanumeric() || b[0] == b'_')
        && b[1] == b'$'
        && b[2..].iter().all(u8::is_ascii_digit)
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::CatchParameter, AstType::CallExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let ident = match node.kind() {
            AstKind::CatchParameter(p) => match &p.pattern {
                BindingPattern::BindingIdentifier(id) => &**id,
                _ => return,
            },
            AstKind::CallExpression(call) => {
                let ok = is_method_call(call, &["then"], Some(2), true).is_some()
                    || is_method_call(call, &["catch"], Some(1), true).is_some();
                if !ok || call.optional {
                    return;
                }
                let Some(last) = call.arguments.last().and_then(|a| a.as_expression()) else {
                    return;
                };
                let params = match last {
                    Expression::ArrowFunctionExpression(f) => &f.params,
                    Expression::FunctionExpression(f) => &f.params,
                    _ => return,
                };
                match params.items.first().map(|p| &p.pattern) {
                    Some(BindingPattern::BindingIdentifier(id))
                        if params.items[0].initializer.is_none() =>
                    {
                        &**id
                    }
                    _ => return,
                }
            }
            _ => return,
        };
        let name = ident.name.as_str();
        if is_allowed(name) || is_allowed(name.trim_end_matches('_')) {
            return;
        }
        let Some(sym) = ident.symbol_id.get() else {
            return;
        };
        // Pick a name that doesn't clash with a visible binding, like unicorn's fixer does.
        let scoping = ctx.scoping();
        let mut scopes = vec![scoping.symbol_scope_id(sym)];
        scopes.extend(scoping.get_resolved_references(sym).map(|r| r.scope_id()));
        let mut fixed = EXPECTED.to_string();
        while scopes.iter().any(|s| {
            scoping
                .find_binding(*s, fixed.as_str().into())
                .is_some_and(|b| b != sym)
        }) {
            fixed.push('_');
        }
        ctx.report(
            ident.span,
            format!("The catch parameter `{name}` should be named `{fixed}`."),
        );
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn catch_error_name() {
        check(
            "S7718",
            "a.js",
            r#"
try { foo(); } catch (badName) { console.log(badName); } // Noncompliant {{The catch parameter `badName` should be named `error`.}}
try { foo(); } catch (x) { console.log(x); } // Noncompliant
try { foo(); } catch (err2) {} // Noncompliant
try { foo(); } catch (errors) {} // Noncompliant
promise.catch(resp => console.log(resp)); // Noncompliant
promise.then(res => res, function (rejected) {}); // Noncompliant
{
  const error = 1;
  try { foo(); } catch (value) { reject(value); } // Noncompliant {{The catch parameter `value` should be named `error_`.}}
}

try { foo(); } catch (error) {}
try { foo(); } catch (error_) {}
try { foo(); } catch (e) {}
try { foo(); } catch (ex) {}
try { foo(); } catch (myErr) {}
try { foo(); } catch (parseERR) {}
try { foo(); } catch (NetworkException) {}
try { foo(); } catch (fsError) {}
try { foo(); } catch (x$11) {}
try { foo(); } catch (_ignored) {}
try { foo(); } catch (cause) {}
try { foo(); } catch (reason) {}
try { foo(); } catch {}
try { foo(); } catch ({ message }) {}
promise.then(res => res);
promise.catch(handler);
promise.catch(error => 1);
promise.then(x => x, y => y, z => z);
"#,
        );
    }
}
