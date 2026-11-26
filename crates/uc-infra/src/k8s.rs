//! Kubernetes manifest rules (`K8S*`).

use crate::util::{Out, literal_secret_value, secret_name};
use crate::yaml::{Doc, V, Y, duplicate_keys};

pub const RULES: &[&str] =
    &["K8S001", "K8S002", "K8S003", "K8S004", "K8S005", "K8S006", "K8S007", "K8S008", "K8S009", "K8S010", "K8S011"];

/// True if the document is a Kubernetes object (`apiVersion`, `kind` and `metadata`).
pub(crate) fn is_object(doc: &Doc) -> bool {
    let r = &doc.root;
    let (Some(api), Some(_)) = (r.get("apiVersion").and_then(Y::str), r.get("kind").and_then(Y::str)) else {
        return false;
    };
    // Other tools borrow the Kubernetes object shape.
    if api.starts_with("backstage.io/") || api.starts_with("skaffold/") {
        return false;
    }
    r.get("metadata").is_some_and(Y::is_map)
}

/// The pod spec of a workload object, and whether the object is complete enough to judge what
/// it lacks (Kustomize patches are partial objects).
fn pod_spec(root: &Y) -> Option<(&Y, bool)> {
    let kind = root.get("kind")?.str()?;
    match kind {
        "Pod" => Some((root.get("spec")?, true)),
        "Deployment" | "StatefulSet" | "DaemonSet" | "ReplicaSet" | "ReplicationController" => {
            let spec = root.get("spec")?;
            let complete = kind == "ReplicationController" || spec.get("selector").is_some();
            Some((spec.path(&["template", "spec"])?, complete))
        }
        "Job" => Some((root.path(&["spec", "template", "spec"])?, true)),
        "CronJob" => Some((root.path(&["spec", "jobTemplate", "spec", "template", "spec"])?, true)),
        _ => None,
    }
}

pub(crate) fn check(src: &str, doc: &Doc, out: &mut Out) {
    if doc.has_error {
        return;
    }
    let root = &doc.root;
    if out.on("K8S011") {
        let mut d = Vec::new();
        duplicate_keys(root, &mut d);
        for (first, dup) in d {
            let line = out.lines.line(first.key_span.start);
            out.emit(
                "K8S011",
                dup.key_span,
                format!(
                    "Remove this duplicate key \"{}\" (first defined on line {line}); only the last value is used.",
                    dup.key
                ),
            );
        }
    }
    let Some((spec, complete)) = pod_spec(root) else { return };
    if !spec.is_map() {
        return;
    }
    for key in ["hostNetwork", "hostPID", "hostIPC"] {
        if let Some(p) = spec.pair(key)
            && p.val.is_true()
        {
            out.emit(
                "K8S003",
                p.key_span,
                format!("Make sure sharing the host's namespace with \"{key}: true\" is safe here."),
            );
        }
    }
    for v in spec.get("volumes").map_or(&[][..], Y::seq) {
        if let Some(p) = v.pair("hostPath") {
            out.emit("K8S006", p.key_span, "Make sure mounting a host path into the pod is safe here.");
        }
    }
    // Pod-level security context applies to every container that doesn't override it.
    let pod_sc = spec.get("securityContext");
    check_root(pod_sc, out);
    for list in ["initContainers", "containers"] {
        for c in spec.get(list).map_or(&[][..], Y::seq) {
            container(src, c, complete, list == "containers", out);
        }
    }
}

fn check_root(sc: Option<&Y>, out: &mut Out) {
    let Some(sc) = sc else { return };
    if let Some(p) = sc.pair("runAsUser")
        && p.val.str() == Some("0")
    {
        out.emit("K8S005", p.val.span, "Make sure running this container as root (\"runAsUser: 0\") is necessary.");
    }
    if let Some(p) = sc.pair("runAsNonRoot")
        && p.val.is_false()
    {
        out.emit("K8S005", p.val.span, "Make sure allowing this container to run as root is necessary.");
    }
}

