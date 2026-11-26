//! OS commands should not be vulnerable to command injection attacks
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
        taint::report(ctx, Vuln::Cmd, "Change this code to not construct the OS command from user-controlled data.");
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn commands() {
        check(
            "UJ1002",
            "a.js",
            r#"
const { exec, execSync, spawn, execFile } = require('child_process');
const cp = require('node:child_process');
const util = require('util');
const run = util.promisify(exec);
app.post('/ping', (req, res) => {
  const host = req.body.host;
  exec('ping -c 1 ' + host); // Noncompliant {{Change this code to not construct the OS command from user-controlled data.}}
  cp.execSync(`nslookup ${req.query.name}`); // Noncompliant
  run(`dig ${host}`); // Noncompliant
  spawn('ping', ['-c', '1', host]);
  execFile('ping', ['-c', '1', host]);
  spawn('ping', ['-c', '1', host], { shell: true }); // Noncompliant
  spawn('sh', ['-c', 'ping ' + host]); // Noncompliant
  spawn(req.query.bin, []); // Noncompliant
  exec('ls -la');
  exec('ping ' + parseInt(host));
  exec(`ping ${shellescape([host])}`);
  let cmd = 'ping ' + host;
  cmd = 'uptime';
  exec(cmd);
  regex.exec(host);
});
function notHandler(input) {
  exec('ping ' + input);
}
"#,
        );
    }

    #[test]
    fn imports() {
        check(
            "UJ1002",
            "a.ts",
            r#"
import { exec } from 'child_process';
import { promisify } from 'node:util';
const execAsync = promisify(exec);
export default async function handler(req: NextApiRequest, res: NextApiResponse) {
  const { repo } = req.query;
  await execAsync(`git clone ${repo}`); // Noncompliant
  res.json({ ok: true });
}
"#,
        );
    }
}
