//! Token-shaped values are generated at run time so this file holds no secret-looking literals
//! (it would otherwise trip this scanner and push protection).

use super::*;

const UPPER: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZ";
const LOWER: &str = "abcdefghijklmnopqrstuvwxyz";
const DIGITS: &str = "0123456789";
const HEX: &str = "0123456789abcdef";

/// Deterministic pseudo-random string over `alphabet`.
fn rnd(seed: u64, len: usize, alphabet: &str) -> String {
    let a = alphabet.as_bytes();
    let mut x = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
    (0..len)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            a[(x % a.len() as u64) as usize] as char
        })
        .collect()
}

fn b62(seed: u64, len: usize) -> String {
    rnd(seed, len, &format!("{UPPER}{LOWER}{DIGITS}"))
}

fn b64(seed: u64, len: usize) -> String {
    rnd(seed, len, &format!("{UPPER}{LOWER}{DIGITS}+/"))
}

fn keys_in(path: &str, src: &str, is_test: bool) -> Vec<String> {
    let rules = RuleSet::all(Repo::Secrets, IMPLEMENTED);
    let lines = LineIndex::new(src);
    scan(path, src, &lines, &rules, is_test).into_iter().map(|i| i.rule).collect()
}

fn keys(path: &str, src: &str) -> Vec<String> {
    keys_in(path, src, false)
}

#[track_caller]
fn fires(key: &str, path: &str, src: &str) {
    assert_eq!(keys(path, src), vec![key.to_string()], "expected {key} in {path}: {src}");
}

#[track_caller]
fn silent(path: &str, src: &str) {
    assert!(keys(path, src).is_empty(), "expected nothing in {path}: {src} -> {:?}", keys(path, src));
}

#[test]
fn catalog_has_every_rule() {
    let rules = RuleSet::all(Repo::Secrets, IMPLEMENTED);
    assert_eq!(rules.len(), IMPLEMENTED.len());
    assert_eq!(RULES.len() + 1, IMPLEMENTED.len());
    for (r, k) in RULES.iter().zip(IMPLEMENTED) {
        assert_eq!(r.key, *k);
    }
}

#[test]
fn aws_access_key_id() {
    let id = format!("AKIA{}", rnd(1, 16, "ABCDEFGHIJKLMNOPQRSTUVWXYZ234567"));
    fires("SEC001", "src/config.js", &format!("const key = '{id}';"));
    fires("SEC001", "deploy.sh", &format!("export AWS_ACCESS_KEY_ID={id}\n"));
    // The docs example key and placeholders.
    silent("src/config.js", &format!("const key = '{}';", ["AKIA", "IOSFODNN7", "EXAMPLE"].concat()));
    silent("src/config.js", "const key = 'AKIAXXXXXXXXXXXXXXXX';");
    // Part of a longer word, or too long.
    silent("a.txt", &format!("X{id}"));
    silent("a.txt", &format!("{id}Q"));
    // An all-letter constant.
    silent("a.go", "const region = \"ASIAPACIFICSOUTHEAST\"");
}

#[test]
fn aws_secret_access_key() {
    let secret = b64(2, 40);
    fires("SEC002", "credentials", &format!("[default]\naws_secret_access_key = {secret}\n"));
    fires("SEC002", "main.go", &format!("SecretAccessKey: \"{secret}\","));
    silent(
        "credentials",
        &format!("aws_secret_access_key = {}\n", ["wJalrXUtnFEMI/K7MDENG/bPxRfiCY", "EXAMPLEKEY"].concat()),
    );
    silent("main.go", "SecretAccessKey: os.Getenv(\"AWS_SECRET_ACCESS_KEY\"),");
    silent("a.yml", "aws_secret_access_key: ${{ secrets.AWS_SECRET_ACCESS_KEY }}");
}

#[test]
fn github_tokens() {
    let classic = format!("ghp_{}", b62(3, 36));
    fires("SEC003", "src/a.ts", &format!("const t = \"{classic}\";"));
    fires("SEC003", "src/a.test.ts", &format!("const t = \"{classic}\";"));
    let fine = format!("github_pat_{}_{}", b62(4, 22), b62(5, 59));
    fires("SEC003", ".env", &format!("GH={fine}"));
    silent("src/a.ts", "const t = \"ghp_xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx\";");
    silent("src/a.ts", "const t = \"ghp_...\";");
    silent("src/a.ts", &format!("const t = \"x{classic}\";"));
    silent("src/a.ts", "if (token.startsWith('ghp_')) {}");
}

