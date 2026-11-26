//! Database queries should not be vulnerable to injection attacks
use crate::helpers::taint::{self, Vuln};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

impl JsRule for Rule {
    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {
        taint::report(
            ctx,
            Vuln::Sql,
            "Change this code to not construct SQL queries directly from user-controlled data.",
        );
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn express() {
        check(
            "UJ1001",
            "a.js",
            r#"
const express = require('express');
const app = express();
app.get('/users', (req, res) => {
  const id = req.query.id;
  db.query("SELECT * FROM users WHERE id = '" + id + "'"); // Noncompliant {{Change this code to not construct SQL queries directly from user-controlled data.}}
  db.query(`SELECT * FROM users WHERE name = '${req.params.name}'`); // Noncompliant
  db.query("SELECT * FROM users WHERE id = ?", [id]);
  db.query("SELECT * FROM users WHERE id = " + parseInt(id, 10));
  db.query("SELECT * FROM users WHERE id = " + Number(req.query.n));
  db.query(id);
  db.query(`SELECT * FROM t WHERE x = ${other}`);
  let q = "SELECT * FROM t";
  q += " WHERE name = '" + req.body.name + "'";
  db.query(q); // Noncompliant
  const { name } = req.body;
  sequelize.query(`DELETE FROM t WHERE name = '${name.trim()}'`); // Noncompliant
  prisma.$queryRawUnsafe(`SELECT * FROM t WHERE a = ${req.query.a}`); // Noncompliant
  prisma.$queryRaw`SELECT * FROM t WHERE a = ${req.query.a}`;
  knex.raw("select * from t order by " + req.query.sort); // Noncompliant
  let sql = "SELECT * FROM t WHERE id = " + id;
  sql = "SELECT * FROM t";
  db.query(sql);
  db.query("SELECT * FROM t WHERE id = " + escape(id));
  db.query("SELECT * FROM t WHERE id = " + sanitize(id));
  db.query("SELECT * FROM t WHERE id = " + mysql.escape(id));
});
"#,
        );
    }

    #[test]
    fn handlers() {
        check(
            "UJ1001",
            "route.ts",
            r#"
export async function GET(request: Request, { params }: { params: { id: string } }) {
  const url = new URL(request.url);
  const q = url.searchParams.get('q');
  await sql.unsafe(`SELECT * FROM items WHERE name LIKE '%${q}%'`); // Noncompliant
  await db.execute(`SELECT * FROM items WHERE id = ${params.id}`); // Noncompliant
}
export async function POST(request: Request) {
  const body = await request.json();
  await db.query(`INSERT INTO t (a) VALUES ('${body.a}')`); // Noncompliant
}
function helper(req) {
  db.query("SELECT * FROM t WHERE id = " + req.query.id);
}
"#,
        );
        check(
            "UJ1001",
            "a.js",
            r#"
const http = require('http');
http.createServer((req, res) => {
  db.query("SELECT * FROM t WHERE path = '" + req.url + "'"); // Noncompliant
}).listen(8080);
exports.handler = async (event) => {
  await db.query(`SELECT * FROM t WHERE id = ${event.body}`);
};
router.post('/x', async (ctx) => {
  await db.query(`SELECT * FROM t WHERE a = ${ctx.request.body.a}`); // Noncompliant
});
"#,
        );
    }

    #[test]
    fn guards_and_lambda() {
        check(
            "UJ1001",
            "a.js",
            r#"
// APIGatewayProxyEvent
exports.handler = async (event) => {
  const id = event.pathParameters.id;
  await db.query(`SELECT * FROM t WHERE id = ${id}`); // Noncompliant
};
app.get('/x', (req, res) => {
  const sort = req.query.sort;
  if (!['name', 'date'].includes(sort)) return res.status(400).end();
  db.query(`SELECT * FROM t ORDER BY ${sort}`);
  const col = ALLOWED.has(req.query.col) ? req.query.col : 'id';
  db.query(`SELECT * FROM t ORDER BY ${col}`);
});
"#,
        );
    }
}
