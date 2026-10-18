//! Token stream for copy-paste detection. String literals are normalized so that blocks that
//! only differ in string contents still count as duplicates (like SonarJS).

use oxc_parser::Kind;
use uc_core::CpdToken;
use xxhash_rust::xxh3::xxh3_64;

use crate::Ctx;

pub fn tokens(ctx: &Ctx<'_, '_>) -> Vec<CpdToken> {
    let mut out = Vec::with_capacity(ctx.tokens.len());
    for t in ctx.tokens {
        let hash = match t.kind() {
            Kind::Str | Kind::NoSubstitutionTemplate | Kind::TemplateHead | Kind::TemplateMiddle | Kind::TemplateTail => {
                0x5354_5221
            }
            Kind::Eof => continue,
            _ => xxh3_64(ctx.text(t.span()).as_bytes()) as u32,
        };
        out.push(CpdToken { hash, line: ctx.line(t.start()) });
    }
    out
}
