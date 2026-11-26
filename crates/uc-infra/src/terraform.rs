//! Terraform rules (`TF*`), on tree-sitter-hcl.

use std::cell::RefCell;

use serde_json::Value;
use tree_sitter::{Node, Parser, Tree};
use uc_core::Span;

use crate::util::{Out, literal_secret_value, secret_name};

pub const RULES: &[&str] =
    &["TF001", "TF002", "TF003", "TF004", "TF005", "TF006", "TF007", "TF008", "TF009", "TF010", "TF011", "TF012"];

thread_local! {
    static PARSER: RefCell<Parser> = RefCell::new({
        let mut p = Parser::new();
        p.set_language(&tree_sitter_hcl::LANGUAGE.into()).expect("tree-sitter-hcl");
        p
    });
}

pub(crate) fn parse(src: &str) -> Option<Tree> {
    PARSER.with_borrow_mut(|p| p.parse(src, None))
}

fn sp(n: Node) -> Span {
    Span::new(n.start_byte() as u32, n.end_byte() as u32)
}

/// Comment ranges, for metrics.
pub(crate) fn comments(tree: &Tree) -> Vec<Span> {
    let mut out = Vec::new();
    let mut c = tree.walk();
    'outer: loop {
        let n = c.node();
        if n.kind() == "comment" {
            out.push(sp(n));
        } else if c.goto_first_child() {
            continue;
        }
        while !c.goto_next_sibling() {
            if !c.goto_parent() {
                break 'outer;
            }
        }
    }
    out
}

