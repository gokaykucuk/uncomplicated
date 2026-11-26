//! Server-side requests should not be vulnerable to forging attacks
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
        taint::report(ctx, Vuln::Ssrf, "Change this code to not construct the URL from user-controlled data.");
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn requests() {
        check(
            "UJ1007",
            "a.js",
            r#"
const axios = require('axios');
const http = require('https');
app.post('/fetch', async (req, res) => {
  const url = req.body.url;
  await fetch(url); // Noncompliant {{Change this code to not construct the URL from user-controlled data.}}
  await axios.get(url); // Noncompliant
  await axios({ method: 'get', url: req.query.target }); // Noncompliant
  http.get(`https://${req.query.host}/status`); // Noncompliant
  await fetch(`https://api.example.com/users/${req.params.id}`);
  await fetch(`${API_BASE}/users/${req.params.id}`);
  await fetch('https://api.example.com/search?q=' + url);
  await fetch(`https://${req.headers.host}/api/x`);
  await fetch(new URL(req.query.u)); // Noncompliant
  await fetch('https://api.example.com');
  myClient.get(url);
});
app.get('/proxy', async (req, res) => {
  const target = req.query.target;
  const parsed = new URL(target);
  if (parsed.hostname !== 'api.example.com') return res.status(400).end();
  await fetch(target);
  const u2 = req.query.u2;
  if (!ALLOWED_HOSTS.includes(new URL(u2).hostname)) return;
  await fetch(u2);
  const u3 = req.query.u3;
  if (!isAllowedUrl(u3)) return;
  await fetch(u3);
  const u4 = req.query.u4;
  if (!u4) return;
  await fetch(u4); // Noncompliant
});
"#,
        );
    }

    #[test]
    fn browser_is_not_ssrf() {
        check(
            "UJ1007",
            "a.js",
            r#"
const q = new URLSearchParams(location.search).get('api');
fetch(q);
"#,
        );
    }
}
