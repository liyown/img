use image::DynamicImage;
use std::{
    io::{Cursor, Write},
    path::PathBuf,
    process::{Command, Output, Stdio},
    sync::{Arc, Mutex},
    time::Duration,
};
const BIN: &str = env!("CARGO_BIN_EXE_img");
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
