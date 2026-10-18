//! Generates the rule registry from `src/rules/s*.rs`: each file defines `pub struct Rule` with
//! `fn new(&Params) -> Self` and an `impl JsRule`. The file name gives the rule key.
use std::{env, fs, path::Path};

fn main() {
    let dir = Path::new(&env::var("CARGO_MANIFEST_DIR").unwrap()).join("src/rules");
    println!("cargo:rerun-if-changed={}", dir.display());
    let mut names: Vec<String> = fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| {
            let name = e.ok()?.file_name().into_string().ok()?;
            let stem = name.strip_suffix(".rs")?;
            (stem.starts_with('s') && stem[1..].chars().all(|c| c.is_ascii_digit())).then(|| stem.to_string())
        })
        .collect();
    names.sort_by_key(|n| n[1..].parse::<u32>().unwrap());
    let mut out = String::new();
    for n in &names {
        out += &format!("#[path = {:?}]\npub mod {n};\n", dir.join(format!("{n}.rs")).display().to_string());
    }
    out += "pub static REGISTRY: &[(&str, fn(&uc_core::Params) -> Box<dyn crate::JsRule>)] = &[\n";
    for n in &names {
        out += &format!("    (\"S{}\", |p| Box::new({n}::Rule::new(p))),\n", &n[1..]);
    }
    out += "];\n";
    fs::write(Path::new(&env::var("OUT_DIR").unwrap()).join("registry.rs"), out).unwrap();
}