fn container(src: &str, c: &Y, complete: bool, main: bool, out: &mut Out) {
    if !c.is_map() {
        return;
    }
    let sc = c.get("securityContext");
    if let Some(sc) = sc {
        if let Some(p) = sc.pair("privileged")
            && p.val.is_true()
        {
            out.emit("K8S001", p.key_span, "Make sure running this container in privileged mode is necessary.");
        }
        if let Some(p) = sc.pair("allowPrivilegeEscalation")
            && p.val.is_true()
        {
            out.emit("K8S002", p.key_span, "Make sure allowing privilege escalation is necessary.");
        }
        for cap in sc.path(&["capabilities", "add"]).map_or(&[][..], Y::seq) {
            if let Some(name) = cap.str() {
                let n = name.trim_start_matches("CAP_").to_ascii_uppercase();
                if matches!(n.as_str(), "SYS_ADMIN" | "NET_ADMIN" | "ALL" | "SYS_PTRACE" | "SYS_MODULE") {
                    out.emit("K8S004", cap.span, format!("Make sure adding the \"{name}\" capability is necessary."));
                }
            }
        }
        check_root(Some(sc), out);
    }
    let image = c.get("image");
    if let Some(img) = image
        && let Some(s) = img.str()
        && let Some(msg) = image_problem(s)
    {
        out.emit("K8S009", img.span, msg);
    }
    // Limits: only for complete objects with a real container definition.
    if complete && image.is_some() {
        let resources = c.get("resources");
        let unknown = resources.is_some_and(|r| matches!(r.v, V::Alias) || r.raw(src).contains('*'));
        if !unknown {
            let limits = resources.and_then(|r| r.get("limits"));
            let name_span = c.pair("name").map_or(c.span, |p| p.val.span);
            let at = c.pair("resources").map_or(name_span, |p| p.key_span);
            let has = |k: &str| limits.is_some_and(|l| l.get(k).is_some() || matches!(l.v, V::Alias));
            if !has("memory") {
                out.emit("K8S007", at, "Specify a memory limit for this container.");
            }
            if !has("cpu") && main {
                out.emit("K8S008", at, "Specify a CPU limit for this container.");
            }
        }
    }
    for e in c.get("env").map_or(&[][..], Y::seq) {
        let (Some(name), Some(val)) = (e.get("name").and_then(Y::str), e.get("value")) else { continue };
        if let Some(v) = val.str()
            && secret_name(name)
            && literal_secret_value(v)
        {
            out.emit(
                "K8S010",
                val.span,
                format!("Load \"{name}\" from a Secret (secretKeyRef) instead of hard-coding it."),
            );
        }
    }
}