/// An evaluated expression.
#[derive(Debug)]
enum Val<'t> {
    Str(String),
    Num(f64),
    Bool(bool),
    Null,
    Tuple(Vec<Node<'t>>),
    Object(Vec<(String, Node<'t>)>),
    /// References, calls, interpolated templates...
    Other,
}

struct Tf<'s> {
    src: &'s str,
}

impl<'s> Tf<'s> {
    fn text(&self, n: Node) -> &'s str {
        &self.src[n.start_byte()..n.end_byte()]
    }

    fn named_children<'t>(&self, n: Node<'t>) -> Vec<Node<'t>> {
        let mut c = n.walk();
        n.named_children(&mut c).filter(|c| c.kind() != "comment").collect()
    }

    /// The (type, labels, body) of a block.
    fn block<'t>(&self, b: Node<'t>) -> (&'s str, Vec<String>, Option<Node<'t>>) {
        let mut ty = "";
        let mut labels = Vec::new();
        let mut body = None;
        for (i, ch) in self.named_children(b).into_iter().enumerate() {
            match ch.kind() {
                "identifier" if i == 0 => ty = self.text(ch),
                "identifier" => labels.push(self.text(ch).to_string()),
                "string_lit" => labels.push(self.string_lit(ch)),
                "body" => body = Some(ch),
                _ => {}
            }
        }
        (ty, labels, body)
    }

    fn string_lit(&self, n: Node) -> String {
        let t = self.text(n);
        unescape(t.strip_prefix('"').and_then(|t| t.strip_suffix('"')).unwrap_or(t))
    }

    /// Attributes of a body: (name, attribute node, expression node).
    fn attrs<'t>(&self, body: Option<Node<'t>>) -> Vec<(&'s str, Node<'t>, Node<'t>)> {
        let Some(body) = body else { return Vec::new() };
        self.named_children(body)
            .into_iter()
            .filter(|n| n.kind() == "attribute")
            .filter_map(|a| {
                let ch = self.named_children(a);
                let name = ch.iter().find(|c| c.kind() == "identifier")?;
                let expr = ch.iter().find(|c| c.kind() == "expression")?;
                Some((self.text(*name), a, *expr))
            })
            .collect()
    }

    fn attr<'t>(&self, body: Option<Node<'t>>, name: &str) -> Option<Node<'t>> {
        self.attrs(body).into_iter().rev().find(|(n, _, _)| *n == name).map(|(_, _, e)| e)
    }

    fn blocks<'t>(&self, body: Option<Node<'t>>, ty: &str) -> Vec<Node<'t>> {
        let Some(body) = body else { return Vec::new() };
        self.named_children(body)
            .into_iter()
            .filter(|n| n.kind() == "block" && !n.has_error() && self.block(*n).0 == ty)
            .collect()
    }

    fn body<'t>(&self, block: Node<'t>) -> Option<Node<'t>> {
        self.block(block).2
    }

    fn eval<'t>(&self, n: Node<'t>) -> Val<'t> {
        match n.kind() {
            "expression" | "literal_value" | "template_expr" | "collection_value" => {
                match self.named_children(n).as_slice() {
                    [one] => self.eval(*one),
                    _ => Val::Other,
                }
            }
            "string_lit" => Val::Str(self.string_lit(n)),
            "numeric_lit" => self.text(n).parse().map_or(Val::Other, Val::Num),
            "bool_lit" => Val::Bool(self.text(n) == "true"),
            "null_lit" => Val::Null,
            "quoted_template" => {
                let parts = self.named_children(n);
                let lits: Vec<_> = parts
                    .iter()
                    .filter(|p| !matches!(p.kind(), "quoted_template_start" | "quoted_template_end"))
                    .collect();
                if lits.iter().all(|p| p.kind() == "template_literal") {
                    Val::Str(unescape(&lits.iter().map(|p| self.text(**p)).collect::<String>()))
                } else {
                    Val::Other
                }
            }
            "heredoc_template" => match self.heredoc(n, false) {
                Some(s) => Val::Str(s),
                None => Val::Other,
            },
            "tuple" => Val::Tuple(self.named_children(n).into_iter().filter(|c| c.kind() == "expression").collect()),
            "object" => Val::Object(
                self.named_children(n)
                    .into_iter()
                    .filter(|c| c.kind() == "object_elem")
                    .filter_map(|e| {
                        let k = e.child_by_field_name("key")?;
                        let v = e.child_by_field_name("val")?;
                        let key = match self.eval(k) {
                            Val::Str(s) => s,
                            _ => self.text(k).trim().to_string(),
                        };
                        Some((key, v))
                    })
                    .collect(),
            ),
            "variable_expr" => Val::Other,
            _ => Val::Other,
        }
    }

    /// The text of a heredoc. With `placeholders`, interpolations become `__x__` instead of
    /// failing.
    fn heredoc(&self, n: Node, placeholders: bool) -> Option<String> {
        let mut out = String::new();
        for p in self.named_children(n) {
            match p.kind() {
                "heredoc_start" | "heredoc_identifier" => {}
                "template_literal" => out.push_str(self.text(p)),
                "template_interpolation" if placeholders => out.push_str("__x__"),
                _ => return None,
            }
        }
        Some(out)
    }

    fn str_of(&self, n: Option<Node>) -> Option<String> {
        match self.eval(n?) {
            Val::Str(s) => Some(s),
            _ => None,
        }
    }

    fn bool_of(&self, n: Option<Node>) -> Option<bool> {
        match self.eval(n?) {
            Val::Bool(b) => Some(b),
            Val::Str(s) if s == "true" || s == "false" => Some(s == "true"),
            _ => None,
        }
    }

    fn num_of(&self, n: Option<Node>) -> Option<f64> {
        match self.eval(n?) {
            Val::Num(x) => Some(x),
            Val::Str(s) => s.parse().ok(),
            _ => None,
        }
    }

    /// String items of a tuple (non-literal items are skipped) with their nodes.
    fn strings<'t>(&self, n: Option<Node<'t>>) -> Vec<(String, Node<'t>)> {
        match n.map(|n| self.eval(n)) {
            Some(Val::Tuple(items)) => items.into_iter().filter_map(|i| self.str_of(Some(i)).map(|s| (s, i))).collect(),
            Some(Val::Str(s)) => vec![(s, n.unwrap())],
            _ => Vec::new(),
        }
    }

    /// An expression as JSON (for IAM policies): objects, tuples, strings, `jsonencode(...)`,
    /// JSON text in strings and heredocs.
    fn json(&self, n: Node) -> Option<Value> {
        if let Some(call) = self.function_call(n) {
            let ch = self.named_children(call);
            let name = ch.first().map(|c| self.text(*c))?;
            if name != "jsonencode" {
                return None;
            }
            let args = ch.iter().find(|c| c.kind() == "function_arguments")?;
            let first = self.named_children(*args).into_iter().find(|c| c.kind() == "expression")?;
            return Some(self.to_json(first));
        }
        if let Some(h) = self.find_kind(n, "heredoc_template") {
            return serde_json::from_str(&self.heredoc(h, true)?).ok();
        }
        match self.eval(n) {
            Val::Str(s) => serde_json::from_str(&s).ok(),
            _ => None,
        }
    }

    fn find_kind<'t>(&self, n: Node<'t>, kind: &str) -> Option<Node<'t>> {
        let mut cur = n;
        loop {
            if cur.kind() == kind {
                return Some(cur);
            }
            if !matches!(cur.kind(), "expression" | "template_expr" | "expr_term") {
                return None;
            }
            match self.named_children(cur).as_slice() {
                [one] => cur = *one,
                _ => return None,
            }
        }
    }

    fn function_call<'t>(&self, n: Node<'t>) -> Option<Node<'t>> {
        self.find_kind(n, "function_call")
    }

    fn to_json(&self, n: Node) -> Value {
        match self.eval(n) {
            Val::Str(s) => Value::String(s),
            Val::Num(x) => serde_json::Number::from_f64(x).map_or(Value::Null, Value::Number),
            Val::Bool(b) => Value::Bool(b),
            Val::Tuple(items) => Value::Array(items.into_iter().map(|i| self.to_json(i)).collect()),
            Val::Object(kv) => Value::Object(kv.into_iter().map(|(k, v)| (k, self.to_json(v))).collect()),
            Val::Null | Val::Other => Value::Null,
        }
    }
}

