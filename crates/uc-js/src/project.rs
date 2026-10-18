//! Project context for JS/TS files: dependencies from the `package.json` files enclosing a file,
//! and compiler options from the nearest `tsconfig.json`. SonarJS enables several rules only
//! when a project depends on a given package (version), so rules need this.

use std::path::Path;

use rustc_hash::FxHashMap;
use serde_json::Value;

#[derive(Debug, Clone, Default)]
pub struct ProjectInfo {
    /// True if a `package.json` encloses the file.
    pub has_package_json: bool,
    /// Package name → version range, from dependencies/devDependencies/peerDependencies of every
    /// `package.json` between the file and the project root.
    pub deps: FxHashMap<String, String>,
    /// `"type"` of the nearest `package.json` (`"module"` / `"commonjs"`).
    pub module_type: Option<String>,
    /// `engines.node` of the nearest `package.json`.
    pub engines_node: Option<String>,
    /// `compilerOptions.lib` of the nearest `tsconfig.json` (lowercased).
    pub ts_lib: Vec<String>,
    pub ts_target: Option<String>,
    pub ts_module: Option<String>,
    /// Detected ECMAScript year for JS files (`None` = no limit), like SonarJS `detectedEsYear`.
    pub es_year_js: Option<u16>,
    /// Detected ECMAScript year for TS files.
    pub es_year_ts: Option<u16>,
    /// Year of the tsconfig `target` (ES5 = 2014), for rules whose syntax can't be down-levelled.
    pub ts_target_year: Option<u16>,
    /// Directory of the nearest `tsconfig.json` (absolute).
    pub tsconfig_dir: Option<std::path::PathBuf>,
    /// tsconfig `compilerOptions.baseUrl`, relative to `tsconfig_dir`.
    pub ts_base_url: Option<String>,
    /// tsconfig `compilerOptions.paths` patterns (e.g. `@utils/*`).
    pub ts_paths: Vec<String>,
    /// `name` of every `package.json` enclosing the file, nearest first.
    pub package_names: Vec<String>,
    /// `_moduleAliases` keys (module-alias package) of enclosing `package.json` files.
    pub module_aliases: Vec<String>,
    /// `workspaces` globs of enclosing `package.json` files.
    pub workspaces: Vec<String>,
    /// Catalog versions (pnpm/bun `catalog:` ranges) declared by enclosing `package.json` files.
    pub catalog: FxHashMap<String, String>,
}

impl ProjectInfo {
    /// Declared dependency; `@types/x` counts as a dependency on `x` (like SonarJS).
    pub fn has_dependency(&self, name: &str) -> bool {
        self.deps.contains_key(name) || (!name.starts_with("@types/") && self.deps.contains_key(&types_name(name)))
    }

    /// True if any dependency name starts with `prefix` (e.g. `@testing-library/`).
    pub fn has_dependency_prefix(&self, prefix: &str) -> bool {
        self.deps.keys().any(|k| k.starts_with(prefix))
    }

    /// Lowest major version allowed by a dependency's range (`^14.2.0` → 14), when parseable.
    pub fn dependency_major(&self, name: &str) -> Option<u64> {
        min_major(self.deps.get(name)?)
    }
}

const NODE_TO_ES: &[(u64, u16)] =
    &[(22, 2024), (20, 2023), (18, 2022), (16, 2021), (14, 2020), (12, 2019), (10, 2018), (8, 2017)];

fn node_major(v: &str) -> Option<u64> {
    if v == "*" || v == "latest" {
        return None;
    }
    v.split(|c: char| !c.is_ascii_digit()).filter_map(|n| n.parse::<u64>().ok()).filter(|n| (8..100).contains(n)).max()
}

fn target_year(t: &str, es5: u16) -> Option<u16> {
    let u = t.to_ascii_uppercase();
    match u.as_str() {
        "ESNEXT" | "JSON" => None,
        "ES3" | "ES5" => Some(es5),
        "ES6" => Some(2015),
        _ => u.strip_prefix("ES").and_then(|y| y.parse::<u16>().ok()).filter(|y| (2015..=2030).contains(y)),
    }
}