#[test]
fn gitlab_tokens() {
    fires("SEC004", "ci.sh", &format!("TOKEN=glpat-{}\n", b62(6, 20)));
    silent("ci.sh", "TOKEN=glpat-xxxxxxxxxxxxxxxxxxxx\n");
    silent("ci.sh", "TOKEN=glpat-your-personal-token-here\n");
}

#[test]
fn slack() {
    let bot = format!("xoxb-{}-{}-{}", rnd(7, 12, DIGITS), rnd(8, 13, DIGITS), b62(9, 24));
    fires("SEC005", "bot.py", &format!("SLACK = '{bot}'"));
    silent("bot.py", "SLACK = 'xoxb-your-token'");
    // Placeholders are assembled at run time so the source holds no token-shaped literal.
    silent("bot.py", &format!("SLACK = '{}'", ["xoxb", "1234567890", "1234567890", &"x".repeat(24)].join("-")));

    let hook = format!(
        "https://hooks.slack.com/services/T{}/B{}/{}",
        rnd(10, 8, "ABCDEFGHJKLMNPQRSTUVWXYZ0123456789"),
        rnd(11, 10, "ABCDEFGHJKLMNPQRSTUVWXYZ0123456789"),
        b62(12, 24)
    );
    fires("SEC006", "notify.js", &format!("fetch('{hook}')"));
    let placeholder = format!("https://hooks.slack.com/services/T{}/B{}/{}", "0".repeat(8), "0".repeat(8), "X".repeat(24));
    silent("notify.js", &format!("fetch('{placeholder}')"));
}

#[test]
fn stripe() {
    fires("SEC007", "pay.rb", &format!("Stripe.api_key = 'sk_live_{}'", b62(13, 24)));
    fires("SEC007", "pay.rb", &format!("key = 'rk_live_{}'", b62(14, 99)));
    silent("pay.rb", &format!("Stripe.api_key = 'sk_test_{}'", b62(13, 24)));
    silent("pay.js", &format!("const pk = 'pk_live_{}'", b62(15, 24)));
    silent("pay.rb", "Stripe.api_key = 'sk_live_...'");
}

#[test]
fn google_api_key() {
    fires(
        "SEC008",
        "maps.html",
        &format!(
            "<script src=\"https://maps.googleapis.com/maps/api/js?key=AIza{}\">",
            rnd(16, 35, &format!("{UPPER}{LOWER}{DIGITS}-_"))
        ),
    );
    silent("maps.html", "key=AIzaSyXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX");
    silent("maps.js", "const k = 'AIza' + rest;");
}

#[test]
fn private_keys() {
    // PKCS#8 framing (`SEQUENCE { INTEGER 0, ...`), random content.
    let mut body: Vec<String> = (0..10).map(|i| b64(100 + i, 64)).collect();
    body[0] = format!("MIIEvQIBADANBgkqhkiG9w0BAQEFAASC{}", b64(99, 32));
    let pem = format!("-----BEGIN RSA PRIVATE KEY-----\n{}\n-----END RSA PRIVATE KEY-----\n", body.join("\n"));
    fires("SEC009", "deploy/id_rsa", &pem);
    // Escaped in a JSON service-account file.
    let json = format!(
        "{{\"private_key\": \"-----BEGIN PRIVATE KEY-----\\n{}\\n-----END PRIVATE KEY-----\\n\"}}",
        body.join("\\n")
    );
    fires("SEC009", "service-account.json", &json);
    // Test fixtures, placeholders, code that only mentions the header, encrypted keys.
    silent("src/crypto_test.go", &pem);
    silent("testdata/key.pem", &pem);
    silent("a.md.txt", "-----BEGIN PRIVATE KEY-----\n...\n-----END PRIVATE KEY-----");
    silent("a.txt", "-----BEGIN PRIVATE KEY-----\n<your key here>\n-----END PRIVATE KEY-----");
    silent("a.go", "if strings.HasPrefix(s, \"-----BEGIN PRIVATE KEY-----\") {");
    silent("a.pem", &pem.replace("RSA PRIVATE", "ENCRYPTED PRIVATE"));
    silent("a.pem", &pem.replace("RSA PRIVATE KEY", "CERTIFICATE"));
    // A certificate body (`SEQUENCE { SEQUENCE ...`) under a private-key header.
    let mut cert = body.clone();
    cert[0] = format!("MIIFRDCCAyygAwIBAgIU{}", b64(98, 44));
    silent(
        "a.ts",
        &format!("const k = \"-----BEGIN PRIVATE KEY-----\\n{}\\n-----END PRIVATE KEY-----\";", cert.join("\\n")),
    );
    // OpenSSH keys.
    let mut ssh = body.clone();
    ssh[0] = format!("b3BlbnNzaC1rZXktdjEAAAAA{}", b64(97, 40));
    fires(
        "SEC009",
        "deploy/key",
        &format!("-----BEGIN OPENSSH PRIVATE KEY-----\n{}\n-----END OPENSSH PRIVATE KEY-----\n", ssh.join("\n")),
    );
}

