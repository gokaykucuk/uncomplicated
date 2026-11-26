//! I/O function calls should not be vulnerable to path injection attacks
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
        taint::report(ctx, Vuln::Path, "Change this code to not construct the path from user-controlled data.");
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn paths() {
        check(
            "UJ1003",
            "a.js",
            r#"
const fs = require('fs');
const fsp = require('fs/promises');
const path = require('path');
app.get('/file', async (req, res) => {
  const name = req.query.name;
  fs.readFileSync(name); // Noncompliant {{Change this code to not construct the path from user-controlled data.}}
  fs.readFile(path.join(__dirname, 'uploads', name), cb); // Noncompliant
  await fsp.unlink(`/tmp/${req.params.id}.json`); // Noncompliant
  fs.createReadStream(path.resolve('data', req.body.file)); // Noncompliant
  fs.readFileSync(path.join(__dirname, 'uploads', path.basename(name)));
  fs.readFileSync('static.txt');
  res.sendFile(name); // Noncompliant
  res.sendFile(name, { root: path.join(__dirname, 'public') });
  res.download(path.join('/srv', req.params.f)); // Noncompliant
  const p = path.resolve(base, name);
  if (!p.startsWith(base)) return res.status(403).end();
  fs.readFileSync(p);
  fs.readFileSync(encodeURIComponent(name));
  let f = name;
  f = 'default.txt';
  fs.readFileSync(f);
});
"#,
        );
    }

    #[test]
    fn imports() {
        check(
            "UJ1003",
            "a.ts",
            r#"
import { readFile } from 'node:fs/promises';
import { promises as fs } from 'fs';
import { join } from 'path';
router.get('/doc/:name', async (ctx) => {
  ctx.body = await readFile(join('docs', ctx.params.name), 'utf8'); // Noncompliant
  await fs.writeFile(ctx.query.out, 'x'); // Noncompliant
  await readFile(join('docs', String(Number(ctx.params.n))));
});
"#,
        );
    }
}