fn unescape(s: &str) -> String {
    if !s.contains('\\') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match it.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some(o) => out.push(o),
            None => {}
        }
    }
    out
}

pub(crate) fn check(path: &str, src: &str, tree: &Tree, out: &mut Out) {
    let tf = Tf { src };
    let root = tree.root_node();
    let Some(body) = tf.named_children(root).into_iter().find(|n| n.kind() == "body") else { return };
    let tfvars = path.ends_with(".tfvars");
    if tfvars {
        let name = path.rsplit('/').next().unwrap_or(path).to_ascii_lowercase();
        if !name.contains("example") && !name.contains("sample") && !name.contains("template") {
            secrets(&tf, Some(body), out);
        }
        return;
    }
    let ebs_default = src.contains("aws_ebs_encryption_by_default");
    for b in tf.named_children(body) {
        if b.kind() != "block" || b.has_error() {
            continue;
        }
        let (ty, labels, body) = tf.block(b);
        match ty {
            "resource" => {
                let Some(rtype) = labels.first() else { continue };
                resource(&tf, rtype, b, body, ebs_default, out);
                secrets(&tf, body, out);
            }
            "data" if labels.first().map(String::as_str) == Some("aws_iam_policy_document") => {
                for st in tf.blocks(body, "statement") {
                    let sb = tf.body(st);
                    let effect = tf.str_of(tf.attr(sb, "effect")).unwrap_or_else(|| "Allow".into());
                    let actions = tf.strings(tf.attr(sb, "actions"));
                    let resources = tf.strings(tf.attr(sb, "resources"));
                    if effect == "Allow"
                        && resources.iter().any(|(r, _)| r == "*")
                        && let Some((_, at)) = actions.iter().find(|(a, _)| a == "*")
                    {
                        out.emit("TF008", sp(*at), "Make sure granting all actions on all resources is necessary.");
                    }
                }
            }
            "provider" | "module" => {
                secrets(&tf, body, out);
                if ty == "module" {
                    module(&tf, body, out);
                }
            }
            _ => {}
        }
    }
}

/// TF007: literal credentials in attributes (nested blocks included).
fn secrets(tf: &Tf, body: Option<Node>, out: &mut Out) {
    if !out.on("TF007") {
        return;
    }
    for (name, _, expr) in tf.attrs(body) {
        if secret_name(name)
            && let Val::Str(v) = tf.eval(expr)
            && literal_secret_value(&v)
        {
            out.emit("TF007", sp(expr), format!("Don't hard-code \"{name}\"; use a variable or a secret store."));
        }
    }
    if let Some(body) = body {
        for b in tf.named_children(body) {
            if b.kind() == "block" && !b.has_error() && tf.block(b).0 != "dynamic" {
                secrets(tf, tf.body(b), out);
            }
        }
    }
}

const PUBLIC_ACLS: &[&str] = &["public-read", "public-read-write", "authenticated-read"];
const WORLD: &[&str] = &["0.0.0.0/0", "::/0"];

