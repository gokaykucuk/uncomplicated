//! DOM updates should not lead to cross-site scripting (XSS) attacks
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
            Vuln::DomXss,
            "Change this code to not dynamically update the DOM with user-controlled data.",
        );
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn dom() {
        check(
            "UJ1005",
            "a.js",
            r#"
const params = new URLSearchParams(window.location.search);
const q = params.get('q');
document.getElementById('out').innerHTML = 'Results for ' + q; // Noncompliant {{Change this code to not dynamically update the DOM with user-controlled data.}}
document.getElementById('out').textContent = q;
el.innerHTML = DOMPurify.sanitize(q);
el.insertAdjacentHTML('beforeend', location.hash.slice(1)); // Noncompliant
document.write('<a href="' + document.referrer + '">back</a>'); // Noncompliant
$('#x').html(decodeURIComponent(location.hash)); // Noncompliant
$('#x').text(location.hash);
el.innerHTML = '<b>static</b>';
el.innerHTML = '<a href="' + location.href + '">link</a>';
el.innerHTML = decodeURIComponent(location.href); // Noncompliant
let h = location.hash;
h = '';
el.innerHTML = h;
window.addEventListener('message', (e) => {
  el.innerHTML = e.data; // Noncompliant
});
window.addEventListener('message', (e) => {
  if (e.origin !== 'https://example.com') return;
  el.innerHTML = e.data;
});
function f(location) {
  el.innerHTML = location.search;
}
"#,
        );
    }

    #[test]
    fn react() {
        check(
            "UJ1005",
            "a.jsx",
            r#"
function Search() {
  const q = new URLSearchParams(location.search).get('q');
  return <div>
    <p dangerouslySetInnerHTML={{ __html: q }} /> {/* Noncompliant */}
    <p dangerouslySetInnerHTML={{ __html: marked(q) }} />
    <p>{q}</p>
  </div>;
}
"#,
        );
    }
}