#[test]
fn inline_tests_and_scanner_configs() {
    let pw = b62(50, 16);
    let url = format!("let u = \"postgres://app:{pw}@db.prod.acme-corp.com/app\";\n");
    fires("SEC020", "src/db.rs", &url);
    silent("src/db.rs", &format!("fn f() {{}}\n#[cfg(test)]\nmod tests {{\n{url}}}\n"));
    // Provider rules still apply in inline tests.
    let tok = format!("let t = \"ghp_{}\";\n", b62(51, 36));
    fires("SEC003", "src/db.rs", &format!("#[cfg(test)]\nmod tests {{\n{tok}}}\n"));
    silent(".gitleaks.toml", &tok);
    silent("catalog/gitleaks.toml", &tok);
}

#[test]
fn documented_values() {
    let tok = format!("{}:AA{}", rnd(40, 10, DIGITS), rnd(41, 33, &format!("{UPPER}{LOWER}{DIGITS}-_")));
    silent("a.tsx", &format!("<Input placeholder=\"{tok}\" />"));
    silent("a.py", &format!("# Example: TOKEN = '{tok}'"));
    let id = format!("AKIA{}", rnd(42, 16, "ABCDEFGHIJKLMNOPQRSTUVWXYZ234567"));
    silent("a.rb", &format!("url = 'https://b.s3.amazonaws.com/a.jpg?AWSAccessKeyId={id}&Expires=1&Signature=x'"));
    let g = format!("AIza{}", rnd(43, 35, &format!("{UPPER}{LOWER}{DIGITS}-_")));
    silent("android/app/google-services.json", &format!("{{\"current_key\": \"{g}\"}}"));
    fires("SEC008", "src/maps.js", &format!("const key = \"{g}\";"));
}

#[test]
fn openai() {
    let legacy = format!("sk-{}T3BlbkFJ{}", b62(17, 20), b62(18, 20));
    fires("SEC010", "app.py", &format!("openai.api_key = \"{legacy}\""));
    let proj = format!("sk-proj-{}", rnd(19, 156, &format!("{UPPER}{LOWER}{DIGITS}-_")));
    fires("SEC010", "app.py", &format!("OPENAI_API_KEY=\"{proj}\""));
    silent("app.py", "OPENAI_API_KEY=\"sk-proj-...\"");
    silent("app.py", "OPENAI_API_KEY=\"sk-proj-xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx\"");
}

#[test]
fn anthropic() {
    let key = format!("sk-ant-api03-{}AA", rnd(20, 93, &format!("{UPPER}{LOWER}{DIGITS}-_")));
    fires("SEC011", "app.ts", &format!("new Anthropic({{ apiKey: '{key}' }})"));
    silent("app.ts", "new Anthropic({ apiKey: 'sk-ant-api03-...' })");
    silent("app.ts", "new Anthropic({ apiKey: process.env.ANTHROPIC_API_KEY })");
}

#[test]
fn npm() {
    let tok = format!("npm_{}", b62(21, 36));
    fires("SEC012", ".github/publish.sh", &format!("NPM_TOKEN={tok}"));
    silent("build.sh", "echo $npm_package_version $npm_config_registry_with_a_long_name_here");
    // In .npmrc the npm-format value is reported once, by the more specific rule.
    fires("SEC012", ".npmrc", &format!("//registry.npmjs.org/:_authToken={tok}\n"));
    let other = rnd(22, 36, &format!("{HEX}-"));
    fires("SEC013", ".npmrc", &format!("//npm.pkg.github.com/:_authToken={other}\n"));
    fires("SEC013", ".yarnrc.yml", &format!("npmAuthToken: \"{other}\"\n"));
    silent(".npmrc", "//registry.npmjs.org/:_authToken=${NPM_TOKEN}\n");
    silent("Dockerfile", "RUN echo \"//registry.npmjs.org/:_authToken=$NPM_TOKEN\" > .npmrc");
    silent(".npmrc", "//registry.npmjs.org/:_authToken=YOUR_TOKEN_HERE_1234\n");
}

