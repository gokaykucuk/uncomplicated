//! Character classes in regular expressions should not contain the same character twice
use oxc_regular_expression::ast::{
    CharacterClass, CharacterClassContents, CharacterClassEscapeKind,
};

use crate::helpers::batch_regex::{INTERESTS, Node, for_each_regex, walk};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const MAX: u32 = 0x10ffff;

fn upper(c: u32) -> u32 {
    char::from_u32(c)
        .and_then(|ch| ch.to_uppercase().next())
        .map_or(c, |u| u as u32)
}

fn lower(c: u32) -> u32 {
    char::from_u32(c)
        .and_then(|ch| ch.to_lowercase().next())
        .map_or(c, |u| u as u32)
}

/// Code point ranges matched by a class element (SonarJS' simplified model).
fn ranges(e: &CharacterClassContents<'_>, flags: RegExpFlags, out: &mut Vec<(u32, u32)>) {
    let unicode = flags.intersects(RegExpFlags::U | RegExpFlags::V);
    let mut folded = |from: u32, to: u32| {
        let (uf, ut) = (upper(from), upper(to));
        let (lf, lt) = (lower(uf), lower(ut));
        if flags.contains(RegExpFlags::I)
            && lf != uf
            && lt != ut
            && ((from < 128 && to < 128) || unicode)
        {
            out.push((uf, ut));
            out.push((lf, lt));
        } else {
            out.push((from, to));
        }
    };
    match e {
        CharacterClassContents::Character(c) => folded(c.value, c.value),
        CharacterClassContents::CharacterClassRange(r) => folded(r.min.value, r.max.value),
        CharacterClassContents::CharacterClassEscape(esc) => {
            use CharacterClassEscapeKind as K;
            let c = |ch: char| ch as u32;
            match esc.kind {
                K::D => out.push((c('0'), c('9'))),
                K::NegativeD => {
                    out.push((0, c('0') - 1));
                    out.push((c('9') + 1, if unicode { 0xff } else { MAX }));
                }
                K::S => {
                    out.extend([(c('\t'), c('\r')), (c(' '), c(' '))]);
                    if unicode {
                        out.extend([
                            (0x85, 0x85),
                            (0xa0, 0xa0),
                            (0x1680, 0x1680),
                            (0x2000, 0x200a),
                            (0x2028, 0x2029),
                            (0x202f, 0x202f),
                            (0x205f, 0x205f),
                            (0x3000, 0x3000),
                        ]);
                    }
                }
                K::NegativeS => {
                    out.extend([(0, c('\t') - 1), (c('\r') + 1, c(' ') - 1)]);
                    if unicode {
                        out.extend([
                            (c(' ') + 1, 0x84),
                            (0x86, 0x9f),
                            (0xa1, 0x167f),
                            (0x1681, 0x1fff),
                            (0x200b, 0x2027),
                            (0x202a, 0x202e),
                            (0x2030, 0x205e),
                            (0x2060, 0x2fff),
                            (0x3001, MAX),
                        ]);
                    } else {
                        out.push((c(' ') + 1, MAX));
                    }
                }
                K::W => out.extend([
                    (c('0'), c('9')),
                    (c('A'), c('Z')),
                    (c('_'), c('_')),
                    (c('a'), c('z')),
                ]),
                K::NegativeW => {
                    out.extend([
                        (0, c('0') - 1),
                        (c('9') + 1, c('A') - 1),
                        (c('Z') + 1, c('_') - 1),
                        (c('`'), c('`')),
                    ]);
                    out.push((c('z') + 1, if unicode { c('µ') - 1 } else { MAX }));
                }
            }
        }
        _ => {}
    }
}

fn intersects(a: &[(u32, u32)], b: &[(u32, u32)]) -> bool {
    a.iter()
        .any(|&(f1, t1)| b.iter().any(|&(f2, t2)| f1 <= t2 && f2 <= t1))
}

/// Indices of duplicate elements, in SonarJS' reporting order.
fn duplicates(c: &CharacterClass<'_>, flags: RegExpFlags) -> Vec<usize> {
    let sets: Vec<Vec<(u32, u32)>> = c
        .body
        .iter()
        .map(|e| {
            let mut v = Vec::new();
            ranges(e, flags, &mut v);
            v
        })
        .collect();
    let mut dups: Vec<usize> = Vec::new();
    for i in 1..sets.len() {
        if sets[i].is_empty() {
            continue;
        }
        let mut found = false;
        for j in 0..i {
            if intersects(&sets[i], &sets[j]) {
                found = true;
                if !dups.contains(&j) {
                    dups.push(j);
                }
            }
        }
        if found && !dups.contains(&i) {
            dups.push(i);
        }
    }
    dups
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        INTERESTS
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        for_each_regex(node, ctx, |re| {
            walk(re.pattern, &mut |n, enter| {
                let Node::Class(c) = n else { return };
                if !enter {
                    return;
                }
                let dups = duplicates(c, re.flags);
                let Some((&first, rest)) = dups.split_first() else {
                    return;
                };
                let secondary: Vec<(Span, Option<String>)> = rest
                    .iter()
                    .filter_map(|&i| re.loc(c.body[i].span()))
                    .map(|s| (s, Some("Additional duplicate".to_string())))
                    .collect();
                ctx.report_with(
                    re.loc_or_node(c.body[first].span()),
                    "Remove duplicates in this character class.",
                    secondary,
                );
            });
        });
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn duplicates() {
        check(
            "S5869",
            "a.js",
            r#"
/[0-99]/;                 // Noncompliant {{Remove duplicates in this character class.}}
/[0-9\d]/;                // Noncompliant
/[a-zA-Z]/i;              // Noncompliant
/[aa]/;                   // Noncompliant
/[\w_]/;                  // Noncompliant
/[a-c][b-d]/;
/[a-zA-Z]/;
/[0-9a-f]/;
/[\s\S]/;
new RegExp("[xx]");       // Noncompliant
"#,
        );
    }
}
