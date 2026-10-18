//! Destructuring patterns should not be empty
use crate::prelude::*;

pub struct Rule {
    allow_object_params: bool,
}

impl Rule {
    pub fn new(p: &Params) -> Self {
        Self {
            allow_object_params: p.bool("allowObjectPatternsAsParameters", true),
        }
    }
}

const OBJ: &str = "Unexpected empty object pattern.";
const ARR: &str = "Unexpected empty array pattern.";

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[
            AstType::ObjectPattern,
            AstType::ArrayPattern,
            AstType::ObjectAssignmentTarget,
            AstType::ArrayAssignmentTarget,
        ]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        match node.kind() {
            AstKind::ObjectPattern(p) if p.properties.is_empty() && p.rest.is_none() => {
                if self.allowed_param(node, ctx) {
                    return;
                }
                ctx.report(p.span, OBJ);
            }
            AstKind::ArrayPattern(p) if p.elements.is_empty() && p.rest.is_none() => {
                ctx.report(p.span, ARR)
            }
            AstKind::ObjectAssignmentTarget(p) if p.properties.is_empty() && p.rest.is_none() => {
                ctx.report(p.span, OBJ)
            }
            AstKind::ArrayAssignmentTarget(p) if p.elements.is_empty() && p.rest.is_none() => {
                ctx.report(p.span, ARR)
            }
            _ => {}
        }
    }
}

impl Rule {
    fn allowed_param(&self, node: &AstNode<'_>, ctx: &Ctx<'_, '_>) -> bool {
        let mut parent = ctx.parent(node.id());
        let mut default: Option<&Expression<'_>> = None;
        if let AstKind::AssignmentPattern(a) = parent.kind() {
            default = Some(&a.right);
            parent = ctx.parent(parent.id());
        }
        let AstKind::FormalParameter(param) = parent.kind() else {
            return false;
        };
        if default.is_none() {
            default = param.initializer.as_deref();
        }
        let params = ctx.parent(parent.id());
        let owner = ctx.parent(params.id());
        // TypeScript signatures without a body never destructure anything.
        let ts_signature = match owner.kind() {
            AstKind::Function(f) => {
                matches!(
                    f.r#type,
                    FunctionType::TSDeclareFunction | FunctionType::TSEmptyBodyFunctionExpression
                ) || f.body.is_none()
            }
            AstKind::TSMethodSignature(_)
            | AstKind::TSCallSignatureDeclaration(_)
            | AstKind::TSConstructSignatureDeclaration(_)
            | AstKind::TSFunctionType(_)
            | AstKind::TSConstructorType(_) => true,
            _ => false,
        };
        if ts_signature {
            return true;
        }
        if !self.allow_object_params
            || !matches!(
                owner.kind(),
                AstKind::Function(_) | AstKind::ArrowFunctionExpression(_)
            )
        {
            return false;
        }
        match default.map(strip_parens) {
            None => true,
            Some(Expression::ObjectExpression(o)) => o.properties.is_empty(),
            Some(_) => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn empty_pattern() {
        check(
            "S3799",
            "a.js",
            r#"
let {a: {}} = myObj; // Noncompliant {{Unexpected empty object pattern.}}
let {b: []} = myObj; // Noncompliant {{Unexpected empty array pattern.}}
let {} = myObj; // Noncompliant
let [] = myArr; // Noncompliant
function foo([]) {} // Noncompliant
function bar({p: []}) {} // Noncompliant
function baz({} = {a: 1}) {} // Noncompliant
({} = x); // Noncompliant
let {c = {}} = myObj;
let {d: {e}} = myObj;
let [,] = myArr;
let {...rest} = myObj;
test('example', async ({}, testInfo) => { console.log(testInfo); });
function qux({}) {}
const fn = ({} = {}) => {};
"#,
        );
        check_with(
            "S3799",
            "a.js",
            serde_json::json!({"allowObjectPatternsAsParameters": false}),
            r#"
function qux({}) {} // Noncompliant
"#,
        );
    }

    #[test]
    fn ts_signatures() {
        check(
            "S3799",
            "a.ts",
            r#"
interface EmptyProps {}
interface Component { render({}: EmptyProps): void; }
abstract class AbstractComponent { abstract initialize({}: EmptyProps): void; }
type C = ({}: EmptyProps) => void;
declare function useHook({}: EmptyProps): void;
interface Factory { new({}: EmptyProps): object; }
interface Callable { ({}: EmptyProps): void; }
"#,
        );
    }
}
