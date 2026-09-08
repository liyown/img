use super::*;
use base64::Engine;
use std::{collections::BTreeMap, io::Write, time::Instant};
#[path = "sync_form.rs"]
mod form;

pub(super) struct ConfigurationChanged;
impl EventEmitter<ConfigurationChanged> for SyncPanel {}
pub(super) struct SyncPanel {
    root: PathBuf,
    engine: PathBuf,
    fields: BTreeMap<&'static str, Entity<InputState>>,
    s3: bool,
    editing: bool,
    configured: bool,
    paused: bool,
    busy: bool,
    polling: bool,
    stopped: bool,
    notice: Option<String>,
    notice_error: bool,
    errors: form::Errors,
    operation: String,
    clear_secrets: bool,
    endpoint: String,
    prefix: String,
    path_style: bool,
    _subscriptions: Vec<Subscription>,
    control: Option<Control>,
    stamp: String,
    config_stamp: String,
    dirty: Option<Instant>,
    next: Instant,
    next_index: Instant,
    failures: u32,
    conflicts: Vec<serde_json::Value>,
}
impl SyncPanel {
    pub fn new(
        root: PathBuf,
        engine: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let settings = std::fs::read(root.join("sync.json"))
            .ok()
            .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok());
        let configured = settings.is_some();
        let mut fields = BTreeMap::new();
        for (key, title, secret) in [
            ("endpoint", "HTTPS 服务地址", false),
            ("prefix", "同步目录", false),
            ("username", "用户名", false),
            ("password", "应用密码", true),
            ("bucket", "存储桶", false),
            ("region", "区域", false),
            ("access_key", "Access Key", true),
            ("secret_key", "Secret Key", true),
        ] {
            let value = settings
                .as_ref()
                .and_then(|s| s[key].as_str())
                .unwrap_or(if key == "prefix" { ".img-sync/" } else { "" });
            fields.insert(
                key,
                cx.new(|cx| {
                    InputState::new(window, cx)
                        .placeholder(crate::i18n::text(title))
                        .masked(secret)
                        .default_value(value)
                }),
            );
        }
        let subscriptions = fields
            .iter()
            .map(|(&key, field)| {
                cx.subscribe(field, move |this, _, event: &InputEvent, cx| {
                    if matches!(event, InputEvent::Change) {
                        if this.errors.contains_key(key) {
                            let errors = form::validate(this.s3, &this.values(cx));
                            if let Some(error) = errors.get(key) {
                                this.errors.insert(key, error);
                            } else {
                                this.errors.remove(key);
                            }
                        }
                        if this.notice_error {
                            this.notice = None;
                            this.notice_error = false;
                        }
                        cx.notify();
                    }
                })
            })
            .collect();
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                if this.update(cx, |this, cx| this.tick(cx)).is_err() {
                    break;
                }
            }
        })
        .detach();
        Self {
            root,
            engine,
            fields,
            s3: settings.as_ref().is_some_and(|s| s["kind"] == "s3"),
            editing: !configured,
            configured,
            paused: settings.as_ref().is_some_and(|s| s["paused"] == true),
            busy: false,
            polling: false,
            stopped: false,
            notice: None,
            notice_error: false,
            errors: Default::default(),
            operation: String::new(),
            clear_secrets: false,
            endpoint: settings
                .as_ref()
                .and_then(|s| s["endpoint"].as_str())
                .unwrap_or("")
                .into(),
            prefix: settings
                .as_ref()
                .and_then(|s| s["prefix"].as_str())
                .unwrap_or("")
                .into(),
            path_style: false,
            _subscriptions: subscriptions,
            control: None,
            stamp: String::new(),
            config_stamp: String::new(),
            dirty: None,
            next: Instant::now(),
            next_index: Instant::now(),
            failures: 0,
            conflicts: vec![],
        }
    }
    pub fn stop(&mut self) -> Option<Control> {
        self.stopped = true;
        if let Some(c) = &self.control {
            c.stop(engine::CANCEL);
        }
        self.control.take()
    }
    fn tick(&mut self, cx: &mut Context<Self>) {
        if self.stopped || self.busy || self.polling {
            return;
        }
        self.polling = true;
        let root = self.root.clone();
        let task = cx.background_executor().spawn(async move {
            let settings = std::fs::read(root.join("sync.json"))
                .ok()
                .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok());
            let catalog = img_records::catalog::Catalog::open(&root).ok();
            let revision = catalog.as_ref().and_then(|c| c.sync_revision().ok());
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            let due_scope = catalog
                .as_ref()
                .and_then(|c| c.settings_prefix("scope:").ok())
                .unwrap_or_default()
                .into_iter()
                .filter_map(|(_, value)| serde_json::from_str::<serde_json::Value>(&value).ok())
                .find(|scope| {
                    let last = catalog
                        .as_ref()
                        .and_then(|c| {
                            c.setting(&format!("scan-time:{}", scope["id"].as_str().unwrap_or("")))
                                .ok()
                        })
                        .flatten()
                        .and_then(|s| s.parse::<u64>().ok())
                        .unwrap_or(0);
                    scope["enabled"] == true && now.saturating_sub(last) >= 900
                });
            let config_stamp = storage::config_path()
                .ok()
                .and_then(|p| std::fs::metadata(p).ok())
                .map(|m| format!("{:?}:{}", m.modified(), m.len()))
                .unwrap_or_default();
            (settings, revision, config_stamp, due_scope)
        });
        cx.spawn(async move |this, cx| {
            let (settings, revision, config_stamp, due_scope) = task.await;
            let _ = this.update(cx, |this, cx| {
                this.polling = false;
                if this.config_stamp != config_stamp {
                    this.config_stamp = config_stamp.clone();
                    cx.emit(ConfigurationChanged);
                }
                this.configured = settings.is_some();
                this.paused = settings.as_ref().is_some_and(|s| s["paused"] == true);
                if !this.configured {
                    this.editing = true;
                }
                if !this.editing
                    && let Some(settings) = &settings
                {
                    this.endpoint = settings["endpoint"].as_str().unwrap_or("").into();
                    this.prefix = settings["prefix"].as_str().unwrap_or("").into();
                    this.s3 = settings["kind"] == "s3";
                }
                if this.stopped || this.busy {
                    return;
                }
                let Some(revision) = revision else {
                    return;
                };
                let stamp = format!("{revision}:{config_stamp}");
                if stamp != this.stamp {
                    this.stamp = stamp;
                    this.dirty = Some(Instant::now());
                }
                let now = Instant::now();
                if this.configured
                    && !this.editing
                    && !this.paused
                    && (now >= this.next
                        || (this.failures == 0
                            && this
                                .dirty
                                .is_some_and(|t| now.duration_since(t) >= Duration::from_secs(5))))
                {
                    this.dirty = None;
                    this.run(vec!["run".into()], None, cx);
                } else if now >= this.next_index
                    && let Some(scope) = due_scope
                {
                    this.run_group(
                        "library",
                        vec![
                            "index".into(),
                            "--provider".into(),
                            scope["provider"].as_str().unwrap_or("").into(),
                            "--prefix".into(),
                            scope["prefix"].as_str().unwrap_or("").into(),
                            "--scope-id".into(),
                            scope["id"].as_str().unwrap_or("").into(),
                            "--resume".into(),
                        ],
                        None,
                        cx,
                    );
                }
                cx.notify();
            });
        })
        .detach();
    }
    fn run(
        &mut self,
        args: Vec<String>,
        file: Option<tempfile::NamedTempFile>,
        cx: &mut Context<Self>,
    ) {
        self.run_group("sync", args, file, cx);
    }
    fn run_group(
        &mut self,
        group: &'static str,
        args: Vec<String>,
        file: Option<tempfile::NamedTempFile>,
        cx: &mut Context<Self>,
    ) {
        if self.busy || self.stopped {
            return;
        }
        self.busy = true;
        let operation = if group == "library" {
            "index".into()
        } else {
            args.first().cloned().unwrap_or_default()
        };
        self.operation = operation.clone();
        if group == "sync" {
            self.notice = None;
            self.notice_error = false;
        }
        let root = self.root.clone();
        let engine = self.engine.clone();
        let control = Control::default();
        self.control = Some(control.clone());
        let task = cx.background_executor().spawn(async move {
            let _file = file;
            let _completion = control.completion();
            (|| -> anyhow::Result<_> {
                let mut command = std::process::Command::new(engine);
                command
                    .arg("--config")
                    .arg(storage::config_path()?)
                    .arg(group)
                    .args(args)
                    .env("IMG_DATA_DIR", root);
                let result = engine::run(command, &control)?;
                if result.stopped != 0 {
                    return Ok((false, serde_json::json!({"code":"cancelled"})));
                }
                let value: serde_json::Value = serde_json::from_slice(&result.stdout)?;
                Ok((result.success, value))
            })()
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.busy = false;
                this.control = None;
                let mut retry = 0;
                match result {
                    Ok((true, value)) => {
                        this.failures = 0;
                        this.notice_error = false;
                        if let Some(rows) = value.as_array() {
                            this.conflicts = rows.clone();
                            this.notice = Some(format!("{} 项冲突需要处理", rows.len()));
                        } else if value.get("pushed").is_some() {
                            this.notice = Some(format!(
                                "已发送 {} 项变更，接收 {} 项变更；{} 项冲突",
                                value["pushed"], value["pulled"], value["conflicts"]
                            ));
                        } else if value.get("complete").is_some() {
                            // Gallery indexing has its own status; it is not a sync connection result.
                        } else if operation == "configure" {
                            this.notice = Some("连接成功，已保存同步设置。".into());
                            this.configured = true;
                            this.paused = false;
                            this.endpoint = value["endpoint"].as_str().unwrap_or("").into();
                            this.prefix = value["prefix"].as_str().unwrap_or("").into();
                            this.editing = false;
                            this.clear_secrets = true;
                        } else if operation == "pause" || operation == "resume" {
                            this.paused = operation == "pause";
                            this.notice = Some(
                                if this.paused {
                                    "已暂停自动同步"
                                } else {
                                    "已恢复自动同步"
                                }
                                .into(),
                            );
                        } else if operation == "resolve" {
                            this.notice = Some("冲突已处理".into());
                            if let Some(entity) = value["entity"].as_str() {
                                this.conflicts.retain(|row| {
                                    row["entity"] != entity || row["field"] != value["field"]
                                });
                            }
                        }
                    }
                    Ok((false, value)) => {
                        this.failures = this.failures.saturating_add(1);
                        retry = value["retry_after_seconds"].as_u64().unwrap_or(0);
                        if group == "sync" {
                            this.notice_error = value["code"] != "cancelled";
                            if !this.notice_error {
                                this.failures = 0;
                            }
                            this.notice = Some(
                                form::failure_message(
                                    value["detail_code"]
                                        .as_str()
                                        .or_else(|| value["code"].as_str())
                                        .unwrap_or(""),
                                    this.s3,
                                )
                                .into(),
                            );
                        }
                    }
                    Err(_) => {
                        this.failures = this.failures.saturating_add(1);
                        if group == "sync" {
                            this.notice_error = true;
                            this.notice = Some(form::failure_message("", this.s3).into());
                        }
                    }
                }
                let next = Instant::now()
                    .checked_add(Duration::from_secs(if this.failures == 0 {
                        60
                    } else {
                        (5u64.saturating_mul(1u64 << this.failures.min(6))).max(retry)
                    }))
                    .unwrap_or_else(|| Instant::now() + Duration::from_secs(315360000));
                if group == "library" {
                    this.next_index = if this.failures == 0 {
                        Instant::now() + Duration::from_secs(1)
                    } else {
                        next
                    };
                } else {
                    this.next = next;
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
    fn configure(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let values = self.values(cx);
        self.errors = form::validate(self.s3, &values);
        self.notice = None;
        self.notice_error = false;
        if !self.errors.is_empty() {
            if let Some(key) = form::keys(self.s3)
                .iter()
                .find(|key| self.errors.contains_key(**key))
            {
                self.fields[key].read(cx).focus_handle(cx).focus(window, cx);
            }
            cx.notify();
            return;
        }
        let get = |key: &str| values[key].clone();
        let mut cfg = toml::map::Map::new();
        cfg.insert(
            "type".into(),
            toml::Value::String(if self.s3 { "s3" } else { "webdav" }.into()),
        );
        cfg.insert(
            "endpoint".into(),
            toml::Value::String(get("endpoint").trim().into()),
        );
        if self.s3 {
            for key in ["bucket", "region", "access_key", "secret_key"] {
                cfg.insert(key.into(), toml::Value::String(get(key)));
            }
            cfg.insert("path_style".into(), toml::Value::Boolean(self.path_style));
        } else {
            let authorization = format!(
                "Basic {}",
                base64::engine::general_purpose::STANDARD.encode(format!(
                    "{}:{}",
                    get("username"),
                    get("password")
                ))
            );
            cfg.insert(
                "headers".into(),
                toml::Value::Table(toml::map::Map::from_iter([(
                    "Authorization".into(),
                    toml::Value::String(authorization),
                )])),
            );
        }
        let prefix = format!("{}/", get("prefix").trim().trim_end_matches('/'));
        let result = (|| -> anyhow::Result<_> {
            let mut file = tempfile::NamedTempFile::new()?;
            file.write_all(toml::to_string(&cfg)?.as_bytes())?;
            file.as_file().sync_all()?;
            Ok(file)
        })();
        match result {
            Ok(file) => {
                let args = vec![
                    "configure".into(),
                    "--file".into(),
                    file.path().to_string_lossy().into_owned(),
                    "--prefix".into(),
                    prefix,
                ];
                self.run(args, Some(file), cx);
            }
            Err(_) => {
                self.notice_error = true;
                self.notice = Some("无法保存临时配置，请检查本机磁盘空间和写入权限。".into());
            }
        }
    }
    fn values(&self, cx: &App) -> form::Values {
        self.fields
            .iter()
            .map(|(&key, field)| (key, field.read(cx).value().to_string()))
            .collect()
    }
    fn edit_connection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        self.busy = true;
        self.operation = "load".into();
        self.notice = None;
        self.errors.clear();
        let root = self.root.clone();
        let task = cx.background_executor().spawn(async move {
            (|| -> anyhow::Result<(serde_json::Value, serde_json::Value)> {
                let settings: serde_json::Value =
                    serde_json::from_slice(&std::fs::read(root.join("sync.json"))?)?;
                let key = settings["credential_ref"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("missing reference"))?;
                anyhow::ensure!(
                    key.starts_with("IMG_SYNC_CONNECTION_")
                        && key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_'),
                    "invalid reference"
                );
                let connection = serde_json::from_slice(&img_records::credentials::get(key)?)?;
                Ok((settings, connection))
            })()
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.busy = false;
                this.editing = true;
                match result {
                    Ok((settings, connection)) => {
                        this.s3 = settings["kind"] == "s3";
                        this.path_style = connection["path_style"].as_bool().unwrap_or(false);
                        let basic = connection["headers"]
                            .as_object()
                            .and_then(|headers| {
                                headers
                                    .iter()
                                    .find(|(k, _)| k.eq_ignore_ascii_case("authorization"))
                            })
                            .and_then(|(_, value)| value.as_str())
                            .and_then(|value| value.strip_prefix("Basic "))
                            .and_then(|value| {
                                base64::engine::general_purpose::STANDARD.decode(value).ok()
                            })
                            .and_then(|bytes| String::from_utf8(bytes).ok())
                            .unwrap_or_default();
                        let (username, password) = basic.split_once(':').unwrap_or(("", ""));
                        for (&key, field) in &this.fields {
                            let value = match key {
                                "prefix" => settings["prefix"].as_str().unwrap_or(""),
                                "username" => username,
                                "password" => password,
                                _ => connection[key].as_str().unwrap_or(""),
                            };
                            field.update(cx, |field, cx| field.set_value(value, window, cx));
                        }
                    }
                    Err(_) => {
                        this.notice_error = true;
                        this.notice =
                            Some(form::failure_message("credentials_missing", this.s3).into());
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
    fn field(
        &self,
        key: &'static str,
        title: &'static str,
        helper: Option<&'static str>,
    ) -> AnyElement {
        let error = self.errors.get(key);
        div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .gap(px(5.))
            .child(label(title, 12., TEXT))
            .child(
                Input::new(&self.fields[key])
                    .aria_label(crate::i18n::text(match error {
                        Some(error) => format!("{title}：{error}"),
                        None => title.into(),
                    }))
                    .disabled(self.busy)
                    .when(error.is_some(), |input| {
                        input.border_color(crate::theme::color(RED))
                    }),
            )
            .when_some(error, |field, error| field.child(label(*error, 11., RED)))
            .when(error.is_none(), |field| {
                field.children(helper.map(|hint| label(hint, 11., MUTED)))
            })
            .into_any_element()
    }
}
impl Render for SyncPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.clear_secrets {
            self.clear_secrets = false;
            for key in ["password", "access_key", "secret_key"] {
                self.fields[key].update(cx, |field, cx| field.set_value("", window, cx));
            }
        }
        let mut body = div()
            .flex()
            .flex_col()
            .gap(px(14.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(label("跨设备同步", 16., TEXT).font_weight(FontWeight::SEMIBOLD))
                    .child(label(
                        if !self.configured {
                            "未连接"
                        } else if self.paused {
                            "已暂停"
                        } else {
                            "已连接"
                        },
                        11.,
                        MUTED,
                    )),
            )
            .child(label(
                "在设备之间同步图床配置、图库索引和预设。图片缓存留在本机。",
                12.,
                MUTED,
            ));
        if self.editing {
            body = body.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .child(
                        action("sync-dav", "WebDAV")
                            .ghost()
                            .h(px(32.))
                            .selected(!self.s3)
                            .disabled(self.busy)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.s3 = false;
                                this.errors.clear();
                                this.notice = None;
                                cx.notify();
                            })),
                    )
                    .child(
                        action("sync-s3", "S3 兼容存储")
                            .ghost()
                            .h(px(32.))
                            .selected(self.s3)
                            .disabled(self.busy)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.s3 = true;
                                this.errors.clear();
                                this.notice = None;
                                cx.notify();
                            })),
                    ),
            );
            body = body.child(self.field(
                "endpoint",
                "服务地址",
                Some(if self.s3 {
                    "填写 S3 服务地址，例如 https://s3.us-east-1.amazonaws.com"
                } else {
                    "填写服务商提供的 WebDAV 地址，例如 https://dav.jianguoyun.com/dav/"
                }),
            ));
            if self.s3 {
                body = body
                    .child(
                        div()
                            .flex()
                            .gap(px(12.))
                            .child(self.field("bucket", "存储桶", None))
                            .child(self.field("region", "区域", None)),
                    )
                    .child(
                        div()
                            .flex()
                            .gap(px(12.))
                            .child(self.field("access_key", "Access Key ID", None))
                            .child(self.field("secret_key", "Secret Access Key", None)),
                    );
            } else {
                body = body.child(
                    div()
                        .flex()
                        .gap(px(12.))
                        .child(self.field("username", "用户名", None))
                        .child(self.field(
                            "password",
                            "应用密码",
                            Some("坚果云请使用第三方应用密码"),
                        )),
                );
            }
            body = body.child(self.field(
                "prefix",
                "同步目录",
                Some("使用专用目录，例如 .img-sync/，不要与图片目录混用。"),
            ));
            if self.s3 {
                body = body.child(
                    gpui_kit::component::checkbox::Checkbox::new("sync-path-style")
                        .label(crate::i18n::text("使用路径形式访问存储桶"))
                        .checked(self.path_style)
                        .disabled(self.busy)
                        .on_click(cx.listener(|this, checked, _, cx| {
                            this.path_style = *checked;
                            cx.notify();
                        })),
                );
            }
            body = body.child(label(
                "同步目录会保存未加密的图床密钥。连接此服务的密码仅保存在本机。",
                11.,
                MUTED,
            ));
            if let Some(notice) = &self.notice {
                body = body.child(label(
                    notice.clone(),
                    12.,
                    if self.notice_error { RED } else { MUTED },
                ));
            }
            let mut actions = div().flex().items_center().gap(px(8.)).child(
                action(
                    "sync-configure",
                    if self.busy && self.operation == "configure" {
                        "正在验证连接…"
                    } else {
                        "验证并连接"
                    },
                )
                .primary()
                .disabled(self.busy)
                .loading(self.busy && self.operation == "configure")
                .on_click(cx.listener(|this, _, window, cx| this.configure(window, cx))),
            );
            if self.configured || (self.busy && self.operation == "configure") {
                actions = actions.child(
                    action("sync-cancel-edit", "取消")
                        .ghost()
                        .disabled(self.busy && self.operation != "configure")
                        .on_click(cx.listener(|this, _, _, cx| {
                            if this.busy {
                                if let Some(control) = &this.control {
                                    control.stop(engine::CANCEL);
                                }
                                return;
                            }
                            this.editing = false;
                            this.errors.clear();
                            this.notice = None;
                            this.clear_secrets = true;
                            cx.notify();
                        })),
                );
            }
            body = body.child(actions);
        } else {
            body = body.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(5.))
                    .child(
                        label(
                            format!(
                                "{} · {}",
                                if self.s3 { "S3" } else { "WebDAV" },
                                self.endpoint
                            ),
                            12.,
                            TEXT,
                        )
                        .text_ellipsis(),
                    )
                    .child(label(format!("同步目录：{}", self.prefix), 12., MUTED)),
            );
            let weak = cx.weak_entity();
            let busy = self.busy;
            let paused = self.paused;
            body = body.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        action(
                            "sync-now",
                            if self.busy && self.operation == "run" {
                                "正在同步…"
                            } else if self.paused {
                                "恢复同步"
                            } else {
                                "立即同步"
                            },
                        )
                        .primary()
                        .disabled(self.busy)
                        .loading(self.busy && self.operation == "run")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.run(
                                vec![if this.paused { "resume" } else { "run" }.into()],
                                None,
                                cx,
                            )
                        })),
                    )
                    .child(
                        action(
                            "sync-edit",
                            if self.busy && self.operation == "load" {
                                "正在读取连接…"
                            } else {
                                "编辑连接"
                            },
                        )
                        .ghost()
                        .disabled(self.busy)
                        .on_click(
                            cx.listener(|this, _, window, cx| this.edit_connection(window, cx)),
                        ),
                    )
                    .child(
                        Button::new("sync-more")
                            .ghost()
                            .icon(IconName::Ellipsis)
                            .w(px(32.))
                            .h(px(32.))
                            .accessibility_label(crate::i18n::text("更多同步操作"))
                            .disabled(self.busy)
                            .dropdown_menu(move |menu, _, _| {
                                let pause_view = weak.clone();
                                let conflict_view = weak.clone();
                                menu.item(
                                    PopupMenuItem::new(crate::i18n::text(if paused {
                                        "恢复同步"
                                    } else {
                                        "暂停同步"
                                    }))
                                    .disabled(busy)
                                    .on_click(
                                        move |_, _, cx| {
                                            let _ = pause_view.update(cx, |this, cx| {
                                                this.run(
                                                    vec![
                                                        if paused { "resume" } else { "pause" }
                                                            .into(),
                                                    ],
                                                    None,
                                                    cx,
                                                )
                                            });
                                        },
                                    ),
                                )
                                .item(
                                    PopupMenuItem::new(crate::i18n::text("查看冲突"))
                                        .disabled(busy)
                                        .on_click(move |_, _, cx| {
                                            let _ = conflict_view.update(cx, |this, cx| {
                                                this.run(vec!["conflicts".into()], None, cx)
                                            });
                                        }),
                                )
                            }),
                    ),
            );
            if let Some(notice) = &self.notice {
                body = body.child(label(
                    notice.clone(),
                    12.,
                    if self.notice_error { RED } else { MUTED },
                ));
            }
        }
        if !self.conflicts.is_empty() {
            body = body.child(label(
                "这些设置在不同设备上同时被修改，请选择要保留的内容。",
                12.,
                MUTED,
            ));
        }
        for (index, conflict) in self.conflicts.clone().into_iter().enumerate() {
            let entity = conflict["entity"].as_str().unwrap_or("");
            let field = conflict["field"].as_str().unwrap_or("");
            let title = match entity.split_once(':') {
                Some(("provider", name)) => format!("存储源 {name}"),
                Some(("asset", _)) => "图片设置".into(),
                Some(("location", _)) => "远端地址".into(),
                Some(("preset", _)) => "处理预设".into(),
                _ => "同步设置".into(),
            };
            let field_label = match field {
                "endpoint" => "服务地址",
                "hidden" => "隐藏记录",
                "preferred_location" => "首选链接",
                "name" => "名称",
                "public_url" => "公开访问地址",
                "bucket" => "存储桶",
                "region" => "区域",
                "access_key" | "secret_key" | "session_token" | "token" | "headers" | "fields" => {
                    "访问凭据"
                }
                _ => field,
            };
            let mut choices = div().flex().gap(px(10.));
            if let Some(candidates) = conflict["candidates"].as_array() {
                for (n, event) in candidates.iter().enumerate() {
                    let args = vec![
                        "resolve".into(),
                        entity.into(),
                        field.into(),
                        event["id"].as_str().unwrap_or("").into(),
                    ];
                    let value = if [
                        "access_key",
                        "secret_key",
                        "session_token",
                        "token",
                        "headers",
                        "fields",
                    ]
                    .contains(&field)
                    {
                        "已保存的访问凭据".into()
                    } else if event["value"].is_null() {
                        "已删除".into()
                    } else if let Some(text) = event["value"].as_str() {
                        text.to_owned()
                    } else {
                        event["value"].to_string()
                    };
                    choices = choices.child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(8.))
                            .p(px(12.))
                            .border_1()
                            .border_color(crate::theme::color(BORDER))
                            .rounded(px(8.))
                            .child(label(
                                format!(
                                    "设备 {}",
                                    event["device"]
                                        .as_str()
                                        .unwrap_or("")
                                        .chars()
                                        .take(8)
                                        .collect::<String>()
                                ),
                                11.,
                                MUTED,
                            ))
                            .child(
                                div()
                                    .id(SharedString::from(format!("conflict-value-{index}-{n}")))
                                    .max_h(px(100.))
                                    .overflow_y_scroll()
                                    .child(label(value, 12., TEXT)),
                            )
                            .child(
                                action(
                                    SharedString::from(format!("resolve-{index}-{n}")),
                                    "保留此版本",
                                )
                                .disabled(self.busy)
                                .on_click(cx.listener(
                                    move |this, _, _, cx| this.run(args.clone(), None, cx),
                                )),
                            ),
                    );
                }
            }
            body = body.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(label(format!("{title} · {field_label}"), 13., TEXT))
                    .child(choices),
            );
        }
        body
    }
}
