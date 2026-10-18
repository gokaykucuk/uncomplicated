//! Locks and other "sync" values should not be passed by value (go vet copylocks).
//!
//! Parameters and receivers whose type is `sync.Mutex`, `sync.WaitGroup`, ..., an atomic type, or
//! a struct declared in this file that directly contains one.
use crate::helpers_extra::*;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const SYNC: &[&str] = &[
    "Mutex",
    "RWMutex",
    "WaitGroup",
    "Once",
    "Cond",
    "Map",
    "Pool",
];
const ATOMIC: &[&str] = &[
    "Bool", "Int32", "Int64", "Uint32", "Uint64", "Uintptr", "Value", "Pointer",
];

/// `sync.Mutex`-like type (by value): returns its display name.
fn lock_type(
    ctx: &Ctx<'_>,
    ty: Nd<'_>,
    sync_name: Option<&str>,
    atomic_name: Option<&str>,
) -> Option<String> {
    let ty = if ty.kind() == "generic_type" {
        ty.child_by_field_name("type")?
    } else {
        ty
    };
    if ty.kind() != "qualified_type" {
        return None;
    }
    let pkg = ctx.text(ty.child_by_field_name("package")?);
    let name = ctx.text(ty.child_by_field_name("name")?);
    if (Some(pkg) == sync_name && SYNC.contains(&name))
        || (Some(pkg) == atomic_name && ATOMIC.contains(&name))
    {
        Some(format!("{pkg}.{name}"))
    } else {
        None
    }
}

impl GoRule for Rule {
    fn run_once(&self, ctx: &Ctx<'_>) {
        let mut sync_name = None;
        let mut atomic_name = None;
        for spec in import_specs(ctx) {
            let Some(path) = spec
                .child_by_field_name("path")
                .and_then(|p| string_value(ctx, p))
            else {
                continue;
            };
            let name = spec
                .child_by_field_name("name")
                .map(|n| ctx.text(n))
                .unwrap_or_else(|| default_import_name(path));
            match path {
                "sync" => sync_name = Some(name),
                "sync/atomic" => atomic_name = Some(name),
                _ => {}
            }
        }
        if sync_name.is_none() && atomic_name.is_none() {
            return;
        }
        // Struct types of this file directly containing a lock.
        let mut lock_structs: FxHashMap<&str, String> = FxHashMap::default();
        for n in &ctx.nodes {
            if n.kind() != "type_spec" {
                continue;
            }
            let (Some(name), Some(st)) =
                (n.child_by_field_name("name"), n.child_by_field_name("type"))
            else {
                continue;
            };
            if st.kind() != "struct_type" {
                continue;
            }
            let Some(fields) = st.named_child(0) else {
                continue;
            };
            let mut c = fields.walk();
            for fd in fields
                .named_children(&mut c)
                .filter(|f| f.kind() == "field_declaration")
            {
                if let Some(t) = fd.child_by_field_name("type")
                    && let Some(l) = lock_type(ctx, t, sync_name, atomic_name)
                {
                    lock_structs.insert(ctx.text(name), l);
                    break;
                }
            }
        }
        for n in &ctx.nodes {
            if n.kind() != "parameter_declaration" {
                continue;
            }
            let Some(t) = n.child_by_field_name("type") else {
                continue;
            };
            let msg = if let Some(l) = lock_type(ctx, t, sync_name, atomic_name) {
                format!("Pass this \"{l}\" by pointer: copying it copies its internal state.")
            } else if t.kind() == "type_identifier"
                && let Some(l) = lock_structs.get(ctx.text(t))
            {
                format!(
                    "Pass this \"{}\" by pointer: it contains a \"{l}\" that must not be copied.",
                    ctx.text(t)
                )
            } else {
                continue;
            };
            let at = n.child_by_field_name("name").unwrap_or(t);
            ctx.report(at, msg);
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn rule() {
        check(
            "UG1018",
            r#"package p

import (
	"sync"
	"sync/atomic"
)

type Counter struct {
	mu sync.Mutex
	n  int
}

type Safe struct {
	mu *sync.Mutex
}

func (c Counter) Get() int { // Noncompliant {{Pass this "Counter" by pointer: it contains a "sync.Mutex" that must not be copied.}}
	return c.n
}

func (c *Counter) Inc() {}

func (s Safe) Get() {}

func run(wg sync.WaitGroup, mu *sync.Mutex, n atomic.Int64) { // Noncompliant 2
}

func ok(wg *sync.WaitGroup, c *Counter, f func(*sync.Mutex)) {}
"#,
        );
    }
}
