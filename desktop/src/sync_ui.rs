use super::*;
use base64::Engine;
use std::{collections::BTreeMap, io::Write, time::Instant};

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
                anyhow::ensure!(result.stopped == 0, "同步已取消");
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
                        if let Some(rows) = value.as_array() {
                            this.conflicts = rows.clone();
                            this.notice = Some(format!("{} 项冲突需要处理", rows.len()));
                        } else if value.get("pushed").is_some() {
                            this.notice = Some(format!(
                                "已发送 {} 项变更，接收 {} 项变更；{} 项冲突",
                                value["pushed"], value["pulled"], value["conflicts"]
                            ));
                        } else if value.get("complete").is_some() {
                            this.notice = Some(format!(
                                "索引已更新：{} 张图片",
                                value["seen"].as_array().map(|a| a.len()).unwrap_or(0)
                            ));
                        } else {
                            this.notice = Some("同步设置已保存".into());
                            this.editing = false;
                        }
                    }
                    Ok((false, value)) => {
                        this.failures = this.failures.saturating_add(1);
                        retry = value["retry_after_seconds"].as_u64().unwrap_or(0);
                        this.notice = Some(format!(
                            "同步未完成：{}",
                            value["error"].as_str().unwrap_or("请检查连接后重试")
                        ));
                    }
                    Err(_) => {
                        this.failures = this.failures.saturating_add(1);
                        this.notice = Some("同步未完成，请检查连接后重试".into());
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
        let get = |key| self.fields[key].read(cx).value().to_string();
        let mut cfg = toml::map::Map::new();
        cfg.insert(
            "type".into(),
            toml::Value::String(if self.s3 { "s3" } else { "webdav" }.into()),
        );
        cfg.insert("endpoint".into(), toml::Value::String(get("endpoint")));
        if self.s3 {
            for key in ["bucket", "region", "access_key", "secret_key"] {
                cfg.insert(key.into(), toml::Value::String(get(key)));
            }
            cfg.insert("path_style".into(), toml::Value::Boolean(true));
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
        let prefix = get("prefix");
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
                for key in ["password", "access_key", "secret_key"] {
                    self.fields[key].update(cx, |field, cx| field.set_value("", window, cx));
                }
            }
            Err(_) => self.notice = Some("无法准备同步配置".into()),
        }
    }
}
impl Render for SyncPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut body = div()
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(label("跨设备同步", 16., TEXT).font_weight(FontWeight::SEMIBOLD))
            .child(label(
                "通过自己的 WebDAV 或 S3 同步配置和图库索引。图片缓存和同步连接密码留在本机。",
                12.,
                MUTED,
            ))
            .child(label(
                "同步目录包含未加密的图床密钥，请仅授权可信设备访问。",
                12.,
                MUTED,
            ));
        if self.editing {
            body = body.child(
                div()
                    .flex()
                    .gap(px(8.))
                    .child(
                        action("sync-dav", "WebDAV").on_click(cx.listener(|this, _, _, cx| {
                            this.s3 = false;
                            cx.notify();
                        })),
                    )
                    .child(action("sync-s3", "S3 兼容存储").on_click(cx.listener(
                        |this, _, _, cx| {
                            this.s3 = true;
                            cx.notify();
                        },
                    ))),
            );
            let keys = if self.s3 {
                vec![
                    "endpoint",
                    "prefix",
                    "bucket",
                    "region",
                    "access_key",
                    "secret_key",
                ]
            } else {
                vec!["endpoint", "prefix", "username", "password"]
            };
            for pair in keys.chunks(2) {
                let mut row = div().flex().gap(px(12.));
                for key in pair {
                    row = row.child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(Input::new(&self.fields[key])),
                    );
                }
                body = body.child(row);
            }
            body = body.child(
                action("sync-configure", "连接并保存")
                    .disabled(self.busy)
                    .on_click(cx.listener(|this, _, window, cx| this.configure(window, cx))),
            );
        }
        body = body.child(
            div()
                .flex()
                .flex_wrap()
                .gap(px(8.))
                .child(
                    action("sync-now", "立即同步")
                        .disabled(!self.configured || self.paused || self.busy)
                        .on_click(
                            cx.listener(|this, _, _, cx| this.run(vec!["run".into()], None, cx)),
                        ),
                )
                .child(
                    action(
                        "sync-pause",
                        if self.paused {
                            "恢复同步"
                        } else {
                            "暂停同步"
                        },
                    )
                    .disabled(!self.configured || self.busy)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.run(
                            vec![if this.paused { "resume" } else { "pause" }.into()],
                            None,
                            cx,
                        )
                    })),
                )
                .child(
                    action("sync-conflicts", "查看冲突")
                        .disabled(self.busy)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.run(vec!["conflicts".into()], None, cx)
                        })),
                )
                .child(
                    action("sync-edit", "编辑连接")
                        .disabled(self.busy)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.editing = !this.editing;
                            cx.notify();
                        })),
                ),
        );
        if self.busy {
            body = body.child(label("正在同步…", 12., MUTED));
        }
        if let Some(notice) = &self.notice {
            body = body.child(label(notice.clone(), 12., MUTED));
        }
        for (index, conflict) in self.conflicts.clone().into_iter().enumerate() {
            let mut row = div().flex().flex_col().gap(px(6.)).child(label(
                format!(
                    "{} · {}",
                    conflict["entity"].as_str().unwrap_or(""),
                    conflict["field"].as_str().unwrap_or("")
                ),
                12.,
                TEXT,
            ));
            if let Some(candidates) = conflict["candidates"].as_array() {
                for (n, event) in candidates.iter().enumerate() {
                    let args = vec![
                        "resolve".into(),
                        conflict["entity"].as_str().unwrap_or("").into(),
                        conflict["field"].as_str().unwrap_or("").into(),
                        event["id"].as_str().unwrap_or("").into(),
                    ];
                    row = row.child(
                        action(
                            SharedString::from(format!("resolve-{index}-{n}")),
                            &format!(
                                "使用设备 {} 的值：{}",
                                event["device"]
                                    .as_str()
                                    .unwrap_or("")
                                    .chars()
                                    .take(8)
                                    .collect::<String>(),
                                event["value"]
                            ),
                        )
                        .disabled(self.busy)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.conflicts.clear();
                            this.run(args.clone(), None, cx);
                        })),
                    );
                }
            }
            body = body.child(row);
        }
        body
    }
}