#[test]
fn sendgrid() {
    let k = format!("SG.{}.{}", b62(23, 22), b62(24, 43));
    fires("SEC014", "mail.php", &format!("$key = '{k}';"));
    silent("mail.php", "$key = 'SG.xxxxxxxxxxxxxxxxxxxxxx.xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx';");
    silent("mail.js", "MSG.send()");
}

#[test]
fn shopify() {
    fires("SEC015", "shop.rb", &format!("TOKEN = 'shpat_{}'", rnd(25, 32, HEX)));
    silent("shop.rb", &format!("TOKEN = 'shpat_{}'", "0".repeat(32)));
}

#[test]
fn huggingface() {
    fires("SEC016", "train.py", &format!("token = 'hf_{}'", rnd(26, 34, &format!("{UPPER}{LOWER}"))));
    silent("train.py", "x = hf_thisisaverylongsnakecaseidentifiers");
    silent("train.py", "token = 'hf_xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx'");
}

#[test]
fn digitalocean() {
    fires("SEC017", "do.tf", &format!("token = \"dop_v1_{}\"", rnd(27, 64, HEX)));
    silent("do.tf", "token = \"dop_v1_xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx\"");
}

#[test]
fn telegram() {
    let tok = format!("{}:AA{}", rnd(28, 10, DIGITS), rnd(29, 33, &format!("{UPPER}{LOWER}{DIGITS}-_")));
    fires("SEC018", "bot.py", &format!("TOKEN = '{tok}'"));
    fires("SEC018", "bot.py", &format!("url = 'https://api.telegram.org/bot{tok}/getMe'"));
    silent("bot.py", "TOKEN = '123456789:AAxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx'");
    silent("net.txt", "mac 00:AA:BB:CC:DD:EE");
}

#[test]
fn azure() {
    let key = format!("{}==", b64(30, 86));
    fires(
        "SEC019",
        "appsettings.config",
        &format!("DefaultEndpointsProtocol=https;AccountName=acct;AccountKey={key};EndpointSuffix=core.windows.net"),
    );
    let azurite = std::str::from_utf8(AZURITE_KEY).unwrap();
    silent("appsettings.config", &format!("AccountName=devstoreaccount1;AccountKey={azurite};"));
    silent("appsettings.config", "AccountKey=<your-account-key>;");
}

#[test]
fn connection_urls() {
    let pw = b62(31, 16);
    fires("SEC020", "src/db.ts", &format!("const url = 'postgres://app:{pw}@db.prod.acme-corp.com:5432/app';"));
    fires("SEC020", "settings.py", &format!("DB = 'mongodb+srv://admin:{pw}@cluster0.ab1cd.mongodb.net/test'"));
    fires("SEC020", "settings.py", &format!("DB = 'postgresql+psycopg2://app:{pw}@10.0.3.4/app'"));
    // References, placeholders, local and example hosts, docker service names.
    silent("src/db.ts", "const url = `postgres://${user}:${password}@${host}/db`;");
    silent("src/db.ts", "const url = 'postgres://user:password@db.acme-corp.com/db';");
    silent("src/db.ts", "const url = 'postgres://user:$DB_PASSWORD@db.acme-corp.com/db';");
    silent("src/db.ts", "const url = 'postgres://user:<password>@db.acme-corp.com/db';");
    silent("src/db.ts", "const url = 'redis://:***@cache.acme-corp.com';");
    silent("src/db.ts", &format!("const url = 'postgres://app:{pw}@localhost:5432/app';"));
    silent("src/db.ts", &format!("const url = 'postgres://app:{pw}@db:5432/app';"));
    silent("src/db.ts", &format!("const url = 'postgres://app:{pw}@db.example.com/app';"));
    silent("src/db.ts", "const url = 'mysql://root:changeme@db.acme-corp.com/app';");
    // Test code.
    silent("src/db.test.ts", &format!("const url = 'postgres://app:{pw}@db.prod.acme-corp.com/app';"));
}

