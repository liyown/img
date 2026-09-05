use anyhow::{Context, Result, ensure};
use img_core::{
    config::Upload,
    control::Control,
    provider::Provider,
    upload::{self, Options},
};
use serde::Deserialize;
use std::{
    io::{Read, Write},
    sync::Arc,
    time::Duration,
};
use tiny_http::{Header, Method, Response, Server, StatusCode};
#[derive(Deserialize)]
struct Input {
    list: Vec<String>,
}
const MAX_BODY: u64 = 128 << 20;
fn json_response(value: serde_json::Value, status: u16) -> Response<std::io::Cursor<Vec<u8>>> {
    Response::from_data(serde_json::to_vec(&value).unwrap())
        .with_status_code(StatusCode(status))
        .with_header(Header::from_bytes("Content-Type", "application/json").unwrap())
}
fn handle(
    request: &mut tiny_http::Request,
    p: &Provider,
    c: &Upload,
    o: &Options,
    control: &Control,
) -> Result<serde_json::Value> {
    let ct = request
        .headers()
        .iter()
        .find(|h| h.field.equiv("Content-Type"))
        .map(|h| h.value.as_str().to_owned())
        .unwrap_or_default();
    ensure!(
        request.body_length().is_none_or(|n| n as u64 <= MAX_BODY),
        "request body exceeds 128 MiB"
    );
    let mut bytes = vec![];
    request
        .as_reader()
        .take(MAX_BODY + 1)
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= MAX_BODY,
        "request body exceeds 128 MiB"
    );
    let directory = tempfile::tempdir()?;
    let files = if ct.starts_with("application/json") {
        serde_json::from_slice::<Input>(&bytes)
            .context("invalid JSON body")?
            .list
    } else {
        let boundary =
            multer::parse_boundary(&ct).context("expected JSON or multipart form data")?;
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        rt.block_on(async {
            let stream = futures_util::stream::once(async { Ok::<_, std::io::Error>(bytes) });
            let mut multipart = multer::Multipart::new(stream, boundary);
            let mut files = vec![];
            while let Some(field) = multipart.next_field().await? {
                if field.file_name().is_none() {
                    continue;
                }
                ensure!(files.len() < 50, "at most 50 files per request");
                let name = field
                    .file_name()
                    .unwrap()
                    .replace('\\', "/")
                    .rsplit('/')
                    .next()
                    .unwrap_or("image")
                    .to_string();
                let mut file = tempfile::Builder::new()
                    .prefix("img-")
                    .suffix(&format!("-{}", name.replace(['\r', '\n'], "_")))
                    .tempfile_in(directory.path())?;
                let data = field.bytes().await?;
                ensure!(
                    data.len() as u64 <= c.max_size,
                    "image exceeds maximum size"
                );
                file.write_all(&data)?;
                let (_, path) = file.keep().map_err(|e| e.error)?;
                files.push(path.to_string_lossy().into_owned());
            }
            Ok::<_, anyhow::Error>(files)
        })?
    };
    ensure!(
        !files.is_empty() && files.len() <= 50,
        "provide 1–50 images"
    );
    let results = upload::run(p, c, &files, o, control);
    let urls = results
        .iter()
        .filter(|r| r.success)
        .map(|r| r.url.clone())
        .collect::<Vec<_>>();
    if urls.is_empty() {
        Ok(
            serde_json::json!({"success":false,"msg":results.iter().map(|r|r.error.as_str()).collect::<Vec<_>>().join("; ")}),
        )
    } else {
        Ok(serde_json::json!({"success":true,"result":urls}))
    }
}
pub fn run(
    p: Provider,
    c: Upload,
    o: Options,
    bind: &str,
    port: u16,
    control: Control,
) -> Result<()> {
    let addr = format!("{bind}:{port}");
    let server = Arc::new(Server::http(&addr).map_err(|e| anyhow::anyhow!("cannot listen: {e}"))?);
    println!(
        "img serve listening on http://{}/upload",
        server.server_addr()
    );
    std::io::stdout().flush()?;
    std::thread::scope(|scope| {
        for _ in 0..4 {
            let server = &server;
            let p = &p;
            let c = &c;
            let o = &o;
            let control = &control;
            scope.spawn(move||{while !control.is_cancelled(){let mut request=match server.recv_timeout(Duration::from_millis(100)){Ok(Some(r))=>r,Ok(None)=>continue,Err(_)=>break};
            let (response,status)=if request.url()!="/upload"{(serde_json::json!({"success":false,"msg":"use POST /upload"}),404)}
            else if request.method()!=&Method::Post{(serde_json::json!({"success":false,"msg":"method not allowed"}),405)}
            else if request.headers().iter().any(|h|h.field.equiv("Origin")){(serde_json::json!({"success":false,"msg":"browser origins are not accepted by the editor upload service"}),403)}
            else {match handle(&mut request,p,c,o,control){Ok(v)=>(v,200),Err(e)=>(serde_json::json!({"success":false,"msg":e.to_string()}),400)}};
            let _=request.respond(json_response(response,status));
        }});
        }
    });
    Ok(())
}
