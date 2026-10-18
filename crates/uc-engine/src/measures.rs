//! Project measures with SonarQube metric keys and definitions.

use std::collections::BTreeMap;

use uc_core::{Catalog, IssueType, Lang, Severity};

use crate::coverage::Coverage;
use crate::cpd::FileDuplication;
use crate::scm::NewCode;
use crate::{AnalyzedFile, rule_meta};

pub type Measures = BTreeMap<&'static str, f64>;

/// Sonar's default development cost: 30 minutes per line of code.
const DEV_COST_PER_LINE: f64 = 30.0;

fn severity_rating(worst: Option<Severity>) -> f64 {
    match worst {
        None | Some(Severity::Info) => 1.0,
        Some(Severity::Minor) => 2.0,
        Some(Severity::Major) => 3.0,
        Some(Severity::Critical) => 4.0,
        Some(Severity::Blocker) => 5.0,
    }
}

fn debt_rating(ratio: f64) -> f64 {
    match ratio {
        r if r <= 5.0 => 1.0,
        r if r <= 10.0 => 2.0,
        r if r <= 20.0 => 3.0,
        r if r <= 50.0 => 4.0,
        _ => 5.0,
    }
}

pub fn rating_letter(v: f64) -> &'static str {
    match v.round() as i64 {
        1 => "A",
        2 => "B",
        3 => "C",
        4 => "D",
        _ => "E",
    }
}

#[derive(Default)]
struct Acc {
    files: u64,
    lines: u64,
    ncloc: u64,
    comment_lines: u64,
    functions: u64,
    classes: u64,
    statements: u64,
    complexity: u64,
    cognitive: u64,
    bugs: u64,
    vulns: u64,
    smells: u64,
    hotspots: u64,
    worst_bug: Option<Severity>,
    worst_vuln: Option<Severity>,
    debt: u64,
    by_severity: [u64; 5],
    lines_to_cover: u64,
    uncovered_lines: u64,
    conditions: u64,
    uncovered_conditions: u64,
    dup_lines: u64,
    dup_blocks: u64,
    dup_files: u64,
}

/// Computes overall measures, and new-code measures when a new-code period is set.
pub fn compute(
    files: &[AnalyzedFile],
    dups: &[FileDuplication],
    cov: &Coverage,
    new: Option<&NewCode>,
) -> (Measures, Option<Measures>) {
    let catalog = Catalog::get();
    let mut all = Acc::default();
    let mut nw = Acc::default();
    let has_cov = !cov.is_empty();
    for (i, f) in files.iter().enumerate() {
        let r = &f.result;
        let m = &r.metrics;
        let is_new = |l: u32| new.is_some_and(|n| n.is_new(&r.path, l));
        let go = r.lang == Some(Lang::Go);

        all.files += 1;
        all.lines += m.lines as u64;
        all.ncloc += m.ncloc as u64;
        all.comment_lines += m.comment_lines as u64;
        all.functions += m.functions as u64;
        all.classes += m.classes as u64;
        all.statements += m.statements as u64;
        all.complexity += m.complexity as u64;
        all.cognitive += m.cognitive_complexity as u64;
        if new.is_some() {
            nw.ncloc += m.code_lines.iter().filter(|&&l| is_new(l)).count() as u64;
            nw.lines += (1..=m.lines).filter(|&l| is_new(l)).count() as u64;
        }

        for issue in &r.issues {
            let Some(meta) = rule_meta(catalog, &issue.rule, go) else { continue };
            let effort = meta.effort.minutes(issue.gap) as u64;
            let targets: &mut [&mut Acc] =
                if is_new(issue.loc.line) { &mut [&mut all, &mut nw] } else { &mut [&mut all] };
            for acc in targets.iter_mut() {
                acc.by_severity[meta.severity as usize] += 1;
                match meta.kind {
                    IssueType::Bug => {
                        acc.bugs += 1;
                        acc.worst_bug = acc.worst_bug.max(Some(meta.severity));
                    }
                    IssueType::Vulnerability => {
                        acc.vulns += 1;
                        acc.worst_vuln = acc.worst_vuln.max(Some(meta.severity));
                    }
                    IssueType::CodeSmell => {
                        acc.smells += 1;
                        acc.debt += effort;
                    }
                    IssueType::SecurityHotspot => acc.hotspots += 1,
                }
            }
        }

        // Coverage: only main code; files missing from reports count as fully uncovered.
        if has_cov && !r.is_test {
            match cov.files.get(&r.path) {
                Some(fc) => {
                    for (&l, &h) in &fc.lines {
                        let accs: &mut [&mut Acc] = if is_new(l) { &mut [&mut all, &mut nw] } else { &mut [&mut all] };
                        for a in accs.iter_mut() {
                            a.lines_to_cover += 1;
                            if h == 0 {
                                a.uncovered_lines += 1;
                            }
                        }
                    }
                    for (&l, &(t, c)) in &fc.branches {
                        let accs: &mut [&mut Acc] = if is_new(l) { &mut [&mut all, &mut nw] } else { &mut [&mut all] };
                        for a in accs.iter_mut() {
                            a.conditions += t as u64;
                            a.uncovered_conditions += (t - c.min(t)) as u64;
                        }
                    }
                }
                None => {
                    for &l in &m.executable_lines {
                        let accs: &mut [&mut Acc] = if is_new(l) { &mut [&mut all, &mut nw] } else { &mut [&mut all] };
                        for a in accs.iter_mut() {
                            a.lines_to_cover += 1;
                            a.uncovered_lines += 1;
                        }
                    }
                }
            }
        }

        if let Some(d) = dups.get(i)
            && !d.blocks.is_empty() {
                all.dup_files += 1;
                all.dup_blocks += d.blocks.len() as u64;
                all.dup_lines += d.lines.len() as u64;
                let new_dup = d.lines.iter().filter(|&&l| is_new(l)).count() as u64;
                nw.dup_lines += new_dup;
                nw.dup_blocks += d.blocks.iter().filter(|b| (b.start_line..=b.end_line).any(is_new)).count() as u64;
            }
    }
    let overall = finish(&all, has_cov, false);
    let new_m = new.map(|_| finish(&nw, has_cov, true));
    (overall, new_m)
}

