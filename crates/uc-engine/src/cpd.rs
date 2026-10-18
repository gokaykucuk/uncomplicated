//! Cross-file copy-paste detection over normalized token streams.
//!
//! Every window of `min_tokens` consecutive tokens is hashed (rolling hash); windows whose hash
//! occurs more than once are verified token by token, and all tokens they cover are marked
//! duplicated. Maximal runs of marked tokens spanning at least `min_lines` lines form blocks.

use rayon::prelude::*;
use serde::Serialize;
use uc_core::CpdToken;

#[derive(Debug, Clone, Serialize)]
pub struct Block {
    pub start_line: u32,
    pub end_line: u32,
    /// Other places with the same code: (file index, start line).
    pub partners: Vec<(usize, u32)>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct FileDuplication {
    /// Sorted, deduplicated duplicated lines.
    pub lines: Vec<u32>,
    pub blocks: Vec<Block>,
}

const BASE: u64 = 0x100_0000_01B3;

pub fn detect(files: &[&[CpdToken]], min_tokens: usize, min_lines: u32) -> Vec<FileDuplication> {
    let n = min_tokens.max(1);
    // BASE^(n-1) for removing the outgoing token from the rolling hash.
    let mut pow = 1u64;
    for _ in 1..n {
        pow = pow.wrapping_mul(BASE);
    }
    // (hash, file, position) for every window.
    let mut windows: Vec<(u64, u32, u32)> = files
        .par_iter()
        .enumerate()
        .flat_map_iter(|(fi, toks)| {
            let mut out = Vec::with_capacity(toks.len().saturating_sub(n) + 1);
            if toks.len() >= n {
                let mut h = 0u64;
                for t in &toks[..n] {
                    h = h.wrapping_mul(BASE).wrapping_add(t.hash as u64 + 1);
                }
                out.push((h, fi as u32, 0u32));
                for i in n..toks.len() {
                    h = h
                        .wrapping_sub((toks[i - n].hash as u64 + 1).wrapping_mul(pow))
                        .wrapping_mul(BASE)
                        .wrapping_add(toks[i].hash as u64 + 1);
                    out.push((h, fi as u32, (i - n + 1) as u32));
                }
            }
            out.into_iter()
        })
        .collect();
    windows.par_sort_unstable();

    // Marks per file: duplicated token flags, and first partner seen for each window start.
    let mut marked: Vec<Vec<bool>> = files.iter().map(|t| vec![false; t.len()]).collect();
    let mut partner_at: Vec<Vec<Option<(u32, u32)>>> = files.iter().map(|t| vec![None; t.len()]).collect();
    let same = |a: (u32, u32), b: (u32, u32)| {
        let ta = &files[a.0 as usize][a.1 as usize..a.1 as usize + n];
        let tb = &files[b.0 as usize][b.1 as usize..b.1 as usize + n];
        ta.iter().zip(tb).all(|(x, y)| x.hash == y.hash)
    };
    let mut i = 0;
    while i < windows.len() {
        let mut j = i + 1;
        while j < windows.len() && windows[j].0 == windows[i].0 {
            j += 1;
        }
        if j - i > 1 {
            let group = &windows[i..j];
            let first = (group[0].1, group[0].2);
            for (k, &(_, f, p)) in group.iter().enumerate() {
                let me = (f, p);
                // Overlapping windows in the same file (repetitive code) don't count against themselves.
                let partner = group
                    .iter()
                    .enumerate()
                    .find(|&(k2, &(_, f2, p2))| {
                        k2 != k && (f2 != f || (p2 as i64 - p as i64).unsigned_abs() as usize >= n) && same(me, (f2, p2))
                    })
                    .map(|(_, &(_, f2, p2))| (f2, p2));
                if let Some(partner) = partner {
                    let m = &mut marked[f as usize];
                    for t in &mut m[p as usize..p as usize + n] {
                        *t = true;
                    }
                    let slot = &mut partner_at[f as usize][p as usize];
                    if slot.is_none() {
                        *slot = Some(if partner == me { first } else { partner });
                    }
                }
                if j - i > 64 && k > 64 {
                    // Huge groups (generated tables): mark the rest without pairwise search.
                    for &(_, f, p) in &group[k + 1..] {
                        for t in &mut marked[f as usize][p as usize..p as usize + n] {
                            *t = true;
                        }
                    }
                    break;
                }
            }
        }
        i = j;
    }

    files
        .par_iter()
        .enumerate()
        .map(|(fi, toks)| {
            let m = &marked[fi];
            let mut dup = FileDuplication::default();
            let mut k = 0;
            while k < toks.len() {
                if !m[k] {
                    k += 1;
                    continue;
                }
                let start = k;
                while k < toks.len() && m[k] {
                    k += 1;
                }
                let (sl, el) = (toks[start].line, toks[k - 1].line);
                if el + 1 - sl >= min_lines {
                    let partners = partner_at[fi][start..k]
                        .iter()
                        .flatten()
                        .take(1)
                        .map(|&(f, p)| (f as usize, files[f as usize][p as usize].line))
                        .collect();
                    dup.blocks.push(Block { start_line: sl, end_line: el, partners });
                    dup.lines.extend(sl..=el);
                }
            }
            dup.lines.dedup();
            dup
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toks(src: &[u32], lines_per_tok: u32) -> Vec<CpdToken> {
        src.iter().enumerate().map(|(i, &h)| CpdToken { hash: h, line: 1 + i as u32 / lines_per_tok }).collect()
    }

    #[test]
    fn finds_cross_file_duplicate() {
        let shared: Vec<u32> = (0..200).collect();
        let mut a = vec![900, 901];
        a.extend(&shared);
        let mut b = shared.clone();
        b.extend([999]);
        let c: Vec<u32> = (1000..1300).collect();
        let (ta, tb, tc) = (toks(&a, 5), toks(&b, 5), toks(&c, 5));
        let res = detect(&[&ta, &tb, &tc], 100, 10);
        assert_eq!(res[0].blocks.len(), 1);
        assert_eq!(res[1].blocks.len(), 1);
        assert!(res[2].blocks.is_empty());
        assert_eq!(res[1].blocks[0].partners[0].0, 0);
    }
}
