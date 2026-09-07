use image::DynamicImage;
use std::{
    io::{Cursor, Write},
    path::PathBuf,
    process::{Command, Output, Stdio},
    sync::{Arc, Mutex},
    time::Duration,
};
const BIN: &str = env!("CARGO_BIN_EXE_img");
#[test]
fn webdav_lists_namespaced_properties_and_rechecks_version_before_delete() {
    let s = tiny_http::Server::http("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}/dav", s.server_addr());
    let f = Fixture::new(&endpoint);
    std::fs::write(&f.config,format!("version=1\ndefault_provider='local'\n[providers.local]\ntype='webdav'\nendpoint='{endpoint}'\npublic_url='https://cdn.test'\nallow_insecure=true\n")).unwrap();
    let handle = std::thread::spawn(move || {
        for _ in 0..2 {
            let r = s.recv_timeout(Duration::from_secs(8)).unwrap().unwrap();
            assert_eq!(r.method().as_str(), "PROPFIND");
            assert_eq!(
                r.headers()
                    .iter()
                    .find(|h| h.field.equiv("depth"))
                    .unwrap()
                    .value
                    .as_str(),
                "1"
            );
            r.respond(tiny_http::Response::from_string(r#"<d:multistatus xmlns:d="DAV:"><d:response><d:href>/dav/</d:href><d:propstat><d:status>HTTP/1.1 200 OK</d:status><d:prop><d:resourcetype><d:collection/></d:resourcetype></d:prop></d:propstat></d:response><d:response><d:href>/dav/photo%20one.png</d:href><d:propstat><d:status>HTTP/1.1 200 OK</d:status><d:prop><d:resourcetype/><d:getcontentlength>12</d:getcontentlength><d:getetag>"v1"</d:getetag></d:prop></d:propstat></d:response></d:multistatus>"#).with_status_code(207)).unwrap();
        }
        let r = s.recv_timeout(Duration::from_secs(8)).unwrap().unwrap();
        assert_eq!(r.method().as_str(), "DELETE");
        assert_eq!(r.url(), "/dav/photo%20one.png");
        assert_eq!(
            r.headers()
                .iter()
                .find(|h| h.field.equiv("if-match"))
                .unwrap()
                .value
                .as_str(),
            "\"v1\""
        );
        r.respond(tiny_http::Response::empty(204)).unwrap();
    });
    let list = f.run(&["remote", "list"]);
    assert!(
        list.status.success(),
        "{}",
        String::from_utf8_lossy(&list.stderr)
    );
    assert_eq!(parsed(&list)["items"].as_array().unwrap().len(), 1);
    assert_eq!(parsed(&list)["items"][0]["path"], "photo one.png");
    assert_eq!(parsed(&list)["items"][0]["directory"], false);
    let deleted = f.run(&[
        "remote",
        "delete",
        "photo one.png",
        "--version",
        "\"v1\"",
        "--yes",
    ]);
    assert!(
        deleted.status.success(),
        "{}",
        String::from_utf8_lossy(&deleted.stderr)
    );
    handle.join().unwrap();
}
#[test]
fn remote_s3_lists_pages_and_deletes_only_with_explicit_version() {
    let s = tiny_http::Server::http("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}", s.server_addr());
    let f = Fixture::new(&endpoint);
    std::fs::write(&f.config,format!("version=1\nallow_plaintext_credentials=true\ndefault_provider='local'\n[providers.local]\ntype='s3'\nendpoint='{endpoint}'\nbucket='images'\nregion='test'\naccess_key='test'\nsecret_key='test'\npublic_url='https://cdn.test'\npath_style=true\nallow_insecure=true\n")).unwrap();
    let handle = std::thread::spawn(move || {
        let r = s.recv_timeout(Duration::from_secs(8)).unwrap().unwrap();
        assert_eq!(r.method().as_str(), "GET");
        assert!(r.url().contains("prefix=folder%2F"));
        assert!(r.url().contains("continuation-token=opaque"));
        r.respond(tiny_http::Response::from_string(r#"<ListBucketResult><IsTruncated>true</IsTruncated><NextContinuationToken>next</NextContinuationToken><Contents><Key>folder/a &amp; b.png</Key><Size>12</Size><ETag>"v1"</ETag></Contents><CommonPrefixes><Prefix>folder/sub/</Prefix></CommonPrefixes></ListBucketResult>"#)).unwrap();
        let r = s.recv_timeout(Duration::from_secs(8)).unwrap().unwrap();
        assert_eq!(r.method().as_str(), "DELETE");
        assert_eq!(r.url(), "/images/folder/a%20%26%20b.png");
        assert_eq!(
            r.headers()
                .iter()
                .find(|h| h.field.equiv("if-match"))
                .unwrap()
                .value
                .as_str(),
            "\"v1\""
        );
        r.respond(tiny_http::Response::empty(412)).unwrap();
    });
    let list = f.run(&[
        "remote", "list", "--prefix", "folder/", "--cursor", "opaque",
    ]);
    assert!(
        list.status.success(),
        "{}",
        String::from_utf8_lossy(&list.stderr)
    );
    assert_eq!(parsed(&list)["next"], "next");
    assert_eq!(parsed(&list)["items"][0]["path"], "folder/a & b.png");
    assert_eq!(parsed(&list)["items"][1]["directory"], true);
    assert!(
        !f.run(&[
            "remote",
            "delete",
            "folder/a & b.png",
            "--version",
            "\"v1\""
        ])
        .status
        .success()
    );
    assert!(
        !f.run(&[
            "remote",
            "delete",
            "folder/a & b.png",
            "--version",
            "\"v1\"",
            "--yes"
        ])
        .status
        .success()
    );
    handle.join().unwrap();
}
#[test]
fn webdav_upload_creates_collections_and_preserves_existing_files() {
    let s = tiny_http::Server::http("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}/dav", s.server_addr());
    let f = Fixture::new(&endpoint);
    std::fs::write(&f.config,format!("version=1\ndefault_provider='local'\n[upload]\npath_template='folder/{{filename}}'\n[providers.local]\ntype='webdav'\nendpoint='{endpoint}'\npublic_url='https://cdn.test'\nallow_insecure=true\n")).unwrap();
    let handle = std::thread::spawn(move || {
        let r = s.recv_timeout(Duration::from_secs(8)).unwrap().unwrap();
        assert_eq!(r.method().as_str(), "MKCOL");
        assert_eq!(r.url(), "/dav/folder/");
        r.respond(tiny_http::Response::empty(405)).unwrap();
        let mut r = s.recv_timeout(Duration::from_secs(8)).unwrap().unwrap();
        assert_eq!(r.method().as_str(), "PUT");
        assert_eq!(r.url(), "/dav/folder/picture.png");
        assert_eq!(
            r.headers()
                .iter()
                .find(|h| h.field.equiv("if-none-match"))
                .unwrap()
                .value
                .as_str(),
            "*"
        );
        let mut bytes = vec![];
        r.as_reader().read_to_end(&mut bytes).unwrap();
        assert!(bytes.starts_with(b"\x89PNG"));
        r.respond(tiny_http::Response::empty(412)).unwrap();
    });
    let before = std::fs::read(&f.image).unwrap();
    let out = f.command().arg("upload").arg(&f.image).output().unwrap();
    assert!(!out.status.success());
    assert_eq!(std::fs::read(&f.image).unwrap(), before);
    handle.join().unwrap();
}
#[test]
fn backup_cli_preview_does_not_restore_and_apply_retains_recovery() {
    let f = Fixture::new("https://test.invalid");
    let destination = f.dir.path().join("backup");
    assert!(
        f.command()
            .arg("backup")
            .arg(&destination)
            .output()
            .unwrap()
            .status
            .success()
    );
    std::fs::write(&f.config, "version=1\n").unwrap();
    let before = std::fs::read(&f.config).unwrap();
    assert!(
        f.command()
            .arg("restore")
            .arg(&destination)
            .output()
            .unwrap()
            .status
            .success()
    );
    assert_eq!(std::fs::read(&f.config).unwrap(), before);
    let result = f
        .command()
        .arg("restore")
        .arg(&destination)
        .arg("--apply")
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(std::path::Path::new(parsed(&result)["recovery"].as_str().unwrap()).is_dir());
    assert_ne!(std::fs::read(&f.config).unwrap(), before);
}
struct Fixture {
    dir: tempfile::TempDir,
    config: PathBuf,
    image: PathBuf,
}
impl Fixture {
    fn new(url: &str) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config.toml");
        std::fs::write(&config,format!("version=1\ndefault_provider='local'\n[upload]\npath_template='{{filename}}'\n[providers.local]\ntype='http'\nurl='{url}'\nurl_json_path='data.url'\nallow_insecure=true\n")).unwrap();
        let image = dir.path().join("picture.png");
        DynamicImage::new_rgb8(100, 50).save(&image).unwrap();
        Self { dir, config, image }
    }
    fn command(&self) -> Command {
        let mut c = Command::new(BIN);
        c.current_dir(self.dir.path())
            .env("IMG_DATA_DIR", self.dir.path().join("data"))
            .arg("--config")
            .arg(&self.config);
        for e in [
            "IMG_PROVIDER",
            "IMG_DEFAULT_PROVIDER",
            "IMG_OUTPUT_FORMAT",
            "IMG_OUTPUT_COPY",
            "IMG_UPLOAD_CONCURRENCY",
        ] {
            c.env_remove(e);
        }
        c
    }
    fn run(&self, args: &[&str]) -> Output {
        self.command().args(args).output().unwrap()
    }
}
type ServerCapture = (
    String,
    Arc<Mutex<Vec<Vec<u8>>>>,
    std::thread::JoinHandle<()>,
);
fn server(replies: Vec<(u16, Vec<u8>)>) -> ServerCapture {
    let s = tiny_http::Server::http("127.0.0.1:0").unwrap();
    let url = format!("http://{}/image.png", s.server_addr());
    let seen = Arc::new(Mutex::new(vec![]));
    let copy = seen.clone();
    let h = std::thread::spawn(move || {
        for (code, body) in replies {
            let mut r = s
                .recv_timeout(Duration::from_secs(6))
                .unwrap()
                .expect("expected CLI request");
            let mut b = vec![];
            r.as_reader().read_to_end(&mut b).unwrap();
            copy.lock().unwrap().push(b);
            r.respond(tiny_http::Response::from_data(body).with_status_code(code))
                .unwrap();
        }
    });
    (url, seen, h)
}
fn ok_body() -> Vec<u8> {
    br#"{"data":{"url":"https://cdn.test/picture.png"}}"#.to_vec()
}
fn parsed(out: &Output) -> serde_json::Value {
    assert!(
        !out.stdout.is_empty(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap()
}
#[test]
fn uploads_publish_agent_records_and_local_failure_keeps_remote_success() {
    let (url, _, handle) = server(vec![(200, ok_body()), (200, ok_body()), (200, ok_body())]);
    let f = Fixture::new(&url);
    let out = f.run(&[
        "upload",
        f.image.to_str().unwrap(),
        "--format",
        "json",
        "--origin",
        "agent",
    ]);
    assert!(out.status.success());
    let data = f.dir.path().join("data/upload-inbox");
    let entries: Vec<_> = std::fs::read_dir(&data).unwrap().collect();
    assert_eq!(entries.len(), 1);
    let record: serde_json::Value = serde_json::from_slice(
        &std::fs::read(entries[0].as_ref().unwrap().path().join("record.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(record["origin"], "agent");
    assert_eq!(record["url"], "https://cdn.test/picture.png");
    let out = f.run(&[
        "upload",
        f.image.to_str().unwrap(),
        "--no-history",
        "--format",
        "json",
    ]);
    assert!(out.status.success());
    assert_eq!(std::fs::read_dir(&data).unwrap().count(), 1);
    let blocked = f.dir.path().join("blocked-data");
    std::fs::write(&blocked, b"preserve").unwrap();
    let out = f
        .command()
        .env("IMG_DATA_DIR", &blocked)
        .args(["upload", f.image.to_str().unwrap(), "--format", "json"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let result = parsed(&out);
    assert_eq!(result["files"][0]["success"], true);
    assert!(
        result["files"][0]["record_warning"]
            .as_str()
            .unwrap()
            .contains("could not be saved")
    );
    assert_eq!(std::fs::read(blocked).unwrap(), b"preserve");
    handle.join().unwrap();
}
#[test]
fn repeated_bytes_reuse_only_with_opt_in_and_force_transfers_again() {
    let (url, seen, handle) = server(vec![(200, ok_body()), (200, ok_body())]);
    let f = Fixture::new(&url);
    let args = [
        "upload",
        f.image.to_str().unwrap(),
        "--reuse",
        "--format",
        "json",
    ];
    assert_eq!(
        parsed(&f.run(&args))["files"][0]["reused"],
        serde_json::Value::Null
    );
    assert_eq!(parsed(&f.run(&args))["files"][0]["reused"], true);
    let out = f.command().args(args).arg("--force").output().unwrap();
    assert!(out.status.success());
    assert_eq!(parsed(&out)["files"][0]["reused"], serde_json::Value::Null);
    handle.join().unwrap();
    assert_eq!(seen.lock().unwrap().len(), 2);
}
#[test]
fn migration_preview_never_writes_or_prints_credentials() {
    let f = Fixture::new("https://unused.test");
    let source = f.dir.path().join("picgo.json");
    let bytes = br#"{"picBed":{"github":{"repo":"me/img","token":"do-not-print"}}}"#;
    std::fs::write(&source, bytes).unwrap();
    let before = std::fs::read(&f.config).unwrap();
    let result = f.run(&["import-config", source.to_str().unwrap()]);
    assert!(result.status.success());
    assert!(!String::from_utf8_lossy(&result.stdout).contains("do-not-print"));
    assert!(!String::from_utf8_lossy(&result.stderr).contains("do-not-print"));
    assert_eq!(std::fs::read(&f.config).unwrap(), before);
    assert_eq!(std::fs::read(&source).unwrap(), bytes);
}
#[test]
fn json_setup_errors_keep_exit_code_and_hide_config_contents() {
    let f = Fixture::new("https://unused.test");
    std::fs::write(
        &f.config,
        "version=1\nprivate-token = 'secret-not-to-export\n",
    )
    .unwrap();
    let out = f.run(&["upload", "image.png", "--format", "json"]);
    assert_eq!(out.status.code(), Some(2));
    let doc = parsed(&out);
    assert_eq!(doc["files"][0]["error_code"], "invalid_config");
    assert_eq!(doc["files"][0]["retryable"], false);
    assert!(
        !String::from_utf8(out.stdout)
            .unwrap()
            .contains("secret-not-to-export")
    );
    assert!(
        !String::from_utf8(out.stderr)
            .unwrap()
            .contains("secret-not-to-export")
    );
}
#[test]
fn json_provider_failures_keep_http_status_without_response_secrets() {
    for (status, code, retry) in [
        (401, "authentication", false),
        (403, "permission", false),
        (429, "rate_limited", true),
        (503, "server", true),
        (408, "timeout", true),
    ] {
        let (url, _, handle) = server(vec![(status, b"private-body?token=secret".to_vec())]);
        let f = Fixture::new(&url);
        let out = f.run(&[
            "upload",
            f.image.to_str().unwrap(),
            "--format",
            "json",
            "--no-copy",
        ]);
        handle.join().unwrap();
        assert_eq!(out.status.code(), Some(1));
        let doc = parsed(&out);
        assert_eq!(doc["files"][0]["error_code"], code);
        assert_eq!(doc["files"][0]["http_status"], status);
        assert_eq!(doc["files"][0]["retryable"], retry);
        assert!(
            !String::from_utf8(out.stdout)
                .unwrap()
                .contains("private-body")
        );
    }
}
#[test]
fn rust_version_and_config_do_not_require_storage_credentials() {
    let f = Fixture::new("https://unused.test/upload");
    assert!(
        String::from_utf8(f.run(&["version"]).stdout)
            .unwrap()
            .contains("implementation: Rust")
    );
    assert!(f.run(&["config", "validate"]).status.success());
    assert!(
        f.run(&["config", "set", "upload.max_width", "640"])
            .status
            .success()
    );
    assert_eq!(
        String::from_utf8(f.run(&["config", "get", "upload.max_width"]).stdout)
            .unwrap()
            .trim(),
        "640"
    );
    assert!(
        f.run(&["config", "unset", "upload.max_width"])
            .status
            .success()
    );
    assert_eq!(
        String::from_utf8(f.run(&["config", "get", "upload.max_width"]).stdout)
            .unwrap()
            .trim(),
        "0"
    );
    assert!(
        !f.run(&["config", "set", "upload.retry_count", "9999"])
            .status
            .success()
    );
    assert!(f.run(&["completion", "zsh"]).stdout.len() > 1000);
}
#[test]
fn default_upload_json_partial_failure_progress_and_resize() {
    let (url, seen, h) = server(vec![(200, ok_body()), (200, ok_body())]);
    let f = Fixture::new(&url);
    let out = f.run(&[
        f.image.to_str().unwrap(),
        "missing.png",
        "--format",
        "json",
        "--no-copy",
    ]);
    assert_eq!(out.status.code(), Some(3));
    let json = parsed(&out);
    assert_eq!(json["success"], false);
    assert_eq!(json["files"][0]["success"], true);
    assert_eq!(json["files"][1]["success"], false);
    let out = f.run(&[
        "upload",
        f.image.to_str().unwrap(),
        "--resize",
        "30",
        "--format",
        "json",
        "--progress",
        "--no-copy",
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let json = parsed(&out);
    assert_eq!(json["files"][0]["content_type"], "image/jpeg");
    assert!(json["files"][0]["original_size"].as_u64().unwrap() > 0);
    h.join().unwrap();
    let events = String::from_utf8(out.stderr)
        .unwrap()
        .lines()
        .map(|s| serde_json::from_str::<serde_json::Value>(s).unwrap())
        .collect::<Vec<_>>();
    let body = seen.lock().unwrap();
    assert_eq!(
        events.last().unwrap()["sent"].as_u64(),
        Some(body[1].len() as u64)
    );
    assert_eq!(events.last().unwrap()["stage"], "waiting");
}
#[test]
fn fetch_retains_original_and_never_overwrites_destination() {
    let mut png = Cursor::new(vec![]);
    DynamicImage::new_rgb8(5, 4)
        .write_to(&mut png, image::ImageFormat::Png)
        .unwrap();
    let bytes = png.into_inner();
    let (url, _, h) = server(vec![(200, bytes.clone()), (200, bytes.clone())]);
    let f = Fixture::new("https://unused.test");
    let dest = f.dir.path().join("download.png");
    let args = [
        "fetch",
        "--output",
        dest.to_str().unwrap(),
        "--allow-insecure",
        &url,
    ];
    assert!(f.run(&args).status.success());
    assert_eq!(std::fs::read(&dest).unwrap(), bytes);
    assert!(!f.run(&args).status.success());
    assert_eq!(std::fs::read(&dest).unwrap(), bytes);
    h.join().unwrap();
}
#[test]
fn transient_http_failures_retry_and_permanent_ones_do_not() {
    let (url, seen, h) = server(vec![(503, b"temporary".to_vec()), (200, ok_body())]);
    let f = Fixture::new(&url);
    assert!(
        f.run(&["config", "set", "upload.retry_count", "1"])
            .status
            .success()
    );
    let out = f.run(&[
        "upload",
        f.image.to_str().unwrap(),
        "--format",
        "json",
        "--progress",
        "--no-copy",
    ]);
    assert!(out.status.success());
    assert!(String::from_utf8(out.stderr).unwrap().contains("retrying"));
    h.join().unwrap();
    assert_eq!(seen.lock().unwrap().len(), 2);
}
#[test]
fn rewrite_keeps_prose_and_fails_stdin_when_upload_fails() {
    let (url, _, h) = server(vec![(200, ok_body()), (403, b"denied".to_vec())]);
    let f = Fixture::new(&url);
    let md = f.dir.path().join("article.md");
    std::fs::write(
        &md,
        "(picture.png) is text. ![photo](picture.png \"title\")",
    )
    .unwrap();
    assert!(f.run(&["rewrite", md.to_str().unwrap()]).status.success());
    assert_eq!(
        std::fs::read_to_string(&md).unwrap(),
        "(picture.png) is text. ![photo](https://cdn.test/picture.png \"title\")"
    );
    let mut c = f
        .command()
        .args(["rewrite"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    c.stdin
        .take()
        .unwrap()
        .write_all(b"![](picture.png)")
        .unwrap();
    let out = c.wait_with_output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(out.stdout, b"![](picture.png)");
    h.join().unwrap();
}
#[test]
fn installation_produces_a_standalone_rust_cli() {
    let f = Fixture::new("https://unused.test");
    let dir = f.dir.path().join("command line");
    let out = f.run(&["install-cli", "--dir", dir.to_str().unwrap()]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let binary = dir.join(if cfg!(windows) { "img.exe" } else { "img" });
    let version = Command::new(binary).arg("version").output().unwrap();
    assert!(version.status.success());
    assert!(String::from_utf8(version.stdout).unwrap().contains("Rust"));
}
#[test]
fn info_flags_after_file_and_url_fetch_limits() {
    let f = Fixture::new("https://unused.test");
    let out = f.run(&["info", f.image.to_str().unwrap(), "--format", "json"]);
    assert!(out.status.success());
    assert_eq!(parsed(&out)[0]["width"], 100);
    let (url, _, h) = server(vec![(200, vec![1; 500])]);
    let out = f.run(&[
        "fetch",
        "--output",
        f.dir.path().join("no.png").to_str().unwrap(),
        "--max-size",
        "20",
        "--allow-insecure",
        &url,
    ]);
    assert!(!out.status.success());
    h.join().unwrap();
    assert!(!f.dir.path().join("no.png").exists());
}
#[test]
fn project_cannot_supply_credentials() {
    let f = Fixture::new("https://unused.test");
    std::fs::write(
        f.dir.path().join(".img.toml"),
        "[providers.evil]\ntype='http'\nurl='https://evil.test'\n",
    )
    .unwrap();
    assert!(!f.run(&["config", "validate"]).status.success());
}
#[test]
fn server_supports_picgo_json_and_multipart() {
    let (url, _, h) = server(vec![(200, ok_body()), (200, ok_body())]);
    let f = Fixture::new(&url);
    let mut child = f
        .command()
        .args(["serve", "--port", "0"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    use std::io::BufRead;
    let mut line = String::new();
    std::io::BufReader::new(child.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    let endpoint = format!(
        "{}/upload",
        line.trim()
            .strip_prefix("img serve listening on http://")
            .map(|s| format!("http://{}", s.trim_end_matches("/upload")))
            .unwrap()
    );
    let client = reqwest::blocking::Client::new();
    let json: serde_json::Value = client
        .post(&endpoint)
        .json(&serde_json::json!({"list":[f.image]}))
        .send()
        .unwrap()
        .json()
        .unwrap();
    assert_eq!(json["success"], true);
    let multipart = reqwest::blocking::multipart::Form::new()
        .file("file", &f.image)
        .unwrap();
    let json: serde_json::Value = client
        .post(&endpoint)
        .multipart(multipart)
        .send()
        .unwrap()
        .json()
        .unwrap();
    assert_eq!(json["success"], true);
    assert_eq!(
        client
            .post(&endpoint)
            .header("Origin", "https://evil.test")
            .json(&serde_json::json!({"list":[f.image]}))
            .send()
            .unwrap()
            .status()
            .as_u16(),
        403
    );
    child.kill().unwrap();
    child.wait().unwrap();
    h.join().unwrap();
}
