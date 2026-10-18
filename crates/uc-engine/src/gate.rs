//! Quality gate evaluation (defaults to the "Sonar way" gate on new code).

use serde::Serialize;

use crate::config::{Condition, Gate};
use crate::measures::Measures;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub enum Status {
    #[serde(rename = "OK")]
    Ok,
    #[serde(rename = "ERROR")]
    Error,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConditionResult {
    pub metric: String,
    pub op: String,
    pub threshold: f64,
    pub value: Option<f64>,
    pub status: Status,
    /// Why a condition was not evaluated, if it wasn't.
    pub skipped: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct GateResult {
    pub status: Status,
    pub on_new_code: bool,
    pub conditions: Vec<ConditionResult>,
}

pub fn sonar_way() -> Vec<Condition> {
    let c = |metric: &str, op: &str, threshold: f64| Condition { metric: metric.into(), op: op.into(), threshold };
    vec![
        c("new_reliability_rating", "gt", 1.0),
        c("new_security_rating", "gt", 1.0),
        c("new_maintainability_rating", "gt", 1.0),
        c("new_coverage", "lt", 80.0),
        c("new_duplicated_lines_density", "gt", 3.0),
    ]
}

/// Coverage and duplication conditions are ignored on small changes (Sonar's "fudge factor").
const SMALL_CHANGE_LINES: f64 = 20.0;

pub fn evaluate(cfg: &Gate, overall: &Measures, new: Option<&Measures>) -> GateResult {
    let conditions = cfg.conditions.clone().unwrap_or_else(sonar_way);
    let on_new = new.is_some();
    let mut results = Vec::new();
    for c in conditions {
        let (metric, source) = match (c.metric.strip_prefix("new_"), new) {
            (Some(_), Some(n)) => (c.metric.as_str(), n),
            (Some(base), None) if cfg.fallback_overall => (overall_key(base), overall),
            (Some(_), None) => {
                results.push(skip(&c, "no new-code reference configured"));
                continue;
            }
            (None, _) => (c.metric.as_str(), overall),
        };
        let small = new
            .filter(|_| c.metric.starts_with("new_"))
            .and_then(|n| n.get("new_lines").copied())
            .is_some_and(|l| l < SMALL_CHANGE_LINES);
        if small && (c.metric.contains("coverage") || c.metric.contains("duplicated")) {
            results.push(skip(&c, "fewer than 20 new lines"));
            continue;
        }
        let Some(&value) = source.get(metric) else {
            results.push(skip(&c, "no value (e.g. no coverage report)"));
            continue;
        };
        let failed = match c.op.as_str() {
            "lt" => value < c.threshold,
            "gt" => value > c.threshold,
            _ => false,
        };
        results.push(ConditionResult {
            metric: c.metric.clone(),
            op: c.op.clone(),
            threshold: c.threshold,
            value: Some(value),
            status: if failed { Status::Error } else { Status::Ok },
            skipped: None,
        });
    }
    let status = if results.iter().any(|r| r.status == Status::Error) { Status::Error } else { Status::Ok };
    GateResult { status, on_new_code: on_new, conditions: results }
}

fn overall_key(base: &str) -> &str {
    match base {
        "maintainability_rating" => "sqale_rating",
        "technical_debt" => "sqale_index",
        other => other,
    }
}

fn skip(c: &Condition, why: &str) -> ConditionResult {
    ConditionResult {
        metric: c.metric.clone(),
        op: c.op.clone(),
        threshold: c.threshold,
        value: None,
        status: Status::Ok,
        skipped: Some(why.into()),
    }
}
