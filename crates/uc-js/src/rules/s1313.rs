//! IP addresses should not be hardcoded
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const ACCEPTED: &[&str] = &[
    "255.255.255.255",
    "::1",
    "::",
    "0:0:0:0:0:0:0:1",
    "0:0:0:0:0:0:0:0",
];
const ACCEPTED_V6_STARTS: &[&str] = &["2001:db8:"];
const ACCEPTED_V4_STARTS: &[&str] = &["127.", "0.", "2.5", "192.0.2.", "198.51.100.", "203.0.113."];
const V4_MAPPED_PREFIX: &str = "::ffff:0:";

fn is_exception(ip: &str) -> bool {
    ACCEPTED_V6_STARTS.iter().any(|p| ip.starts_with(p))
        || ACCEPTED_V4_STARTS.iter().any(|p| {
            ip.starts_with(p)
                || (ip.starts_with(V4_MAPPED_PREFIX) && ip[V4_MAPPED_PREFIX.len()..].starts_with(p))
        })
        || ACCEPTED.contains(&ip)
}

/// Node's `net.isIPv4`: four dotted decimal octets without leading zeros.
fn is_ipv4(s: &str) -> bool {
    let mut n = 0;
    for part in s.split('.') {
        n += 1;
        let b = part.as_bytes();
        if b.is_empty()
            || b.len() > 3
            || !b.iter().all(u8::is_ascii_digit)
            || (b.len() > 1 && b[0] == b'0')
        {
            return false;
        }
        if part.parse::<u32>().unwrap_or(256) > 255 {
            return false;
        }
    }
    n == 4
}

/// Node's `net.isIPv6`, including an optional zone index.
fn is_ipv6(s: &str) -> bool {
    let addr = match s.find('%') {
        Some(i) => {
            let zone = &s[i + 1..];
            if zone.is_empty()
                || !zone
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'.' | b':'))
            {
                return false;
            }
            &s[..i]
        }
        None => s,
    };
    // Rust accepts the same textual forms (compressed, embedded IPv4).
    addr.contains(':') && addr.parse::<std::net::Ipv6Addr>().is_ok() && embedded_v4_ok(addr)
}

/// Rust tolerates leading zeros nowhere in the embedded IPv4 part either, but double check.
fn embedded_v4_ok(addr: &str) -> bool {
    match addr.rfind(':') {
        Some(i) if addr[i + 1..].contains('.') => is_ipv4(&addr[i + 1..]),
        _ => true,
    }
}

fn is_ip(s: &str) -> bool {
    is_ipv4(s) || is_ipv6(s)
}

fn is_v4_octal_or_hex(ip: &str) -> bool {
    let parts: Vec<&str> = ip.split('.').collect();
    if parts.len() != 4 {
        return false;
    }
    let mut digits = Vec::with_capacity(4);
    for d in parts {
        let b = d.as_bytes();
        if !b.is_empty() && b[0] == b'0' && b[1..].iter().all(|c| (b'0'..=b'7').contains(c)) {
            digits
                .push(u64::from_str_radix(if d.len() > 1 { &d[1..] } else { "0" }, 8).unwrap_or(0));
        } else if b.len() > 2
            && b[0] == b'0'
            && (b[1] == b'x' || b[1] == b'X')
            && b[2..].iter().all(u8::is_ascii_hexdigit)
        {
            match u64::from_str_radix(&d[2..], 16) {
                Ok(v) => digits.push(v),
                Err(_) => return false,
            }
        } else {
            return false;
        }
    }
    let converted = format!("{}.{}.{}.{}", digits[0], digits[1], digits[2], digits[3]);
    !is_exception(&converted) && is_ip(&converted)
}

/// Strips a trailing `/NN` net mask.
fn strip_mask(v: &str) -> &str {
    if let Some(i) = v.rfind('/') {
        let (head, mask) = (&v[..i], &v[i + 1..]);
        if !head.is_empty()
            && !head.contains('/')
            && (1..=3).contains(&mask.len())
            && mask.bytes().all(|c| c.is_ascii_digit())
        {
            return head;
        }
    }
    v
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::StringLiteral]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::StringLiteral(lit) = node.kind() else {
            return;
        };
        let value = lit.value.as_str();
        // Cheap pre-filter: an IP needs a '.' or ':' and is short.
        if value.len() < 2 || value.len() > 64 || !value.bytes().any(|b| b == b'.' || b == b':') {
            return;
        }
        let ip = strip_mask(value);
        if (!is_exception(ip) && is_ip(ip)) || is_v4_octal_or_hex(ip) {
            ctx.report(
                lit.span,
                format!("Make sure using a hardcoded IP address {value} is safe here."),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn ips() {
        check(
            "S1313",
            "a.js",
            r#"
const a = "192.168.12.42";      // Noncompliant {{Make sure using a hardcoded IP address 192.168.12.42 is safe here.}}
const b = "10.0.0.1/24";        // Noncompliant
const c = "127.0.0.1";
const d = "255.255.255.255";
const e = "2001:db8:1::ab9:C0A8:102";
const f = "1080:0:0:0:8:800:200C:417A"; // Noncompliant
const g = "::1";
const h = "0300.0250.0014.0052"; // Noncompliant
const i = "1.2.3";
const j = "a.b.c.d";
const k = "2.5.4.3";
const l = "192.168.012.42";
const m = "fe80::1%eth0";       // Noncompliant
const n = "::ffff:127.0.0.1";   // Noncompliant
const o = "::ffff:0:127.0.0.1";
"#,
        );
    }
}
