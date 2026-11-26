//! NoSQL operations should not be vulnerable to injection attacks
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
            Vuln::NoSql,
            "Change this code to not construct database queries directly from user-controlled data.",
        );
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn mongo() {
        check(
            "UJ1009",
            "a.js",
            r#"
app.post('/users', async (req, res) => {
  await User.find(req.body); // Noncompliant {{Change this code to not construct database queries directly from user-controlled data.}}
  await User.findOne(req.query); // Noncompliant
  const filter = req.body.filter;
  await db.collection('u').deleteMany(filter); // Noncompliant
  await User.find({ ...req.query }); // Noncompliant
  await User.find({ $where: `this.name === '${req.body.name}'` }); // Noncompliant
  await User.find({ name: String(req.body.name) });
  await User.find({ name: req.params.name });
  await User.findById(req.params.id);
  await User.find(req.params);
  const q = JSON.parse(req.query.q);
  await User.find(q); // Noncompliant
  await User.find(sanitize(req.body));
});
"#,
        );
    }
}
