//! Version-pinned quality profiles: the exact "Sonar way" rule sets of specific SonarQube
//! releases, so results can be compared with (or kept identical to) a given server.

use rustc_hash::FxHashSet;
use serde::Deserialize;
use uc_core::{Lang, RuleMeta};

#[derive(Deserialize)]
struct ProfileData {
    #[allow(dead_code)]
    name: String,
    js: Vec<String>,
    go: Vec<String>,
    /// Rule parameters reproducing older rule behaviour (JS rules).
    #[serde(default)]
    params: rustc_hash::FxHashMap<String, serde_json::Map<String, serde_json::Value>>,
}

pub struct Pinned {
    js: FxHashSet<String>,
    go: FxHashSet<String>,
    pub params: rustc_hash::FxHashMap<String, serde_json::Map<String, serde_json::Value>>,
}

impl Pinned {
    pub fn contains(&self, m: &RuleMeta) -> bool {
        if m.langs.contains(&Lang::Go) { self.go.contains(&m.key) } else { self.js.contains(&m.key) }
    }
}

const PROFILES: &[(&str, &str)] = &[("sonarqube-26.9", include_str!("../../../catalog/profiles/sonarqube-26.9.json"))];

/// Names of the built-in pinned profiles.
pub fn names() -> impl Iterator<Item = &'static str> {
    PROFILES.iter().map(|(n, _)| *n)
}

/// The pinned profile called `name`; `Ok(None)` for the regular profiles.
pub fn pinned(name: &str) -> Result<Option<Pinned>, String> {
    if matches!(name, "sonar-way" | "all" | "none") {
        return Ok(None);
    }
    let Some((_, data)) = PROFILES.iter().find(|(n, _)| *n == name) else {
        let known: Vec<&str> = names().collect();
        return Err(format!("unknown profile {name:?} (known: sonar-way, all, none, {})", known.join(", ")));
    };
    let d: ProfileData = serde_json::from_str(data).map_err(|e| e.to_string())?;
    Ok(Some(Pinned { js: d.js.into_iter().collect(), go: d.go.into_iter().collect(), params: d.params }))
}