impl ProjectInfo {
    /// Computes the detected ES years from the collected signals (call after filling the fields).
    pub fn compute_es_years(&mut self) {
        let node = self
            .deps
            .get("@types/node")
            .filter(|v| v.as_str() != "*" && v.as_str() != "latest")
            .or(self.engines_node.as_ref())
            .and_then(|v| node_major(v))
            .map(|m| NODE_TO_ES.iter().find(|(n, _)| m >= *n).map_or(2017, |(_, y)| *y));
        self.es_year_js = node;
        self.es_year_ts = if !self.ts_lib.is_empty() {
            if self.ts_lib.iter().any(|l| l.starts_with("esnext")) {
                None
            } else {
                self.ts_lib
                    .iter()
                    .filter_map(|l| {
                        if l == "es6" || l.starts_with("es6.") {
                            Some(2015)
                        } else {
                            l.strip_prefix("es").and_then(|r| r.get(..4)).and_then(|y| y.parse::<u16>().ok())
                        }
                    })
                    .max()
            }
        } else {
            [self.ts_target.as_deref().and_then(|t| target_year(t, 2020)), node].into_iter().flatten().max()
        };
        self.ts_target_year = self.ts_target.as_deref().and_then(|t| target_year(t, 2014));
    }

    /// True if a rule requiring ES `year` applies to a file of this project.
    pub fn allows_es(&self, year: u16, is_ts: bool, downlevelable: Option<bool>, html: bool) -> bool {
        if html || (is_ts && downlevelable == Some(true)) {
            return true;
        }
        if is_ts && downlevelable == Some(false) && let Some(t) = self.ts_target_year {
            return year <= t;
        }
        let detected = if is_ts { self.es_year_ts } else { self.es_year_js };
        detected.is_none_or(|d| year <= d)
    }
}

/// `@types` package name for a package: `react` → `@types/react`, `@scope/x` → `@types/scope__x`.
fn types_name(name: &str) -> String {
    match name.strip_prefix('@') {
        Some(scoped) => format!("@types/{}", scoped.replacen('/', "__", 1)),
        None => format!("@types/{name}"),
    }
}

/// Lowest major version in a semver range like `^1.2.3`, `~2`, `>=3.0 <4`, `4.x`.
pub fn min_major(range: &str) -> Option<u64> {
    let r = range.trim();
    if r.starts_with("workspace:") || r.starts_with("file:") || r.starts_with("link:") || r.contains("://") {
        return None;
    }
    r.split("||")
        .filter_map(|alt| {
            let t = alt.trim().trim_start_matches(['^', '~', '>', '=', 'v', ' ']);
            let digits: String = t.chars().take_while(char::is_ascii_digit).collect();
            digits.parse::<u64>().ok()
        })
        .min()
}

/// Contents of a `package.json` that rules care about.
#[derive(Debug, Default)]
pub struct PackageJson {
    pub deps: FxHashMap<String, String>,
    /// pnpm/bun `catalog` versions (`workspaces.catalog` or top-level `catalog`).
    pub catalog: FxHashMap<String, String>,
    pub module_type: Option<String>,
    pub engines_node: Option<String>,
    pub name: Option<String>,
    pub module_aliases: Vec<String>,
    pub workspaces: Vec<String>,
}

pub fn read_package_json(path: &Path) -> Option<PackageJson> {
    let v: Value = serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()?;
    let mut p = PackageJson::default();
    for key in ["dependencies", "devDependencies", "peerDependencies", "optionalDependencies"] {
        if let Some(Value::Object(m)) = v.get(key) {
            for (k, ver) in m {
                p.deps.insert(k.clone(), ver.as_str().unwrap_or("").to_string());
            }
        }
    }
    for cat in [v.get("catalog"), v.get("workspaces").and_then(|w| w.get("catalog"))].into_iter().flatten() {
        if let Value::Object(m) = cat {
            for (k, ver) in m {
                p.catalog.insert(k.clone(), ver.as_str().unwrap_or("").to_string());
            }
        }
    }
    p.module_type = v.get("type").and_then(Value::as_str).map(str::to_string);
    p.engines_node = v.get("engines").and_then(|e| e.get("node")).and_then(Value::as_str).map(str::to_string);
    p.name = v.get("name").and_then(Value::as_str).map(str::to_string);
    if let Some(Value::Object(m)) = v.get("_moduleAliases") {
        p.module_aliases = m.keys().cloned().collect();
    }
    p.workspaces = match v.get("workspaces") {
        Some(Value::Array(a)) => a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect(),
        Some(Value::Object(o)) => o
            .get("packages")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect())
            .unwrap_or_default(),
        _ => Vec::new(),
    };
    Some(p)
}

