//! Generates the rule registry from `src/rules/s*.rs` (SonarJS keys) and `src/rules/uj*.rs`
//! (extra rules): each file defines `pub struct Rule` with `fn new(&Params) -> Self` and an
//! `impl JsRule`. The file name gives the rule key.
use std::{env, fs, path::Path};

fn main() {
    let dir = Path::new(&env::var("CARGO_MANIFEST_DIR").unwrap()).join("src/rules");
    println!("cargo:rerun-if-changed={}", dir.display());
    // (module name, rule key, sort key)
    let mut names: Vec<(String, String, (u8, u32))> = fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| {
            let name = e.ok()?.file_name().into_string().ok()?;
            let stem = name.strip_suffix(".rs")?;
            let (prefix, num, order) =
                if let Some(n) = stem.strip_prefix("uj") { ("UJ", n, 1) } else { ("S", stem.strip_prefix('s')?, 0) };
            (!num.is_empty() && num.chars().all(|c| c.is_ascii_digit()))
                .then(|| (stem.to_string(), format!("{prefix}{num}"), (order, num.parse().unwrap())))
        })
        .collect();
    names.sort_by_key(|n| n.2);
    let mut out = String::new();
    for (n, _, _) in &names {
        out += &format!("#[path = {:?}]\npub mod {n};\n", dir.join(format!("{n}.rs")).display().to_string());
    }
    out += "/// Constructor of a rule from its parameters.\npub type Ctor = fn(&uc_core::Params) -> Box<dyn crate::JsRule>;\npub static REGISTRY: &[(&str, Ctor)] = &[\n";
    for (n, key, _) in &names {
        out += &format!("    (\"{key}\", |p| Box::new({n}::Rule::new(p))),\n");
    }
    out += "];\n";
    fs::write(Path::new(&env::var("OUT_DIR").unwrap()).join("registry.rs"), out).unwrap();
}
