use super::*;
use std::io::Write;

#[derive(Clone, serde::Deserialize)]
struct RemoteItem {
    path: String,
    url: String,
    size: u64,
    version: String,
    directory: bool,
}
#[derive(serde::Deserialize)]
struct RemotePage {
    items: Vec<RemoteItem>,
    next: Option<String>,
}
#[derive(Default)]
pub(super) struct RemoteState {
    snapshot: Option<std::sync::Arc<model::UploadConfiguration>>,
    items: Vec<RemoteItem>,
    prefix: String,
    next: Option<String>,
    provider: String,
    busy: bool,
}
impl ImgDesktop {
    pub(super) fn remote_controls(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(label("远端文件", 16., TEXT))
            .child(label(
                "浏览当前存储源。远端删除会影响已有链接，与图库中的本地清理相互独立。",
                12.,
                MUTED,
            ))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(
                        action("remote-root", "打开根目录")
                            .disabled(self.remote.busy || self.provider.is_empty())
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.remote = Default::default();
                                this.load_remote(String::new(), None, cx)
                            })),
                    )
                    .child(
                        action("remote-parent", "上一级")
                            .disabled(self.remote.busy || self.remote.prefix.is_empty())
                            .on_click(cx.listener(|this, _, _, cx| {
                                let prefix = this
                                    .remote
                                    .prefix
                                    .trim_end_matches('/')
                                    .rsplit_once('/')
                                    .map(|(p, _)| format!("{p}/"))
                                    .unwrap_or_default();
                                this.load_remote(prefix, None, cx);
                            })),
                    )
                    .child(
                        label(
                            format!("{} /{}", self.remote.provider, self.remote.prefix),
                            12.,
                            MUTED,
                        )
                        .truncate(),
                    ),
            )
            .when(!self.remote.items.is_empty(), |this| {
                this.child(
                    uniform_list(
                        "remote-files",
                        self.remote.items.len(),
                        cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                            range
                                .map(|index| {
                                    let item = this.remote.items[index].clone();
                                    let target = item.clone();
                                    let url = item.url.clone();
                                    div()
                                        .h(px(48.))
                                        .flex()
                                        .items_center()
                                        .gap(px(10.))
                                        .border_b_1()
                                        .border_color(crate::theme::color(BORDER))
                                        .child(
                                            label(item.path.clone(), 12., TEXT)
                                                .flex_1()
                                                .min_w_0()
                                                .truncate(),
                                        )
                                        .child(label(
                                            if item.directory {
                                                "目录".into()
                                            } else {
                                                format!("{} KB", item.size.div_ceil(1024))
                                            },
                                            11.,
                                            MUTED,
                                        ))
                                        .child(
                                            action(
                                                SharedString::from(format!("remote-open-{index}")),
                                                if item.directory {
                                                    "打开"
                                                } else {
                                                    "复制链接"
                                                },
                                            )
                                            .disabled(this.remote.busy)
                                            .on_click(
                                                cx.listener(move |this, _, _, cx| {
                                                    if target.directory {
                                                        this.load_remote(
                                                            format!(
                                                                "{}/",
                                                                target.path.trim_end_matches('/')
                                                            ),
                                                            None,
                                                            cx,
                                                        );
                                                    } else {
                                                        cx.write_to_clipboard(
                                                            ClipboardItem::new_string(url.clone()),
                                                        );
                                                        this.message("已复制链接", false, cx);
                                                    }
                                                }),
                                            ),
                                        )
                                        .when(!item.directory, |row| {
                                            row.child(
                                                action(
                                                    SharedString::from(format!(
                                                        "remote-delete-{index}"
                                                    )),
                                                    "远端删除",
                                                )
                                                .disabled(
                                                    this.remote.busy || item.version.is_empty(),
                                                )
                                                .on_click(cx.listener(
                                                    move |this, _, window, cx| {
                                                        this.confirm_remote_delete(
                                                            item.clone(),
                                                            window,
                                                            cx,
                                                        )
                                                    },
                                                )),
                                            )
                                        })
                                })
                                .collect()
                        }),
                    )
                    .h(px(280.))
                    .w_full(),
                )
            })
            .when(
                self.remote.items.is_empty() && !self.remote.provider.is_empty(),
                |this| this.child(label("此目录没有文件", 12., MUTED)),
            )
            .when(self.remote.next.is_some(), |this| {
                this.child(
                    action("remote-next", "加载下一页")
                        .disabled(self.remote.busy)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.load_remote(
                                this.remote.prefix.clone(),
                                this.remote.next.clone(),
                                cx,
                            )
                        })),
                )
            })
            .into_any_element()
    }
    fn load_remote(&mut self, prefix: String, cursor: Option<String>, cx: &mut Context<Self>) {
        if self.remote.busy || self.shutting_down {
            return;
        }
        self.remote.busy = true;
        let snapshot = self.remote.snapshot.clone();
        let target = if snapshot.is_some() {
            self.remote.provider.clone()
        } else {
            self.provider.clone()
        };
        let binary = self.engine.clone();
        let root = self.root.clone();
        let next_prefix = prefix.clone();
        let append =
            cursor.is_some() && self.remote.provider == target && self.remote.prefix == prefix;
        let task = cx.background_executor().spawn(async move {
            let snapshot = match snapshot {
                Some(snapshot) => snapshot,
                None => std::sync::Arc::new(model::UploadConfiguration::capture(&target)?),
            };
            let mut file = tempfile::NamedTempFile::new_in(&root)?;
            file.write_all(snapshot.config.as_bytes())?;
            let mut command = std::process::Command::new(binary);
            command
                .envs(&snapshot.environment)
                .env("IMG_DATA_DIR", &root)
                .arg("--config")
                .arg(file.path())
                .arg("remote")
                .arg("--provider")
                .arg(&target)
                .arg("list")
                .arg("--prefix")
                .arg(prefix);
            if let Some(cursor) = cursor {
                command.arg("--cursor").arg(cursor);
            }
            let output = engine::run(command, &Control::default())?;
            anyhow::ensure!(
                output.success,
                "无法读取远端文件，请检查存储源是否支持浏览以及目录读取权限"
            );
            let page: RemotePage = serde_json::from_slice(&output.stdout)?;
            Ok::<_, anyhow::Error>((target, page, snapshot))
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.remote.busy = false;
                match result {
                    Ok((provider, page, snapshot)) => {
                        this.remote.snapshot = Some(snapshot);
                        if !append {
                            this.remote.items.clear();
                        }
                        this.remote.items.extend(page.items);
                        this.remote.next = page.next;
                        this.remote.provider = provider;
                        this.remote.prefix = next_prefix;
                        cx.notify();
                    }
                    Err(e) => this.message(e.to_string(), true, cx),
                }
            });
        })
        .detach();
        cx.notify();
    }
    fn confirm_remote_delete(
        &mut self,
        item: RemoteItem,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.remote.busy || self.shutting_down {
            return;
        }
        let provider = self.remote.provider.clone();
        let Some(snapshot) = self.remote.snapshot.clone() else {
            return;
        };
        self.remote.busy = true;
        let prompt = crate::i18n::prompt(
            window,
            PromptLevel::Warning,
            "删除远端图片？",
            Some(&format!(
                "存储源：{provider}\n文件：{}\n已有链接可能立即失效。仅删除此文件，不删除目录；操作前检查文件版本。原文件和本地记录保留。",
                item.path
            )),
            &["取消", "删除远端文件"],
            cx,
        );
        let binary = self.engine.clone();
        let root = self.root.clone();
        cx.spawn(async move |this, cx| {
            if prompt.await.ok() != Some(1) {
                let _ = this.update(cx, |this, cx| {
                    this.remote.busy = false;
                    cx.notify();
                });
                return;
            }
            let deleted = item.path.clone();
            let result = cx
                .background_executor()
                .spawn(async move {
                    let mut file = tempfile::NamedTempFile::new_in(&root)?;
                    file.write_all(snapshot.config.as_bytes())?;
                    let mut command = std::process::Command::new(binary);
                    command
                        .envs(&snapshot.environment)
                        .env("IMG_DATA_DIR", root)
                        .arg("--config")
                        .arg(file.path())
                        .arg("remote")
                        .arg("--provider")
                        .arg(provider)
                        .arg("delete")
                        .arg("--version")
                        .arg(item.version)
                        .arg("--yes")
                        .arg("--")
                        .arg(item.path);
                    let output = engine::run(command, &Control::default())?;
                    anyhow::ensure!(
                        output.success,
                        "远端删除未完成：文件可能已变化，或当前凭据没有删除权限。请刷新后重试。"
                    );
                    Ok::<_, anyhow::Error>(())
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.remote.busy = false;
                match result {
                    Ok(()) => {
                        this.remote.items.retain(|i| i.path != deleted);
                        this.message("远端文件已删除，本地记录保留", false, cx);
                    }
                    Err(e) => this.message(e.to_string(), true, cx),
                }
            });
        })
        .detach();
        cx.notify();
    }
}
