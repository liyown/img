use super::*;
use img_records::cache::Cache;
pub(super) struct CacheSettings {
    root: PathBuf,
    usage: Option<(u64, u64, u64)>,
    busy: bool,
    notice: Option<(String, bool)>,
}
impl CacheSettings {
    pub fn new(root: PathBuf, cx: &mut Context<Self>) -> Self {
        cx.spawn(async move |this, cx| {
            loop {
                if this
                    .update(cx, |this, cx| this.refresh(None, false, cx))
                    .is_err()
                {
                    break;
                }
                cx.background_executor()
                    .timer(Duration::from_secs(10))
                    .await;
            }
        })
        .detach();
        Self {
            root,
            usage: None,
            busy: false,
            notice: None,
        }
    }
    fn refresh(&mut self, limit: Option<u64>, clear: bool, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        self.busy = true;
        if clear || limit.is_some() {
            self.notice = None;
        }
        let root = self.root.clone();
        let task = cx.background_executor().spawn(async move {
            (|| -> anyhow::Result<_> {
                let cache = Cache::open(&root)?;
                if let Some(limit) = limit {
                    cache.set_limit(limit)?;
                }
                let removed = if clear || limit.is_some() {
                    cache.trim(if clear { 0 } else { cache.limit()? })?
                } else {
                    0
                };
                let (used, protected) = cache.usage()?;
                Ok((used, protected, cache.limit()?, removed))
            })()
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.busy = false;
                match result {
                    Ok((used, protected, max, removed)) => {
                        this.usage = Some((used, protected, max));
                        if clear {
                            this.notice = Some((
                                format!("已清理 {} 缓存", model::size_label(removed)),
                                false,
                            ));
                        }
                    }
                    Err(_) => {
                        this.notice =
                            Some(("无法管理缓存，请检查本机目录权限后重试。".into(), true))
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
}
impl Render for CacheSettings {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (used, protected, limit) =
            self.usage
                .unwrap_or((0, 0, img_records::cache::DEFAULT_LIMIT));
        let weak = cx.weak_entity();
        div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(label("本机缓存", 16., TEXT).font_weight(FontWeight::SEMIBOLD))
            .child(label(
                "缓存用于加快预览。清理后可重新下载，图库记录和远端图片保留。",
                12.,
                MUTED,
            ))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(label(
                        format!("已用 {}", model::size_label(used)),
                        13.,
                        TEXT,
                    ))
                    .child(div().flex_1())
                    .child(
                        action("cache-limit", &format!("上限 {}", model::size_label(limit)))
                            .disabled(self.busy)
                            .dropdown_menu(move |mut menu, _, _| {
                                for mib in [512u64, 2048, 5120, 10240] {
                                    let weak = weak.clone();
                                    let bytes = mib * 1024 * 1024;
                                    menu = menu.item(
                                        PopupMenuItem::new(model::size_label(bytes))
                                            .checked(limit == bytes)
                                            .on_click(move |_, _, cx| {
                                                let _ = weak.update(cx, |this, cx| {
                                                    this.refresh(Some(bytes), false, cx)
                                                });
                                            }),
                                    );
                                }
                                menu
                            }),
                    )
                    .child(
                        action("cache-clear", "清理缓存")
                            .disabled(self.busy || used == 0)
                            .on_click(cx.listener(|this, _, _, cx| this.refresh(None, true, cx))),
                    ),
            )
            .when(protected > 0, |body| {
                body.child(label(
                    format!(
                        "{} 用于未完成的上传任务，暂不清理。",
                        model::size_label(protected)
                    ),
                    12.,
                    MUTED,
                ))
            })
            .when_some(self.notice.as_ref(), |body, (message, error)| {
                body.child(label(
                    message.clone(),
                    12.,
                    if *error { RED } else { MUTED },
                ))
            })
    }
}