/// `compilerOptions` of a `tsconfig.json` that rules care about.
#[derive(Debug, Default, Clone)]
pub struct TsConfig {
    pub lib: Vec<String>,
    pub target: Option<String>,
    pub module: Option<String>,
    pub base_url: Option<String>,
    pub paths: Vec<String>,
}

/// Reads `compilerOptions` from a `tsconfig.json` (JSONC), following relative `extends`.
pub fn read_tsconfig(path: &Path) -> Option<TsConfig> {
    read_tsconfig_depth(path, 0)
}

fn read_tsconfig_depth(path: &Path, depth: u8) -> Option<TsConfig> {
    let text = strip_jsonc(&std::fs::read_to_string(path).ok()?);
    let v: Value = serde_json::from_str(&text).ok()?;
    let mut cfg = TsConfig::default();
    if depth < 5
        && let Some(ext) = v.get("extends").and_then(Value::as_str)
        && ext.starts_with('.')
    {
        let mut p = path.parent()?.join(ext);
        if p.extension().is_none_or(|e| e != "json") {
            p.set_extension("json");
        }
        if let Some(base) = read_tsconfig_depth(&p, depth + 1) {
            cfg = base;
        }
    }
    if let Some(co) = v.get("compilerOptions") {
        if let Some(a) = co.get("lib").and_then(Value::as_array) {
            cfg.lib = a.iter().filter_map(|x| x.as_str().map(str::to_ascii_lowercase)).collect();
        }
        let s = |k: &str| co.get(k).and_then(Value::as_str).map(str::to_ascii_lowercase);
        cfg.target = s("target").or(cfg.target);
        cfg.module = s("module").or(cfg.module);
        if let Some(b) = co.get("baseUrl").and_then(Value::as_str) {
            cfg.base_url = Some(b.to_string());
        }
        if let Some(Value::Object(m)) = co.get("paths") {
            cfg.paths = m.keys().cloned().collect();
        }
    }
    Some(cfg)
}

/// Removes `//` and `/* */` comments and trailing commas from JSONC.
fn strip_jsonc(src: &str) -> String {
    let b = src.as_bytes();
    let mut out = String::with_capacity(src.len());
    let mut i = 0;
    let mut in_str = false;
    while i < b.len() {
        let c = b[i];
        if in_str {
            out.push(c as char);
            if c == b'\\' && i + 1 < b.len() {
                out.push(b[i + 1] as char);
                i += 2;
                continue;
            }
            if c == b'"' {
                in_str = false;
            }
            i += 1;
        } else if c == b'"' {
            in_str = true;
            out.push('"');
            i += 1;
        } else if c == b'/' && b.get(i + 1) == Some(&b'/') {
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
        } else if c == b'/' && b.get(i + 1) == Some(&b'*') {
            i += 2;
            while i + 1 < b.len() && !(b[i] == b'*' && b[i + 1] == b'/') {
                i += 1;
            }
            i += 2;
        } else if c == b',' {
            let mut j = i + 1;
            while j < b.len() && b[j].is_ascii_whitespace() {
                j += 1;
            }
            if !matches!(b.get(j), Some(b'}') | Some(b']')) {
                out.push(',');
            }
            i += 1;
        } else {
            out.push(c as char);
            i += 1;
        }
    }
    // Non-ASCII bytes were pushed one by one; rebuild from bytes to keep UTF-8 intact.
    String::from_utf8(out.chars().map(|c| c as u8).collect()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions() {
        assert_eq!(min_major("^14.2.0"), Some(14));
        assert_eq!(min_major(">=3.0 <4"), Some(3));
        assert_eq!(min_major("1.x || 2.x"), Some(1));
        assert_eq!(min_major("workspace:*"), None);
        let v: Value = serde_json::from_str(&strip_jsonc("{\"a\": [1,2,], // c\n /* x */ \"b\": \"//no\",}")).unwrap();
        assert_eq!(v["b"], "//no");
        assert_eq!(v["a"].as_array().unwrap().len(), 2);
    }
}