fn resource(tf: &Tf, rtype: &str, block: Node, body: Option<Node>, ebs_default: bool, out: &mut Out) {
    let attr = |n: &str| tf.attr(body, n);
    let str_attr = |n: &str| tf.str_of(attr(n));
    let bool_attr = |n: &str| tf.bool_of(attr(n));
    match rtype {
        "aws_s3_bucket" | "aws_s3_bucket_acl" => {
            if let Some(acl) = str_attr("acl")
                && PUBLIC_ACLS.contains(&acl.as_str())
            {
                out.emit(
                    "TF001",
                    sp(attr("acl").unwrap()),
                    format!("Make sure granting \"{acl}\" access to this bucket is safe here."),
                );
            }
        }
        "aws_s3_bucket_public_access_block" | "aws_s3_account_public_access_block" => {
            for k in ["block_public_acls", "block_public_policy", "ignore_public_acls", "restrict_public_buckets"] {
                if bool_attr(k) == Some(false) {
                    out.emit(
                        "TF002",
                        sp(attr(k).unwrap()),
                        format!("Make sure allowing public access with \"{k} = false\" is safe here."),
                    );
                }
            }
        }
        "aws_security_group" => {
            for ing in tf.blocks(body, "ingress") {
                sg_rule(tf, tf.body(ing), "cidr_blocks", "ipv6_cidr_blocks", "protocol", out);
            }
        }
        "aws_security_group_rule" => {
            if str_attr("type").as_deref() == Some("ingress") {
                sg_rule(tf, body, "cidr_blocks", "ipv6_cidr_blocks", "protocol", out);
            }
        }
        "aws_vpc_security_group_ingress_rule" => sg_rule(tf, body, "cidr_ipv4", "cidr_ipv6", "ip_protocol", out),
        "google_compute_firewall" => gcp_firewall(tf, body, out),
        "azurerm_network_security_rule" => azure_rule(tf, body, out),
        "azurerm_network_security_group" => {
            for r in tf.blocks(body, "security_rule") {
                azure_rule(tf, tf.body(r), out);
            }
        }
        _ => {}
    }
    if matches!(
        rtype,
        "aws_db_instance" | "aws_rds_cluster_instance" | "aws_redshift_cluster" | "aws_dms_replication_instance"
    ) && bool_attr("publicly_accessible") == Some(true)
    {
        out.emit(
            "TF004",
            sp(attr("publicly_accessible").unwrap()),
            "Make sure making this database reachable from the internet is safe here.",
        );
    }
    encryption(tf, rtype, block, body, ebs_default, out);
    if rtype.starts_with("aws_iam_") {
        let mut policies: Vec<Node> = attr("policy").into_iter().collect();
        for ip in tf.blocks(body, "inline_policy") {
            policies.extend(tf.attr(tf.body(ip), "policy"));
        }
        for p in policies {
            if let Some(v) = tf.json(p)
                && grants_all(&v)
            {
                out.emit("TF008", sp(p), "Make sure granting all actions on all resources is necessary.");
            }
        }
    }
    weak_tls(tf, rtype, body, out);
    match rtype {
        "google_storage_bucket_iam_binding"
        | "google_storage_bucket_iam_member"
        | "google_storage_bucket_access_control"
        | "google_storage_object_access_control"
        | "google_storage_default_object_access_control" => {
            for k in ["members", "member", "entity"] {
                for (m, n) in tf.strings(attr(k)) {
                    if m == "allUsers" || m == "allAuthenticatedUsers" {
                        out.emit(
                            "TF010",
                            sp(n),
                            format!("Make sure granting \"{m}\" access to this bucket is safe here."),
                        );
                    }
                }
            }
        }
        "google_storage_bucket_acl" | "google_storage_default_object_acl" | "google_storage_object_acl" => {
            for (m, n) in tf.strings(attr("role_entity")) {
                if m.ends_with(":allUsers") || m.ends_with(":allAuthenticatedUsers") {
                    out.emit("TF010", sp(n), "Make sure granting public access to this bucket is safe here.");
                }
            }
        }
        "azurerm_storage_account" => {
            for k in ["allow_nested_items_to_be_public", "allow_blob_public_access"] {
                if bool_attr(k) == Some(true) {
                    out.emit("TF011", sp(attr(k).unwrap()), "Make sure allowing public access to blobs is safe here.");
                }
            }
        }
        "azurerm_storage_container" => {
            if let Some(t) = str_attr("container_access_type")
                && (t == "blob" || t == "container")
            {
                out.emit(
                    "TF011",
                    sp(attr("container_access_type").unwrap()),
                    format!("Make sure allowing public \"{t}\" access is safe here."),
                );
            }
        }
        _ => {}
    }
}

/// An ingress rule open to the world on SSH, RDP or every port.
fn sg_rule(tf: &Tf, body: Option<Node>, v4: &str, v6: &str, proto: &str, out: &mut Out) {
    let cidrs: Vec<(String, Node)> =
        tf.strings(tf.attr(body, v4)).into_iter().chain(tf.strings(tf.attr(body, v6))).collect();
    let Some((_, at)) = cidrs.iter().find(|(c, _)| WORLD.contains(&c.as_str())) else { return };
    let Some(protocol) =
        tf.str_of(tf.attr(body, proto)).or_else(|| tf.num_of(tf.attr(body, proto)).map(|n| n.to_string()))
    else {
        return;
    };
    let protocol = protocol.to_ascii_lowercase();
    let what = if protocol == "-1" || protocol == "all" {
        Some("all ports")
    } else if protocol == "tcp" || protocol == "6" {
        let (Some(from), Some(to)) = (tf.num_of(tf.attr(body, "from_port")), tf.num_of(tf.attr(body, "to_port")))
        else {
            return;
        };
        exposed(from, to)
    } else {
        None
    };
    if let Some(what) = what {
        out.emit(
            "TF003",
            sp(*at),
            format!("Restrict access to {what} to specific IP addresses instead of the whole internet."),
        );
    }
}

