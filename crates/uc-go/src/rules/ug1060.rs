//! Database queries should not be vulnerable to injection attacks (taint analysis).
//!
//! SQL queries built from request data (taint analysis). Queries UG1008 already reports as built by formatting are left to it.
use crate::prelude::*;
use crate::taint;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const MSG: &str = "Change this code to not construct SQL queries directly from user-controlled data.";

impl GoRule for Rule {
    fn run_once(&self, ctx: &Ctx<'_>) {
        taint::report(ctx, taint::SQL, MSG);
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn rule() {
        check(
            "UG1060",
            r#"package p

import (
	"database/sql"
	"fmt"
	"net/http"
	"strconv"
	"strings"

	"github.com/gin-gonic/gin"
)

func handler(w http.ResponseWriter, r *http.Request, db *sql.DB) {
	name := r.FormValue("name")
	db.Query(name) // Noncompliant {{Change this code to not construct SQL queries directly from user-controlled data.}}
	q := "SELECT * FROM users WHERE name = '" + name + "'"
	db.Query(q) // Noncompliant
	id, _ := strconv.Atoi(r.URL.Query().Get("id"))
	db.Query("SELECT * FROM t WHERE id = " + strconv.Itoa(id))
	db.Query("SELECT * FROM t WHERE name = ?", name)
	sort := r.URL.Query().Get("sort")
	switch sort {
	case "name", "date":
	default:
		sort = "name"
	}
	db.Query("SELECT * FROM t ORDER BY " + sort)
	col := r.Header.Get("X-Col")
	if !valid(col) {
		return
	}
	db.Exec("UPDATE t SET " + col + " = 1")
	var parts []string
	parts = append(parts, "a = '"+r.PostFormValue("a")+"'")
	db.QueryContext(r.Context(), "SELECT 1 WHERE "+strings.Join(parts, " AND ")) // Noncompliant
	name = "fixed"
	db.Query("SELECT * FROM t WHERE n = '" + name + "'")
	cache.Get(r.Context(), r.FormValue("key"))
}

func ginHandler(c *gin.Context, db *sql.DB) {
	db.Exec(fmt.Sprintf("DELETE FROM t WHERE id = %s", c.Param("id"))) // Noncompliant
	var req struct{ Name string }
	c.ShouldBindJSON(&req)
	db.Query("SELECT * FROM t WHERE n = '" + req.Name + "'") // Noncompliant
	db.Where(req)
	c.Query("q")
}

func notHandler(db *sql.DB, name string) {
	db.Query("SELECT " + name)
}
"#,
        );
    }
}
