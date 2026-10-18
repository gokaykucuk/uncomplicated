//! Tables should have headers
use crate::helpers::batch_a11y::*;
use crate::prelude::*;
use crate::rules::s5257::is_presentation_table;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

fn is_valid_table(table: &JSXElement<'_>) -> bool {
    let Some(grid) = compute_grid(table) else {
        return true;
    };
    let Some(first) = grid.first() else {
        return false;
    };
    if grid.iter().any(|row| row.iter().all(|c| c.is_header)) {
        return true;
    }
    (0..first.len()).any(|col| {
        grid.iter()
            .all(|row| row.get(col).is_none_or(|c| c.is_header))
    })
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::JSXElement]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::JSXElement(table) = node.kind() else {
            return;
        };
        let el = &table.opening_element;
        if element_name(el) != "table" || is_presentation_table(el) {
            return;
        }
        if literal_value_of(el, "aria-hidden") == Val::Bool(true) {
            return;
        }
        // Reusable wrapper components receive their rows through spread props.
        if table.children.is_empty() && has_spread(el) {
            return;
        }
        if !is_valid_table(table) {
            ctx.report(
                table.span,
                "Add a valid header row or column to this \"<table>\".",
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn table_header() {
        check(
            "S5256",
            "a.jsx",
            r##"
<table className="data"><tbody>{rows}</tbody></table>;
<table className="data"><Header /><tr><th></th></tr></table>;
<table> {/* Noncompliant {{Add a valid header row or column to this "<table>".}} */}
</table>;
<table> {/* Noncompliant */}
  <th></th>
</table>;
<table><tr><th></th></tr></table>;
<table><thead><tr><th></th></tr></thead></table>;
<table> {/* Noncompliant */}
  <table><tr><th></th></tr></table>
</table>;
<table role="presentation"></table>;
<table role="None"></table>;
<table role="dummy"></table>; // Noncompliant
<table aria-hidden="true"></table>;
<table aria-hidden="false"></table>; // Noncompliant
<table> {/* Noncompliant */}
  <tbody>
    <tr><td></td><td></td></tr>
    <tr><td></td><th></th></tr>
  </tbody>
</table>;
<table>
  <tbody>
    <tr><td></td><td colspan="2" rowspan="2"></td><th colspan="3" rowspan="3"></th></tr>
    <tr><td></td></tr>
    <tr><th colspan="3"></th></tr>
  </tbody>
</table>;
<table> {/* Noncompliant */}
  <tr><th colspan={2} rowspan="2"></th><td rowspan="2"></td></tr>
  <tr><td colspan="3"></td></tr>
</table>;
<table>
  <tr><th rowspan="0">ID</th><th>Name</th></tr>
  <tr><td>1</td></tr>
</table>;
<table>
  <tr><th>A</th><td>1</td></tr>
  <tr><th>B</th><td>2</td></tr>
</table>;
<table {...props} />;
<table className="x" {...props}></table>;
<table {...props}> {/* Noncompliant */}
  <tbody><tr><td>Data</td></tr></tbody>
</table>;
<Table></Table>;
"##,
        );
    }
}
