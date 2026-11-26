//! Single-threaded throughput of `uc_css::analyze` over the stylesheets under the given
//! directories: `cargo run --profile fast -p uc-css --example bench -- <dir>...`.

use std::time::Instant;

fn walk(dir: &std::path::Path, out: &mut Vec<(String, String)>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        let name = e.file_name();
        let name = name.to_string_lossy();
        if p.is_dir() {
            if name != "node_modules" && !name.starts_with('.') {
                walk(&p, out);
            }
        } else if uc_css::is_css_path(&name)
            && !name.ends_with(".min.css")
            && let Ok(s) = std::fs::read_to_string(&p)
        {
            out.push((p.to_string_lossy().into_owned(), s));
        }
    }
}

fn main() {
    let mut files = Vec::new();
    for d in std::env::args().skip(1) {
        walk(std::path::Path::new(&d), &mut files);
    }
    let bytes: usize = files.iter().map(|f| f.1.len()).sum();
    let rules = uc_core::RuleSet::all(uc_core::Repo::Css, uc_css::IMPLEMENTED);
    let mut best = f64::MAX;
    let mut issues = 0;
    let iters: usize = std::env::var("ITER").ok().and_then(|v| v.parse().ok()).unwrap_or(5);
    for _ in 0..iters {
        let t = Instant::now();
        issues = 0;
        for (p, s) in &files {
            issues += uc_css::analyze(p, s, false, &rules).issues.len();
        }
        best = best.min(t.elapsed().as_secs_f64());
    }
    println!(
        "{} files, {:.2} MB, {issues} issues, best {:.1} ms, {:.0} MB/s",
        files.len(),
        bytes as f64 / 1e6,
        best * 1e3,
        bytes as f64 / 1e6 / best
    );
    if std::env::var_os("SLOWEST").is_some() {
        let mut t: Vec<(f64, usize, &str)> = files
            .iter()
            .map(|(p, s)| {
                let mut best = f64::MAX;
                for _ in 0..5 {
                    let t = Instant::now();
                    uc_css::analyze(p, s, false, &rules);
                    best = best.min(t.elapsed().as_secs_f64() * 1e3);
                }
                (best, s.len(), p.as_str())
            })
            .collect();
        t.sort_by(|a, b| b.0.total_cmp(&a.0));
        for (ms, len, p) in t.iter().take(10) {
            println!("{ms:8.2} ms {len:9} B  {p}");
        }
    }
}
