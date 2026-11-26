//! HTTP request redirections should not be open to forging attacks
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
        taint::report(ctx, Vuln::Redirect, "Change this code to not perform redirects based on user-controlled data.");
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn server() {
        check(
            "UJ1006",
            "a.js",
            r#"
app.get('/login', (req, res) => {
  const next = req.query.next;
  res.redirect(next); // Noncompliant {{Change this code to not perform redirects based on user-controlled data.}}
  res.redirect(301, req.query.url); // Noncompliant
  res.redirect('/' + next); // Noncompliant
  res.redirect('https://' + next); // Noncompliant
  res.redirect('/profile/' + next);
  res.redirect(`/search?q=${next}`);
  res.redirect('https://example.com/' + next);
  res.redirect(`${BASE_URL}${next}`);
  res.redirect('https://' + req.headers.host + req.url);
  res.redirect('https://' + req.get('host') + req.originalUrl);
  res.redirect(encodeURIComponent(next));
  res.setHeader('Location', next); // Noncompliant
  res.writeHead(302, { Location: req.query.to }); // Noncompliant
  if (!next.startsWith('/app/')) return;
  res.redirect(next);
});
router.get('/k', (ctx) => {
  ctx.redirect(ctx.query.next); // Noncompliant
});
"#,
        );
    }

    #[test]
    fn next() {
        check(
            "UJ1006",
            "middleware.ts",
            r#"
import { NextResponse } from 'next/server';
export function middleware(request: NextRequest) {
  const to = request.nextUrl.searchParams.get('to');
  if (to) return NextResponse.redirect(new URL(to, request.url)); // Noncompliant
  return NextResponse.redirect(new URL('/login', request.url));
}
"#,
        );
        check(
            "UJ1006",
            "a.js",
            r#"
const next = new URLSearchParams(location.search).get('next');
location.href = next; // Noncompliant
window.location = next; // Noncompliant
location.assign(next); // Noncompliant
location.href = '/home/' + next;
location.href = '/home';
const u = new URL(location.href);
u.searchParams.set('t', Date.now());
location.href = u.toString();
location.replace(location.href);
location.href = new URL(location.href).searchParams.get('next'); // Noncompliant
location.href = document.referrer; // Noncompliant
"#,
        );
    }
}