fn finish(a: &Acc, has_cov: bool, new: bool) -> Measures {
    let mut m = Measures::new();
    let k = |overall: &'static str, newk: &'static str| if new { newk } else { overall };
    if !new {
        m.insert("files", a.files as f64);
        m.insert("lines", a.lines as f64);
        m.insert("ncloc", a.ncloc as f64);
        m.insert("comment_lines", a.comment_lines as f64);
        if a.ncloc + a.comment_lines > 0 {
            m.insert("comment_lines_density", 100.0 * a.comment_lines as f64 / (a.ncloc + a.comment_lines) as f64);
        }
        m.insert("functions", a.functions as f64);
        m.insert("classes", a.classes as f64);
        m.insert("statements", a.statements as f64);
        m.insert("complexity", a.complexity as f64);
        m.insert("cognitive_complexity", a.cognitive as f64);
    } else {
        m.insert("new_lines", a.ncloc as f64);
    }
    m.insert(k("bugs", "new_bugs"), a.bugs as f64);
    m.insert(k("vulnerabilities", "new_vulnerabilities"), a.vulns as f64);
    m.insert(k("code_smells", "new_code_smells"), a.smells as f64);
    m.insert(k("security_hotspots", "new_security_hotspots"), a.hotspots as f64);
    m.insert(k("violations", "new_violations"), (a.bugs + a.vulns + a.smells) as f64);
    for (sev, name) in [
        (Severity::Blocker, ("blocker_violations", "new_blocker_violations")),
        (Severity::Critical, ("critical_violations", "new_critical_violations")),
        (Severity::Major, ("major_violations", "new_major_violations")),
        (Severity::Minor, ("minor_violations", "new_minor_violations")),
        (Severity::Info, ("info_violations", "new_info_violations")),
    ] {
        m.insert(if new { name.1 } else { name.0 }, a.by_severity[sev as usize] as f64);
    }
    m.insert(k("reliability_rating", "new_reliability_rating"), severity_rating(a.worst_bug));
    m.insert(k("security_rating", "new_security_rating"), severity_rating(a.worst_vuln));
    m.insert(k("sqale_index", "new_technical_debt"), a.debt as f64);
    let ratio = if a.ncloc == 0 { 0.0 } else { 100.0 * a.debt as f64 / (a.ncloc as f64 * DEV_COST_PER_LINE) };
    m.insert(k("sqale_debt_ratio", "new_sqale_debt_ratio"), ratio);
    m.insert(k("sqale_rating", "new_maintainability_rating"), debt_rating(ratio));
    if has_cov {
        m.insert(k("lines_to_cover", "new_lines_to_cover"), a.lines_to_cover as f64);
        m.insert(k("uncovered_lines", "new_uncovered_lines"), a.uncovered_lines as f64);
        m.insert(k("conditions_to_cover", "new_conditions_to_cover"), a.conditions as f64);
        m.insert(k("uncovered_conditions", "new_uncovered_conditions"), a.uncovered_conditions as f64);
        let denom = (a.lines_to_cover + a.conditions) as f64;
        if denom > 0.0 {
            let covered = (a.lines_to_cover - a.uncovered_lines + a.conditions - a.uncovered_conditions) as f64;
            m.insert(k("coverage", "new_coverage"), 100.0 * covered / denom);
            if a.lines_to_cover > 0 {
                m.insert(
                    k("line_coverage", "new_line_coverage"),
                    100.0 * (a.lines_to_cover - a.uncovered_lines) as f64 / a.lines_to_cover as f64,
                );
            }
        }
    }
    m.insert(k("duplicated_lines", "new_duplicated_lines"), a.dup_lines as f64);
    m.insert(k("duplicated_blocks", "new_duplicated_blocks"), a.dup_blocks as f64);
    if !new {
        m.insert("duplicated_files", a.dup_files as f64);
    }
    let base = if new { a.ncloc } else { a.lines };
    if base > 0 {
        m.insert(k("duplicated_lines_density", "new_duplicated_lines_density"), 100.0 * a.dup_lines as f64 / base as f64);
    }
    m
}
