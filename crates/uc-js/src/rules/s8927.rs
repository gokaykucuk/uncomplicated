//! Default imports from modular utility libraries should not be used
use crate::helpers::batch_smell4::{is_unbounded_range, min_version};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// Library → (minimum version supporting named / method imports, method import example).
type Version = (u64, u64, u64);

fn recommendation(library: &str) -> Option<(Version, Option<&'static str>)> {
    Some(match library {
        "lodash" => ((4, 0, 0), Some("lodash/map")),
        "lodash-es" => ((4, 0, 0), None),
        "rxjs" => ((7, 2, 0), None),
        "rambda" => ((0, 8, 8), None),
        "validator" => ((12, 1, 0), Some("validator/es/lib/isEmail")),
        _ => return None,
    })
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::ImportDeclaration]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::ImportDeclaration(decl) = node.kind() else {
            return;
        };
        if decl.import_kind.is_type() {
            return;
        }
        let Some(specifiers) = &decl.specifiers else {
            return;
        };
        let has_default = specifiers
            .iter()
            .any(|s| matches!(s, ImportDeclarationSpecifier::ImportDefaultSpecifier(_)));
        let has_namespace = specifiers
            .iter()
            .any(|s| matches!(s, ImportDeclarationSpecifier::ImportNamespaceSpecifier(_)));
        if !has_default || has_namespace {
            return;
        }
        let library = decl.source.value.as_str();
        let Some((minimum, example)) = recommendation(library) else {
            return;
        };
        // Only when the project declares a version of the library that supports the alternative.
        let Some(range) = ctx.file.project.deps.get(library) else {
            return;
        };
        if !is_unbounded_range(range) && !min_version(range).is_some_and(|v| v >= minimum) {
            return;
        }
        let msg = match example {
            Some(example) => format!(
                "Import {library} methods from subpaths such as \"{example}\" to avoid importing more code than needed."
            ),
            None => format!("Use named imports from \"{library}\" instead of a default import."),
        };
        ctx.report(decl.span, msg);
    }
}

#[cfg(test)]
mod tests {
    use crate::helpers::batch_smell4::{check_project, project_with};

    #[test]
    fn rule() {
        let supported = project_with(&[
            ("lodash", "^4.17.21"),
            ("lodash-es", "4.17.21"),
            ("rxjs", "~7.8.0"),
            ("rambda", "*"),
            ("validator", "^13.0.0"),
        ]);
        check_project(
            "S8927",
            "a.js",
            &supported,
            r#"
import lodash from 'lodash'; // Noncompliant {{Import lodash methods from subpaths such as "lodash/map" to avoid importing more code than needed.}}
import lodashEs from 'lodash-es'; // Noncompliant {{Use named imports from "lodash-es" instead of a default import.}}
import rxjs from 'rxjs'; // Noncompliant
import R from 'rambda'; // Noncompliant
import validator, { isEmail } from 'validator'; // Noncompliant
import map from 'lodash/map';
import { filter } from 'lodash-es';
import * as Rx from 'rxjs';
import def, * as ns from 'rxjs';
import other from 'other';
"#,
        );
        let old = project_with(&[("lodash", "^3.10.0"), ("rxjs", "^6.0.0")]);
        check_project(
            "S8927",
            "a.js",
            &old,
            r#"
import lodash from 'lodash';
import rxjs from 'rxjs';
import lodashEs from 'lodash-es';
"#,
        );
    }
}
