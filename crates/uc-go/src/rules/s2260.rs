//! Go parser failure. Reported by the analyzer itself when a file doesn't parse.
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

impl GoRule for Rule {}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn parse_error() {
        check("S2260", "package p\n\nfunc f( { // Noncompliant\n}\n");
    }
}