fn exposed(from: f64, to: f64) -> Option<&'static str> {
    if from <= 0.0 && to >= 65535.0 {
        Some("all ports")
    } else if from <= 22.0 && to >= 22.0 {
        Some("SSH (port 22)")
    } else if from <= 3389.0 && to >= 3389.0 {
        Some("RDP (port 3389)")
    } else {
        None
    }
}

fn port_range(s: &str) -> Option<(f64, f64)> {
    let s = s.trim();
    if s == "*" {
        return Some((0.0, 65535.0));
    }
    match s.split_once('-') {
        Some((a, b)) => Some((a.trim().parse().ok()?, b.trim().parse().ok()?)),
        None => s.parse().ok().map(|p| (p, p)),
    }
}

fn gcp_firewall(tf: &Tf, body: Option<Node>, out: &mut Out) {
    if tf.str_of(tf.attr(body, "direction")).is_some_and(|d| d.eq_ignore_ascii_case("EGRESS")) {
        return;
    }
    let ranges = tf.strings(tf.attr(body, "source_ranges"));
    let Some((_, at)) = ranges.iter().find(|(c, _)| WORLD.contains(&c.as_str())) else { return };
    for allow in tf.blocks(body, "allow") {
        let ab = tf.body(allow);
        let Some(proto) = tf.str_of(tf.attr(ab, "protocol")) else { continue };
        let proto = proto.to_ascii_lowercase();
        let ports_attr = tf.attr(ab, "ports");
        let what = if proto == "all" || (proto == "tcp" && ports_attr.is_none()) {
            Some("all ports")
        } else if proto == "tcp" {
            tf.strings(ports_attr).iter().filter_map(|(p, _)| port_range(p)).find_map(|(a, b)| exposed(a, b))
        } else {
            None
        };
        if let Some(what) = what {
            out.emit(
                "TF003",
                sp(*at),
                format!("Restrict access to {what} to specific IP addresses instead of the whole internet."),
            );
            return;
        }
    }
}

fn azure_rule(tf: &Tf, body: Option<Node>, out: &mut Out) {
    let s = |k: &str| tf.str_of(tf.attr(body, k)).map(|v| v.to_ascii_lowercase());
    if s("direction").as_deref() != Some("inbound") || s("access").as_deref() != Some("allow") {
        return;
    }
    if !matches!(s("protocol").as_deref(), Some("tcp" | "*")) {
        return;
    }
    let Some(src) = tf.attr(body, "source_address_prefix") else { return };
    let Some(prefix) = tf.str_of(Some(src)) else { return };
    if !matches!(prefix.to_ascii_lowercase().as_str(), "*" | "0.0.0.0/0" | "internet" | "any") {
        return;
    }
    let mut ports: Vec<String> = s("destination_port_range").into_iter().collect();
    ports.extend(tf.strings(tf.attr(body, "destination_port_ranges")).into_iter().map(|(p, _)| p));
    if let Some(what) = ports.iter().filter_map(|p| port_range(p)).find_map(|(a, b)| exposed(a, b)) {
        out.emit(
            "TF003",
            sp(src),
            format!("Restrict access to {what} to specific IP addresses instead of the whole internet."),
        );
    }
}

