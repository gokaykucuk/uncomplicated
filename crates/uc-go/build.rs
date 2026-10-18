//! Generates the Go rule registry from `src/rules/s*.rs` and `src/rules/ug*.rs` (extra rules).
use std::{env, fs, path::Path};

fn main() {
    let dir = Path::new(&env::var("CARGO_MANIFEST_DIR").unwrap()).join("src/rules");
    println!("cargo:rerun-if-changed={}", dir.display());
    let mut names: Vec<(String, String)> = fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| {
            let name = e.ok()?.file_name().into_string().ok()?;
            let stem = name.strip_suffix(".rs")?;
            let (prefix, num) = if let Some(n) = stem.strip_prefix("ug") { ("UG", n) } else { ("S", stem.strip_prefix('s')?) };
            (!num.is_empty() && num.chars().all(|c| c.is_ascii_digit())).then(|| (stem.to_string(), format!("{prefix}{num}")))
        })
        .collect();
    names.sort();
    let mut out = String::new();
    for (n, _) in &names {
        out += &format!("#[path = {:?}]\npub mod {n};\n", dir.join(format!("{n}.rs")).display().to_string());
    }
    out += "pub static REGISTRY: &[(&str, fn(&uc_core::Params) -> Box<dyn crate::GoRule>)] = &[\n";
    for (n, key) in &names {
        out += &format!("    (\"{key}\", |p| Box::new({n}::Rule::new(p))),\n");
    }
    out += "];\n";
    fs::write(Path::new(&env::var("OUT_DIR").unwrap()).join("registry.rs"), out).unwrap();
}
