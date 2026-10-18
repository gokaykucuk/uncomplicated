use serde_json::{Map, Value};

/// User-supplied rule parameters (from config), with typed accessors that fall back to defaults.
#[derive(Debug, Clone, Default)]
pub struct Params(pub Map<String, Value>);

impl Params {
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.0.get(key)
    }

    pub fn u32(&self, key: &str, default: u32) -> u32 {
        match self.0.get(key) {
            Some(Value::Number(n)) => n.as_u64().map_or(default, |v| v as u32),
            Some(Value::String(s)) => s.trim().parse().unwrap_or(default),
            _ => default,
        }
    }

    pub fn bool(&self, key: &str, default: bool) -> bool {
        match self.0.get(key) {
            Some(Value::Bool(b)) => *b,
            Some(Value::String(s)) => s.trim().parse().unwrap_or(default),
            _ => default,
        }
    }

    pub fn str<'p>(&'p self, key: &str, default: &'p str) -> &'p str {
        match self.0.get(key) {
            Some(Value::String(s)) => s,
            _ => default,
        }
    }

    /// Comma-separated string or JSON array of strings.
    pub fn list(&self, key: &str, default: &[&str]) -> Vec<String> {
        match self.0.get(key) {
            Some(Value::String(s)) => {
                s.split(',').map(|p| p.trim().to_string()).filter(|p| !p.is_empty()).collect()
            }
            Some(Value::Array(a)) => a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect(),
            _ => default.iter().map(|s| s.to_string()).collect(),
        }
    }
}