fn encryption(tf: &Tf, rtype: &str, block: Node, body: Option<Node>, ebs_default: bool, out: &mut Out) {
    let attr = |n: &str| tf.attr(body, n);
    let off = |n: &str| tf.bool_of(attr(n)) == Some(false);
    let has = |n: &str| attr(n).is_some();
    let disabled = |out: &mut Out, n: Node, what: &str| {
        out.emit("TF005", sp(n), format!("Make sure {what} is encrypted; encryption is disabled here."));
    };
    let missing = |out: &mut Out, what: &str| {
        // The `resource` keyword.
        let at = Span::new(block.start_byte() as u32, block.start_byte() as u32 + 8);
        out.emit("TF006", at, format!("Make sure this {what} doesn't need encryption at rest; it isn't enabled."));
    };
    match rtype {
        "aws_db_instance" => {
            if off("storage_encrypted") {
                disabled(out, attr("storage_encrypted").unwrap(), "the database storage");
            } else if !has("storage_encrypted")
                && !["replicate_source_db", "snapshot_identifier", "kms_key_id", "restore_to_point_in_time"]
                    .iter()
                    .any(|k| has(k))
                && !tf.str_of(attr("engine")).is_some_and(|e| e.starts_with("aurora"))
                && !tf.str_of(attr("engine")).is_some_and(|e| e.contains("sqlserver-ex"))
            {
                missing(out, "database");
            }
        }
        "aws_rds_cluster" => {
            if off("storage_encrypted") {
                disabled(out, attr("storage_encrypted").unwrap(), "the cluster storage");
            } else if !has("storage_encrypted")
                && tf.str_of(attr("engine_mode")).as_deref() != Some("serverless")
                && ![
                    "snapshot_identifier",
                    "replication_source_identifier",
                    "kms_key_id",
                    "global_cluster_identifier",
                    "source_region",
                ]
                .iter()
                .any(|k| has(k))
            {
                missing(out, "database cluster");
            }
        }
        "aws_ebs_volume" => {
            if off("encrypted") {
                disabled(out, attr("encrypted").unwrap(), "the volume");
            } else if !has("encrypted") && !has("snapshot_id") && !has("kms_key_id") && !ebs_default {
                missing(out, "EBS volume");
            }
        }
        "aws_instance" | "aws_launch_template" | "aws_launch_configuration" => {
            for kind in ["root_block_device", "ebs_block_device"] {
                for b in tf.blocks(body, kind) {
                    let bb = tf.body(b);
                    if tf.bool_of(tf.attr(bb, "encrypted")) == Some(false) {
                        disabled(out, tf.attr(bb, "encrypted").unwrap(), "the block device");
                    }
                }
            }
        }
        "aws_efs_file_system" | "aws_redshift_cluster" => {
            if off("encrypted") {
                disabled(out, attr("encrypted").unwrap(), "the file system");
            }
        }
        "aws_elasticache_replication_group" => {
            if off("at_rest_encryption_enabled") {
                disabled(out, attr("at_rest_encryption_enabled").unwrap(), "the cache");
            }
        }
        "aws_elasticsearch_domain" | "aws_opensearch_domain" => {
            for b in tf.blocks(body, "encrypt_at_rest") {
                let bb = tf.body(b);
                if tf.bool_of(tf.attr(bb, "enabled")) == Some(false) {
                    disabled(out, tf.attr(bb, "enabled").unwrap(), "the domain");
                }
            }
        }
        "aws_sqs_queue" if off("sqs_managed_sse_enabled") && !has("kms_master_key_id") => {
            disabled(out, attr("sqs_managed_sse_enabled").unwrap(), "the queue");
        }
        _ => {}
    }
}

/// True if an IAM policy document allows every action on every resource.
fn grants_all(v: &Value) -> bool {
    let stmts: Vec<&Value> = match v.get("Statement") {
        Some(Value::Array(a)) => a.iter().collect(),
        Some(o @ Value::Object(_)) => vec![o],
        _ => return false,
    };
    let has_star = |v: Option<&Value>| match v {
        Some(Value::String(s)) => s == "*",
        Some(Value::Array(a)) => a.iter().any(|x| x.as_str() == Some("*")),
        _ => false,
    };
    stmts.iter().any(|s| {
        s.get("Effect").and_then(Value::as_str).unwrap_or("Allow") == "Allow"
            && s.get("Condition").is_none()
            && has_star(s.get("Action"))
            && has_star(s.get("Resource"))
    })
}

fn weak_tls(tf: &Tf, rtype: &str, body: Option<Node>, out: &mut Out) {
    let mut check = |n: Option<Node>, weak: &dyn Fn(&str) -> bool| {
        if let Some(n) = n
            && let Some(v) = tf.str_of(Some(n))
            && weak(&v)
        {
            out.emit("TF009", sp(n), format!("Require TLS 1.2 or later instead of \"{v}\"."));
        }
    };
    if rtype.starts_with("azurerm_") {
        check(tf.attr(body, "min_tls_version"), &|v| matches!(v, "TLS1_0" | "TLS1_1" | "1.0" | "1.1"));
        check(tf.attr(body, "minimum_tls_version"), &|v| matches!(v, "TLS1_0" | "TLS1_1" | "1.0" | "1.1"));
        for b in tf.blocks(body, "site_config") {
            check(tf.attr(tf.body(b), "minimum_tls_version"), &|v| matches!(v, "1.0" | "1.1"));
        }
    }
    match rtype {
        "aws_cloudfront_distribution" => {
            for b in tf.blocks(body, "viewer_certificate") {
                check(tf.attr(tf.body(b), "minimum_protocol_version"), &|v| {
                    matches!(v, "SSLv3" | "TLSv1" | "TLSv1_2016" | "TLSv1.1_2016")
                });
            }
        }
        "aws_elasticsearch_domain" | "aws_opensearch_domain" => {
            for b in tf.blocks(body, "domain_endpoint_options") {
                check(tf.attr(tf.body(b), "tls_security_policy"), &|v| v == "Policy-Min-TLS-1-0-2019-07");
            }
        }
        "aws_lb_listener" | "aws_alb_listener" => {
            check(tf.attr(body, "ssl_policy"), &|v| {
                v.starts_with("ELBSecurityPolicy-")
                    && (v.contains("TLS-1-0") || v.contains("TLS-1-1") || v.contains("2016-08") || v.contains("2015-"))
            });
        }
        "google_compute_ssl_policy" => {
            check(tf.attr(body, "min_tls_version"), &|v| matches!(v, "TLS_1_0" | "TLS_1_1"));
        }
        _ => {}
    }
}