#[test]
fn generic_config_secrets() {
    let v = b62(32, 32);
    fires("SEC021", ".env", &format!("STRIPE_WEBHOOK_SECRET={v}\n"));
    fires("SEC021", "config/application.properties", &format!("spring.datasource.password={v}\n"));
    fires("SEC021", "config.yml", &format!("auth:\n  client_secret: \"{v}\"\n"));
    fires("SEC021", "settings.toml", &format!("api_key = '{v}'\n"));
    fires("SEC021", "prod.tfvars", &format!("db_password = \"{v}\"\n"));
    fires("SEC021", ".env.production", &format!("export JWT_SECRET_KEY={v}\n"));

    // Not configuration files: code has its own credential rules.
    silent("src/a.js", &format!("const password = '{v}';"));
    silent("main.py", &format!("PASSWORD = '{v}'"));
    // Example files, test paths.
    silent(".env.example", &format!("JWT_SECRET={v}\n"));
    silent("test/fixtures/config.yml", &format!("password: {v}\n"));
    // References, placeholders, low entropy, names, paths.
    silent(".env", "DB_PASSWORD=${DB_PASSWORD}\n");
    silent(".env", "DB_PASSWORD=changeme\n");
    silent(".env", "DB_PASSWORD=postgres\n");
    silent(".env", "API_KEY=your-api-key-here\n");
    silent(".env", "API_KEY=xxxxxxxxxxxxxxxxxxxx\n");
    silent(".env", "SECRET_KEY=\"change-me-8y1prl&k=xs=xd6uxb#evgo-7@s*iz+&i\"\n");
    silent(".env", "GEMINI_API_KEY=PLACEHOLDER_API_KEY\n");
    silent("docker-compose.yml", "      WAHA_API_KEY: myapp-local-key\n");
    silent("docker-compose.yml", "      POSTGRES_PASSWORD: your_secure_postgres_password # CHANGE THIS\n");
    silent(".env", &format!("MUX_TOKEN_ID={}-{}\n", rnd(52, 8, HEX), rnd(53, 12, HEX)));
    fires("SEC021", ".env", &format!("MUX_TOKEN_SECRET={}\n", b62(54, 40)));
    fires("SEC021", ".env", &format!("WAHA_API_KEY=\"{}\"\n", rnd(55, 32, HEX)));
    silent("config.yml", "password: \"{{ vault_db_password }}\"\n");
    silent("config.yml", "secret_name: my-app-secret-2\n");
    silent("config.yml", "token_url: https://auth.acme.com/oauth/token\n");
    silent("config.yml", "private_key: /etc/ssl/private/server.key\n");
    silent("config.yml", "api_key_file: keys/api_1234567.txt\n");
    silent("config.yml", "max_tokens: 4096\n");
    silent("config.yml", "public_key: MFkwEwYHKoZIzj0CAQYIKoZIzj0DAQcDQgAE\n");
    silent("config.yml", "token: app-config-v2\n");
    silent("config.yml", "password: !vault |\n");
    silent("config.yml", "label: secret token 12345678\n");
    silent("application.properties", "spring.datasource.password=ENC(G6N718UuyPE5bHyWKyuLQSm02auQPUtm)\n");
    // Base64 of a placeholder (Kubernetes Secret data).
    silent("secret.yaml", "data:\n  password: cGFzc3dvcmQxMjM0NTY=\n");
    // Already reported by a provider rule.
    fires("SEC003", ".env", &format!("GITHUB_TOKEN=ghp_{}\n", b62(33, 36)));
}

#[test]
fn locations_and_scope() {
    let rules = RuleSet::all(Repo::Secrets, IMPLEMENTED);
    let src = format!("line one\nconst t = 'ghp_{}';\n", b62(34, 36));
    let lines = LineIndex::new(&src);
    let issues = scan("a.js", &src, &lines, &rules, false);
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].loc.line, 2);
    assert_eq!(issues[0].repo, Repo::Secrets);
    assert!(issues[0].message.contains("GitHub token"), "{}", issues[0].message);
    // Main-only rules skip test files; provider rules don't.
    let v = b62(35, 32);
    assert!(keys_in(".env", &format!("JWT_SECRET={v}\n"), true).is_empty());
    assert_eq!(keys_in("a.js", &src, true), vec!["SEC003".to_string()]);
}

#[test]
fn helpers() {
    assert_eq!(segments(b"spring.datasource.password"), vec!["spring", "datasource", "password"]);
    assert_eq!(segments(b"clientSecretKey"), vec!["client", "secret", "key"]);
    assert!(secret_key(b"SECRET_KEY_BASE"));
    assert!(secret_key(b"x-api-key"));
    assert!(!secret_key(b"api_key_file"));
    assert!(!secret_key(b"primary_key"));
    assert!(!secret_key(b"tokenizer"));
    assert_eq!(base64_text(b"cGFzc3dvcmQ=").as_deref(), Some(&b"password"[..]));
    assert!(entropy(b"aaaa") < 0.1);
    assert!(test_like("pkg/testdata/a.pem"));
    assert!(test_like("src/test/java/FooTest.java"));
    assert!(!test_like("src/main/java/Foo.java"));
}
