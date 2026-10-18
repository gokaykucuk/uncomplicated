//! Table cells should reference their headers
use crate::helpers::batch_a11y::*;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

struct Block<'g, 'a> {
    min_row: usize,
    max_row: usize,
    min_col: usize,
    max_col: usize,
    cell: &'g TableCell<'a>,
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::JSXElement]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::JSXElement(table) = node.kind() else {
            return;
        };
        if element_name(&table.opening_element) != "table" {
            return;
        }
        let Some(grid) = compute_grid(table) else {
            return;
        };
        let Some(first) = grid.first() else { return };
        let header_id = |c: &TableCell<'a>| {
            if c.is_header {
                c.id.clone().filter(|id| !id.is_empty())
            } else {
                None
            }
        };
        let row_headers: Vec<FxHashSet<String>> = grid
            .iter()
            .map(|row| row.iter().filter_map(header_id).collect())
            .collect();
        let col_headers: Vec<FxHashSet<String>> = (0..first.len())
            .map(|col| {
                grid.iter()
                    .filter_map(|row| row.get(col))
                    .filter_map(header_id)
                    .collect()
            })
            .collect();
        let all_headers: FxHashSet<&String> =
            row_headers.iter().chain(&col_headers).flatten().collect();

        // Positions covered by each source cell carrying `headers`, in order of first appearance.
        let mut blocks: Vec<Block<'_, 'a>> = Vec::new();
        let mut index: FxHashMap<usize, usize> = FxHashMap::default();
        for (r, row) in grid.iter().enumerate() {
            for (c, cell) in row.iter().enumerate() {
                if cell.headers.is_none() {
                    continue;
                }
                match index.get(&cell.internal_id) {
                    Some(&i) => {
                        blocks[i].max_row = r;
                        blocks[i].max_col = c;
                    }
                    None => {
                        index.insert(cell.internal_id, blocks.len());
                        blocks.push(Block {
                            min_row: r,
                            max_row: r,
                            min_col: c,
                            max_col: c,
                            cell,
                        });
                    }
                }
            }
        }
        for b in &blocks {
            let Some(headers) = &b.cell.headers else {
                continue;
            };
            let in_scope = |h: &String| {
                col_headers
                    .get(b.min_col..=b.max_col)
                    .is_some_and(|s| s.iter().any(|set| set.contains(h)))
                    || row_headers
                        .get(b.min_row..=b.max_row)
                        .is_some_and(|s| s.iter().any(|set| set.contains(h)))
            };
            for h in headers {
                if in_scope(h) {
                    continue;
                }
                let message = if all_headers.contains(h) {
                    format!(
                        "id \"{h}\" in \"headers\" references the header of another column/row."
                    )
                } else {
                    format!("id \"{h}\" in \"headers\" does not reference any <th> header.")
                };
                ctx.report(b.cell.node.span, message);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn header_references() {
        check(
            "S5260",
            "a.jsx",
            r##"
<table></table>;
<table><tr><td></td></tr></table>;
<table>
  <tr><th id="foo"></th></tr>
  <tr><td headers="foo"></td></tr>
</table>;
<table>
  <tr><th id="foo"></th></tr>
  <tr>
    <td headers="bar"></td> {/* Noncompliant {{id "bar" in "headers" does not reference any <th> header.}} */}
  </tr>
</table>;
<table>
  <tr><td headers="foo"></td><th id="foo"></th></tr>
</table>;
<table>
  <tr><th id="foo"></th><th id="bar"></th></tr>
  <tr>
    <td headers="bar"></td> {/* Noncompliant {{id "bar" in "headers" references the header of another column/row.}} */}
    <td headers="foo"></td> {/* Noncompliant {{id "foo" in "headers" references the header of another column/row.}} */}
  </tr>
</table>;
<table>
  <tr><th id="foo"></th><td headers="foo"></td></tr>
  <tr>
    <th id="bar"></th>
    <td headers="foo"></td> {/* Noncompliant {{id "foo" in "headers" references the header of another column/row.}} */}
  </tr>
</table>;
<table>
  <tr><th colspan="2" id="foo"></th></tr>
  <tr><td></td><td headers="foo"></td></tr>
</table>;
<table>
  <tr><th colspan="2" id="foo"></th></tr>
  <tr><td headers="foo" colspan="2"></td></tr>
</table>;
<table>
  <tr><th rowspan="2" id="foo"></th></tr>
  <tr><td headers="foo"></td></tr>
</table>;
<table>
  <tr><th id="a"></th><th id="b"></th></tr>
  <tr>
    <td headers="a b"></td> {/* Noncompliant {{id "b" in "headers" references the header of another column/row.}} */}
    <td headers="b"></td>
  </tr>
</table>;
<table>
  <tr><th id="foo"></th></tr>
  {rows}
</table>;
"##,
        );
    }
}
