use super::*;
pub(super) struct ScopeDialog {
    parent: WeakEntity<Library>,
    root: PathBuf,
    scopes: Vec<serde_json::Value>,
    polling: bool,
    provider: String,
    prefix: Entity<InputState>,
    error: Option<&'static str>,
}
impl ScopeDialog {
    pub fn new(
        parent: WeakEntity<Library>,
        root: PathBuf,
        provider: String,
        prefix: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(crate::i18n::text("留空索引根目录"))
                .default_value(prefix)
        });
        cx.spawn(async move |this, cx| {
            loop {
                if this.update(cx, |this, cx| this.refresh(cx)).is_err() {
                    break;
                }
                cx.background_executor().timer(Duration::from_secs(2)).await;
            }
        })
        .detach();
        Self {
            root,
            scopes: vec![],
            polling: false,
            parent,
            provider,
            prefix: input,
            error: None,
        }
    }
    fn refresh(&mut self, cx: &mut Context<Self>) {
        if self.polling {
            return;
        }
        self.polling = true;
        let root = self.root.clone();
        let provider = self.provider.clone();
        let task = cx.background_executor().spawn(async move {
            (|| -> anyhow::Result<Vec<serde_json::Value>> {
                let c = Catalog::open(&root)?;
                let mut scopes = vec![];
                for (_, body) in c.settings_prefix("scope:")? {
                    let mut scope: serde_json::Value = serde_json::from_str(&body)?;
                    if scope["provider"] != provider {
                        continue;
                    }
                    let progress = c
                        .task(&format!("index:{}", scope["id"].as_str().unwrap_or("")))
                        .ok()
                        .and_then(|body| serde_json::from_str::<serde_json::Value>(&body).ok());
                    scope["count"] = progress
                        .as_ref()
                        .and_then(|p| p["seen"].as_array())
                        .map(|a| a.len())
                        .unwrap_or(0)
                        .into();
                    scope["complete"] = progress
                        .as_ref()
                        .is_some_and(|p| p["complete"] == true)
                        .into();
                    scopes.push(scope);
                }
                Ok(scopes)
            })()
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.polling = false;
                match result {
                    Ok(scopes) => this.scopes = scopes,
                    Err(_) => this.error = Some("无法读取索引范围，请稍后重试。"),
                };
                cx.notify();
            });
        })
        .detach();
    }
    fn toggle(&mut self, id: String, enabled: bool, cx: &mut Context<Self>) {
        let root = self.root.clone();
        let task = cx.background_executor().spawn(async move {
            (|| -> anyhow::Result<()> {
                let mut c = Catalog::open(&root)?;
                let key = format!("scope:{id}");
                let mut scope: serde_json::Value = serde_json::from_str(
                    &c.setting(&key)?
                        .ok_or_else(|| anyhow::anyhow!("scope missing"))?,
                )?;
                scope["enabled"] = enabled.into();
                c.sync_set(&key, "enabled", Some(enabled.into()))?;
                c.set_setting(&key, &serde_json::to_string(&scope)?)?;
                Ok(())
            })()
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                if result.is_err() {
                    this.error = Some("无法保存索引范围，请稍后重试。");
                }
                this.refresh(cx);
                cx.notify();
            });
        })
        .detach();
    }
}
impl Render for ScopeDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let busy = self
            .parent
            .upgrade()
            .is_some_and(|parent| parent.read(cx).busy);
        let mut ranges = div()
            .id("index-ranges")
            .max_h(px(180.))
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap(px(8.));
        for scope in self.scopes.clone() {
            let id = scope["id"].as_str().unwrap_or("").to_owned();
            let enabled = scope["enabled"] == true;
            let prefix = scope["prefix"]
                .as_str()
                .filter(|s| !s.is_empty())
                .unwrap_or("根目录");
            ranges =
                ranges.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(10.))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .child(label(prefix.to_owned(), 12., TEXT).text_ellipsis())
                                .child(label(
                                    format!(
                                        "已索引 {} 张 · {}",
                                        scope["count"],
                                        crate::i18n::text(if !enabled {
                                            "已暂停"
                                        } else if scope["complete"] == true {
                                            "自动刷新"
                                        } else {
                                            "尚未完成"
                                        })
                                    ),
                                    11.,
                                    MUTED,
                                )),
                        )
                        .child(
                            action(
                                SharedString::from(format!("scope-toggle-{id}")),
                                if enabled { "暂停" } else { "继续" },
                            )
                            .ghost()
                            .on_click(cx.listener(
                                move |this, _, _, cx| this.toggle(id.clone(), !enabled, cx),
                            )),
                        ),
                );
        }
        div()
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(label(format!("存储源：{}", self.provider), 13., TEXT))
            .when(!self.scopes.is_empty(), |body| body.child(ranges))
            .child(label("选择需要加入图库的目录", 12., TEXT))
            .child(
                Input::new(&self.prefix)
                    .aria_label(crate::i18n::text("索引目录"))
                    .when(self.error.is_some(), |input| {
                        input.border_color(crate::theme::color(RED))
                    }),
            )
            .when_some(self.error, |body, error| body.child(label(error, 12., RED)))
            .child(label(
                "包含所有子目录。应用运行期间每 15 分钟刷新，可随时暂停。",
                12.,
                MUTED,
            ))
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap(px(8.))
                    .child(
                        action("index-cancel", "取消")
                            .ghost()
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        action("index-confirm", "开始索引")
                            .primary()
                            .disabled(busy)
                            .on_click(cx.listener(|this, _, window, cx| {
                                let value = this.prefix.read(cx).value().to_string();
                                let value = value.trim();
                                if value.starts_with('/')
                                    || value.contains('\\')
                                    || value.chars().any(char::is_control)
                                    || (!value.is_empty()
                                        && value.trim_end_matches('/').split('/').any(|part| {
                                            part.is_empty() || part == "." || part == ".."
                                        }))
                                {
                                    this.error = Some(
                                        "请输入相对目录，例如 photos/，不要以 / 开头或包含 ..",
                                    );
                                    cx.notify();
                                    return;
                                }
                                let prefix = if value.is_empty() {
                                    String::new()
                                } else {
                                    format!("{}/", value.trim_end_matches('/'))
                                };
                                let provider = this.provider.clone();
                                let _ = this.parent.update(cx, |parent, cx| {
                                    parent.command(
                                        vec![
                                            "library".into(),
                                            "index".into(),
                                            "--provider".into(),
                                            provider,
                                            "--prefix".into(),
                                            prefix,
                                            "--resume".into(),
                                        ],
                                        cx,
                                    )
                                });
                                window.close_dialog(cx);
                            })),
                    ),
            )
    }
}