fn module(tf: &Tf, body: Option<Node>, out: &mut Out) {
    let Some(src_node) = tf.attr(body, "source") else { return };
    let Some(source) = tf.str_of(Some(src_node)) else { return };
    let s = source.as_str();
    if s.starts_with("./") || s.starts_with("../") || s.starts_with('/') {
        return;
    }
    let git = s.starts_with("git::")
        || s.starts_with("git@")
        || s.starts_with("github.com/")
        || s.starts_with("bitbucket.org/");
    if git {
        if !s.contains("ref=") {
            out.emit("TF012", sp(src_node), "Pin this module source to a tag or commit with \"?ref=\".");
        }
        return;
    }
    if s.contains("::") || s.contains("://") || s.contains('?') {
        return;
    }
    let parts: Vec<&str> = s.split("//").next().unwrap_or(s).split('/').collect();
    let registry = match parts.as_slice() {
        [ns, name, provider] => !ns.contains('.') && !ns.contains(':') && !name.is_empty() && !provider.is_empty(),
        [host, _, _, _] => host.contains('.'),
        _ => false,
    };
    if registry && tf.attr(body, "version").is_none() {
        out.emit("TF012", sp(src_node), "Pin this registry module to a version with the \"version\" argument.");
    }
}

#[cfg(test)]
mod tests {
    use crate::tests::run;

    fn t(src: &str) -> Vec<(String, u32)> {
        run("main.tf", src, crate::Kind::Terraform)
    }

