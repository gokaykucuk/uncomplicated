//! SQL queries should not be dynamically formatted
use crate::helpers::batch_security1::{fqn, is_literal};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const SIGNATURES: &[&str] = &[
    "pg.Client.query",
    "pg.Pool.query",
    "mysql.createConnection.query",
    "mysql.createPool.query",
    "mysql.createPoolCluster.query",
    "mysql2.createConnection.query",
    "mysql2.createPool.query",
    "mysql2.createPoolCluster.query",
    "sequelize.Sequelize.query",
    "sqlite3.Database.run",
    "sqlite3.Database.get",
    "sqlite3.Database.all",
    "sqlite3.Database.each",
    "sqlite3.Database.exec",
    "better-sqlite3.exec",
    "better-sqlite3.prepare",
    "mssql.ConnectionPool.query",
    "mssql.Request.query",
    "mssql.Request.batch",
    "mssql.Request.execute",
    "mysql2.createConnection.execute",
    "oracledb.getConnection.execute",
    "oracledb.getConnection.executeMany",
    "oracledb.getConnection.queryStream",
    "pg-promise.any",
    "pg-promise.each",
    "pg-promise.func",
    "pg-promise.many",
    "pg-promise.manyOrNone",
    "pg-promise.map",
    "pg-promise.multi",
    "pg-promise.multiResult",
    "pg-promise.none",
    "pg-promise.one",
    "pg-promise.oneOrNone",
    "pg-promise.proc",
    "pg-promise.query",
    "pg-promise.result",
    "knex.raw",
    "knex.whereRaw",
    "knex.havingRaw",
    "knex.groupByRaw",
    "knex.orderByRaw",
    "knex.joinRaw",
    "typeorm.createConnection.query",
    "typeorm.getConnection.query",
    "typeorm.getManager.query",
    "typeorm.getRepository.query",
];

/// Last segment of one of `SIGNATURES`.
fn is_sql_method(name: &str) -> bool {
    matches!(
        name,
        "query"
            | "run"
            | "get"
            | "all"
            | "each"
            | "exec"
            | "prepare"
            | "batch"
            | "execute"
            | "executeMany"
            | "queryStream"
            | "any"
            | "func"
            | "many"
            | "manyOrNone"
            | "map"
            | "multi"
            | "multiResult"
            | "none"
            | "one"
            | "oneOrNone"
            | "proc"
            | "result"
            | "raw"
            | "whereRaw"
            | "havingRaw"
            | "groupByRaw"
            | "orderByRaw"
            | "joinRaw"
    )
}

fn is_hardcoded(e: &Expression<'_>) -> bool {
    is_literal(e)
        || matches!(strip_parens(e), Expression::TemplateLiteral(t) if t.expressions.is_empty())
}

fn as_concat<'b, 'a>(e: &'b Expression<'a>) -> Option<&'b BinaryExpression<'a>> {
    match strip_parens(e) {
        Expression::BinaryExpression(b) if b.operator == BinaryOperator::Addition => Some(b),
        _ => None,
    }
}

fn is_variable_concat(b: &BinaryExpression<'_>) -> bool {
    if !is_hardcoded(&b.right) {
        return true;
    }
    match as_concat(&b.left) {
        Some(l) => is_variable_concat(l),
        None => !is_hardcoded(&b.left),
    }
}

fn is_questionable(e: &Expression<'_>) -> bool {
    match strip_parens(e) {
        Expression::TemplateLiteral(t) => !t.expressions.is_empty(),
        Expression::BinaryExpression(b) if b.operator == BinaryOperator::Addition => {
            is_variable_concat(b)
        }
        Expression::CallExpression(c) => {
            matches!(strip_parens(&c.callee), Expression::StaticMemberExpression(m) if matches!(m.property.name.as_str(), "concat" | "replace"))
        }
        _ => false,
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::CallExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::CallExpression(call) = node.kind() else {
            return;
        };
        let Some(first) = call.arguments.first().and_then(|a| a.as_expression()) else {
            return;
        };
        let Expression::StaticMemberExpression(m) = strip_parens(&call.callee) else {
            return;
        };
        if !is_sql_method(m.property.name.as_str()) || !is_questionable(first) {
            return;
        }
        if fqn(ctx, &call.callee).is_some_and(|q| SIGNATURES.contains(&q.as_str())) {
            ctx.report(
                call.callee.span(),
                "Make sure that executing SQL queries is safe here.",
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn sql_queries() {
        check(
            "S2077",
            "a.js",
            r#"
const mysql = require('mysql');
const conn = mysql.createConnection({ host: 'localhost' });
conn.query('SELECT * FROM users WHERE id = ' + userId, cb); // Noncompliant {{Make sure that executing SQL queries is safe here.}}
conn.query(`SELECT * FROM users WHERE id = ${userId}`); // Noncompliant
conn.query('SELECT * FROM users WHERE id = ?', [userId]);
conn.query('SELECT ' + 'a');
conn.query("SELECT * FROM t WHERE x = 'X'".replace('X', input)); // Noncompliant
const { Client } = require('pg');
const client = new Client();
client.query("DELETE FROM t WHERE id=" + id); // Noncompliant
const knex = require('knex')(config);
knex.raw(`select ${col}`); // Noncompliant
db.query("x" + y);
"#,
        );
    }
}