/// What's wrong with an image reference's tag, if anything.
pub(crate) fn image_problem(image: &str) -> Option<&'static str> {
    if image.is_empty() || image.contains('$') || image.contains('{') || image.contains('@') {
        return None;
    }
    let last = image.rsplit('/').next().unwrap_or(image);
    match last.split_once(':') {
        None => Some("Specify a version tag for this image instead of the implicit \"latest\"."),
        Some((_, "latest")) => Some("Use a specific version tag for this image instead of \"latest\"."),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use crate::tests::run;

    fn k(src: &str) -> Vec<(String, u32)> {
        run("deploy.yaml", src, crate::Kind::Yaml)
    }

    const DEPLOY: &str = "apiVersion: apps/v1
kind: Deployment
metadata:
  name: web
spec:
  selector:
    matchLabels: {app: web}
  template:
    spec:
      hostNetwork: true
      securityContext:
        runAsNonRoot: false
      volumes:
        - name: h
          hostPath: {path: /var/run/docker.sock}
      containers:
        - name: web
          image: nginx
          securityContext:
            privileged: true
            allowPrivilegeEscalation: true
            runAsUser: 0
            capabilities:
              add: [\"SYS_ADMIN\", \"NET_BIND_SERVICE\"]
          env:
            - name: DB_PASSWORD
              value: \"pr0d-Pa55w0rd\"
            - name: API_TOKEN
              valueFrom: {secretKeyRef: {name: s, key: k}}
            - name: LOG_LEVEL
              value: debug
        - name: side
          image: busybox:latest
          resources:
            limits: {memory: 128Mi, cpu: 100m}
";

    #[test]
    fn noncompliant() {
        let got = k(DEPLOY);
        let keys: Vec<&str> = got.iter().map(|(k, _)| k.as_str()).collect();
        for (key, line) in [
            ("K8S003", 10),
            ("K8S005", 12),
            ("K8S006", 15),
            ("K8S009", 18),
            ("K8S001", 20),
            ("K8S002", 21),
            ("K8S005", 22),
            ("K8S004", 24),
            ("K8S010", 27),
            ("K8S007", 17),
            ("K8S008", 17),
            ("K8S009", 33),
        ] {
            assert!(got.contains(&(key.to_string(), line)), "{key}@{line} in {got:?}");
        }
        assert_eq!(keys.len(), 12, "{got:?}");
    }

    #[test]
    fn compliant() {
        let src = "apiVersion: v1
kind: Pod
metadata: {name: p}
spec:
  securityContext: {runAsNonRoot: true, runAsUser: 1000}
  containers:
    - name: app
      image: registry.local:5000/app:1.2.3
      securityContext:
        privileged: false
        allowPrivilegeEscalation: false
        capabilities: {drop: [ALL], add: [NET_BIND_SERVICE]}
      resources:
        limits: {memory: 1Gi, cpu: \"1\"}
      env:
        - {name: DB_PASSWORD, valueFrom: {secretKeyRef: {name: db, key: pw}}}
        - {name: ADMIN_PASSWORD, value: \"${ADMIN_PASSWORD}\"}
    - name: pinned
      image: nginx@sha256:abc
      resources: {limits: {memory: 1Gi, cpu: 1}}
";
        assert_eq!(k(src), vec![]);
    }

    #[test]
    fn partial_objects_and_helm() {
        // A Kustomize patch: no selector, so missing limits are not reported.
        let patch = "apiVersion: apps/v1\nkind: Deployment\nmetadata: {name: web}\nspec:\n  template:\n    spec:\n      containers:\n        - name: web\n          image: web:1.0\n";
        assert_eq!(k(patch), vec![]);
        let helm = "apiVersion: v1\nkind: Pod\nmetadata:\n  name: {{ .Values.name }}\nspec:\n  containers:\n    - image: nginx\n";
        assert_eq!(k(helm), vec![]);
    }

    #[test]
    fn cronjob_and_duplicates() {
        let src = "apiVersion: batch/v1
kind: CronJob
metadata:
  name: c
  name: d
spec:
  jobTemplate:
    spec:
      template:
        spec:
          hostPID: true
          containers:
            - {name: c, image: \"c:1\", resources: {limits: {memory: 1Gi}}}
";
        assert_eq!(k(src), vec![("K8S011".into(), 5), ("K8S003".into(), 11), ("K8S008".into(), 13)]);
    }

    #[test]
    fn metrics_and_text() {
        let r = crate::tests::result(
            "x.yaml",
            "# hi\napiVersion: v1\nkind: ConfigMap\nmetadata: {name: x}\n",
            crate::Kind::Yaml,
        );
        assert_eq!(r.lang, Some(uc_core::Lang::Yaml));
        assert_eq!((r.metrics.ncloc, r.metrics.comment_lines), (3, 1));
        let r = crate::tests::result("x.yaml", "foo: bar\n", crate::Kind::Yaml);
        assert_eq!(r.lang, Some(uc_core::Lang::Text));
        assert_eq!(r.metrics.lines, 0);
    }
}