    #[test]
    fn s3() {
        let src = r#"resource "aws_s3_bucket" "a" {
  bucket = "a"
  acl    = "public-read"
}
resource "aws_s3_bucket_acl" "b" { acl = "private" }
resource "aws_s3_bucket_public_access_block" "c" {
  block_public_acls   = false
  block_public_policy = true
}
"#;
        assert_eq!(t(src), vec![("TF001".into(), 3), ("TF002".into(), 7)]);
    }

    #[test]
    fn security_groups() {
        let src = r#"resource "aws_security_group" "a" {
  ingress {
    from_port   = 22
    to_port     = 22
    protocol    = "tcp"
    cidr_blocks = ["0.0.0.0/0"]
  }
  ingress {
    from_port   = 443
    to_port     = 443
    protocol    = "tcp"
    cidr_blocks = ["0.0.0.0/0"]
  }
  ingress {
    from_port   = 22
    to_port     = 22
    protocol    = "tcp"
    cidr_blocks = ["10.0.0.0/8"]
  }
}
resource "aws_security_group_rule" "b" {
  type             = "ingress"
  from_port        = 0
  to_port          = 0
  protocol         = "-1"
  ipv6_cidr_blocks = ["::/0"]
}
resource "aws_vpc_security_group_ingress_rule" "c" {
  cidr_ipv4   = "0.0.0.0/0"
  from_port   = 3389
  to_port     = 3389
  ip_protocol = "tcp"
}
resource "google_compute_firewall" "d" {
  source_ranges = ["0.0.0.0/0"]
  allow {
    protocol = "tcp"
    ports    = ["80", "20-25"]
  }
}
resource "azurerm_network_security_rule" "e" {
  direction                  = "Inbound"
  access                     = "Allow"
  protocol                   = "Tcp"
  source_address_prefix      = "*"
  destination_port_range     = "3389"
}
"#;
        assert_eq!(
            t(src),
            vec![
                ("TF003".into(), 6),
                ("TF003".into(), 26),
                ("TF003".into(), 29),
                ("TF003".into(), 35),
                ("TF003".into(), 45)
            ]
        );
    }

    #[test]
    fn databases_and_encryption() {
        let src = r#"resource "aws_db_instance" "a" {
  publicly_accessible = true
  storage_encrypted   = false
  password            = var.db_password
}
resource "aws_db_instance" "b" {
  engine = "postgres"
}
resource "aws_db_instance" "c" {
  storage_encrypted = true
  password          = "Sup3r-S3cure!"
}
resource "aws_db_instance" "replica" { replicate_source_db = aws_db_instance.c.id }
resource "aws_ebs_volume" "d" { size = 10 }
resource "aws_ebs_volume" "e" {
  size      = 10
  encrypted = true
}
resource "aws_instance" "f" {
  root_block_device { encrypted = false }
}
"#;
        assert_eq!(
            t(src),
            vec![
                ("TF004".into(), 2),
                ("TF005".into(), 3),
                ("TF006".into(), 6),
                ("TF007".into(), 11),
                ("TF006".into(), 14),
                ("TF005".into(), 20),
            ]
        );
    }

    #[test]
    fn provider_secrets() {
        let src = r#"provider "aws" {
  access_key = "AKIAEXAMPLEKEY12"
  secret_key = "wJalrXUtnFEMI/K7MDENG/bPxRfiCY"
}
provider "github" { token = var.token }
variable "password" { default = "x" }
"#;
        assert_eq!(t(src), vec![("TF007".into(), 3)]);
    }

    #[test]
    fn iam() {
        let src = r#"data "aws_iam_policy_document" "a" {
  statement {
    actions   = ["*"]
    resources = ["*"]
  }
  statement {
    actions   = ["s3:GetObject"]
    resources = ["*"]
  }
  statement {
    effect    = "Deny"
    actions   = ["*"]
    resources = ["*"]
  }
}
resource "aws_iam_policy" "b" {
  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{ Effect = "Allow", Action = "*", Resource = "*" }]
  })
}
resource "aws_iam_role_policy" "c" {
  policy = <<EOF
{"Statement": [{"Effect": "Allow", "Action": ["*"], "Resource": "*"}]}
EOF
}
resource "aws_iam_role_policy" "d" {
  policy = <<EOF
{"Statement": [{"Effect": "Allow", "Action": ["s3:*"], "Resource": "${aws_s3_bucket.x.arn}"}]}
EOF
}
"#;
        assert_eq!(t(src), vec![("TF008".into(), 3), ("TF008".into(), 17), ("TF008".into(), 23)]);
    }

    #[test]
    fn tls_gcs_azure() {
        let src = r#"resource "azurerm_storage_account" "a" {
  min_tls_version                 = "TLS1_0"
  allow_nested_items_to_be_public = true
}
resource "azurerm_storage_account" "ok" { min_tls_version = "TLS1_2" }
resource "aws_cloudfront_distribution" "b" {
  viewer_certificate { minimum_protocol_version = "TLSv1" }
}
resource "aws_cloudfront_distribution" "ok" {
  viewer_certificate { minimum_protocol_version = "TLSv1.2_2021" }
}
resource "google_storage_bucket_iam_member" "c" {
  member = "allUsers"
}
resource "google_storage_bucket_iam_binding" "d" {
  members = ["user:a@b.c", "allAuthenticatedUsers"]
}
resource "azurerm_storage_container" "e" { container_access_type = "blob" }
"#;
        assert_eq!(
            t(src),
            vec![
                ("TF009".into(), 2),
                ("TF011".into(), 3),
                ("TF009".into(), 7),
                ("TF010".into(), 13),
                ("TF010".into(), 16),
                ("TF011".into(), 18),
            ]
        );
    }

    #[test]
    fn modules() {
        let src = r#"module "a" { source = "terraform-aws-modules/vpc/aws" }
module "b" {
  source  = "terraform-aws-modules/vpc/aws"
  version = "5.0.0"
}
module "c" { source = "git::https://github.com/org/mod.git" }
module "d" { source = "git::https://github.com/org/mod.git?ref=v1.2.0" }
module "e" { source = "./modules/x" }
module "f" { source = "github.com/org/mod" }
"#;
        assert_eq!(t(src), vec![("TF012".into(), 1), ("TF012".into(), 6), ("TF012".into(), 9)]);
    }

    #[test]
    fn tfvars_and_metrics() {
        assert_eq!(
            run("prod.tfvars", "db_password = \"Pr0d-Passw0rd!\"\nregion = \"x\"\n", crate::Kind::Terraform),
            vec![("TF007".into(), 1)]
        );
        assert_eq!(run("example.tfvars", "db_password = \"Pr0d-Passw0rd!\"\n", crate::Kind::Terraform), vec![]);
        let r = crate::tests::result("x.tf", "# a\n/* b\n c */\nlocals {\n  x = 1 // y\n}\n", crate::Kind::Terraform);
        assert_eq!((r.metrics.lines, r.metrics.ncloc, r.metrics.comment_lines), (7, 3, 4));
    }
}
