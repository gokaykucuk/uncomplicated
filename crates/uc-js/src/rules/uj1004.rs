//! Endpoints should not be vulnerable to reflected cross-site scripting (XSS) attacks
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
        taint::report(ctx, Vuln::Xss, "Change this code to not reflect user-controlled data.");
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn express() {
        check(
            "UJ1004",
            "a.js",
            r#"
app.get('/hello', (req, res) => {
  const name = req.query.name;
  res.send('<h1>Hello ' + name + '</h1>'); // Noncompliant {{Change this code to not reflect user-controlled data.}}
  res.status(404).send(`Not found: ${req.params.id}`); // Noncompliant
  res.json({ name });
  res.send({ name });
  res.send(req.body);
  res.send('Hello ' + escapeHtml(name));
  res.send('Hello ' + encodeURIComponent(name));
  res.type('text').send(name);
  res.write(name);
  res.end(name);
});
app.get('/plain', (req, res) => {
  res.set('Content-Type', 'text/plain');
  res.send(req.query.q);
});
app.get('/html', (req, res) => {
  res.writeHead(200, { 'Content-Type': 'text/html' });
  res.end(`<p>${req.query.q}</p>`); // Noncompliant
});
"#,
        );
    }

    #[test]
    fn frameworks() {
        check(
            "UJ1004",
            "a.ts",
            r#"
app.get('/h', (c) => {
  const q = c.req.query('q');
  return c.html(`<p>${q}</p>`); // Noncompliant
});
app.get('/v', zValidator('query', schema), (c) => {
  const { q } = c.req.valid('query');
  return c.html(`<p>${q}</p>`);
});
router.get('/k', (ctx) => {
  ctx.body = `<p>${ctx.query.q}</p>`; // Noncompliant
  ctx.body = { q: ctx.query.q };
});
fastify.get('/f', async (request, reply) => {
  reply.send(request.query.q);
});
export async function GET(request: Request) {
  const q = new URL(request.url).searchParams.get('q');
  return new Response(`<p>${q}</p>`, { headers: { 'Content-Type': 'text/html' } }); // Noncompliant
}
"#,
        );
    }
}
