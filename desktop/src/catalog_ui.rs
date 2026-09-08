use super::*;
use img_records::catalog::{Asset, Catalog, CatalogQuery};
use std::collections::HashSet;

pub(super) struct Library {
    root: PathBuf,
    engine: PathBuf,
    assets: Vec<Asset>,
    pub total: usize,
    matching: HashSet<String>,
    selected: HashSet<String>,
    selecting: bool,
    query: CatalogQuery,
    busy: bool,
    control: Option<Control>,
    generation: u64,
    stamp: String,
    notice: Option<String>,
    grid: bool,
    format: CopyFormat,
    providers: Vec<String>,
    scroll: UniformListScrollHandle,
    prefix: Entity<InputState>,
    _subscriptions: Vec<Subscription>,
}
impl Library {
    pub fn new(
        root: PathBuf,
        engine: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let prefix =
            cx.new(|cx| InputState::new(window, cx).placeholder(crate::i18n::text("目录前缀")));
        let sub = cx.subscribe(&prefix, |this, field, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                this.query.prefix = field.read(cx).value().to_string();
                this.changed(cx);
            }
        });
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(2)).await;
                if this.update(cx, |this, cx| this.refresh(cx)).is_err() {
                    break;
                }
            }
        })
        .detach();
        let preferences = Preferences::load(&root).unwrap_or_default();
        Self {
            root,
            engine,
            assets: vec![],
            total: 0,
            matching: HashSet::new(),
            selected: HashSet::new(),
            selecting: false,
            query: CatalogQuery {
                limit: 200,
                ..Default::default()
            },
            busy: false,
            control: None,
            generation: 0,
            stamp: String::new(),
            notice: None,
            grid: preferences.library_view == LibraryView::Grid,
            format: preferences.copy_format,
            providers: vec![],
            scroll: UniformListScrollHandle::new(),
            prefix,
            _subscriptions: vec![sub],
        }
    }
    pub fn search(&mut self, text: String, cx: &mut Context<Self>) {
        if self.query.text != text {
            self.query.text = text;
            self.changed(cx)
        }
    }
    pub fn leave(&mut self, cx: &mut Context<Self>) {
        self.selected.clear();
        self.selecting = false;
        cx.notify();
    }
    fn changed(&mut self, cx: &mut Context<Self>) {
        self.query.offset = 0;
        self.generation += 1;
        self.stamp.clear();
        self.scroll = UniformListScrollHandle::new();
        self.refresh(cx);
        cx.notify();
    }
    fn refresh(&mut self, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let stamp = ["catalog.sqlite3", "catalog.sqlite3-wal", "queue.json"]
            .iter()
            .map(|n| {
                std::fs::metadata(self.root.join(n))
                    .map(|m| format!("{:?}:{}", m.modified(), m.len()))
                    .unwrap_or_default()
            })
            .collect::<String>();
        if stamp == self.stamp && !stamp.is_empty() {
            return;
        }
        self.stamp = stamp;
        self.busy = true;
        let root = self.root.clone();
        let query = self.query.clone();
        let generation = self.generation;
        let task = cx.background_executor().spawn(async move {
            (|| -> anyhow::Result<_> {
                let mut c = Catalog::open(&root)?;
                c.import_legacy()?;
                let page = c.query(&query)?;
                let ids = c.query_ids(&query)?;
                let providers = storage::configured_providers()?
                    .0
                    .into_iter()
                    .map(|p| p.0)
                    .collect::<Vec<_>>();
                Ok((page, ids, providers))
            })()
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.busy = false;
                if this.generation != generation {
                    this.stamp.clear();
                    this.refresh(cx);
                    return;
                }
                match result {
                    Ok((page, ids, providers)) => {
                        this.assets = page.assets;
                        this.total = page.total;
                        this.matching = ids.into_iter().collect();
                        this.providers = providers;
                    }
                    Err(e) => {
                        this.notice = Some(e.to_string());
                        this.stamp.clear();
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }
    pub fn stop(&mut self) -> Option<Control> {
        if let Some(control) = &self.control {
            control.stop(engine::CANCEL);
        }
        self.control.take()
    }
    fn command(&mut self, args: Vec<String>, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        self.busy = true;
        let engine = self.engine.clone();
        let root = self.root.clone();
        let control = Control::default();
        self.control = Some(control.clone());
        let task = cx.background_executor().spawn(async move {
            (|| -> anyhow::Result<String> {
                let _completion = control.completion();
                let mut command = std::process::Command::new(engine);
                command
                    .arg("--config")
                    .arg(storage::config_path()?)
                    .args(args)
                    .env("IMG_DATA_DIR", root);
                let output = engine::run(command, &control)?;
                if output.stopped != 0 {
                    anyhow::bail!("操作已取消");
                }
                if !output.success && output.stdout.is_empty() {
                    anyhow::bail!("图库操作失败，请检查存储源配置后重试");
                }
                let result: serde_json::Value = serde_json::from_slice(&output.stdout)?;
                if let Some(error) = result["error"].as_str() {
                    anyhow::bail!("{error}");
                }
                let files = result["files"].as_array();
                let failed = files
                    .map(|rows| {
                        rows.iter()
                            .filter(|row| {
                                row["success"] == false
                                    || row["check"]["accessible"] == false
                                    || row.get("error").is_some()
                            })
                            .count()
                    })
                    .unwrap_or(0);
                Ok(format!(
                    "完成 {} 项，失败 {failed} 项",
                    files.map(|a| a.len()).unwrap_or(0)
                ))
            })()
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.busy = false;
                this.control = None;
                this.notice = Some(match result {
                    Ok(message) => message,
                    Err(e) => e.to_string(),
                });
                this.stamp.clear();
                this.refresh(cx);
                cx.notify();
            });
        })
        .detach();
    }
    fn copy(&mut self, cx: &mut Context<Self>) {
        let ids = self.selected.clone();
        let root = self.root.clone();
        let provider = self.query.provider.clone();
        let format = self.format;
        let task = cx.background_executor().spawn(async move {
            (|| -> anyhow::Result<_> {
                let c = Catalog::open(&root)?;
                let mut assets = ids
                    .iter()
                    .map(|id| c.get(id))
                    .collect::<anyhow::Result<Vec<_>>>()?;
                assets.sort_by(|a, b| b.added_at.cmp(&a.added_at).then(a.id.cmp(&b.id)));
                let lines = assets
                    .iter()
                    .filter_map(|a| {
                        a.selected_location(&provider)
                            .map(|l| format.render(&a.name, &l.url))
                    })
                    .collect::<Vec<_>>();
                Ok((lines.join("\n"), lines.len(), assets.len() - lines.len()))
            })()
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok((text, count, skipped)) => {
                        if count > 0 {
                            cx.write_to_clipboard(ClipboardItem::new_string(text));
                        }
                        this.notice = Some(format!("已复制 {count} 项，跳过 {skipped} 项"));
                    }
                    Err(e) => this.notice = Some(e.to_string()),
                }
                cx.notify();
            });
        })
        .detach();
    }
    fn detail(&self, asset: Asset, window: &mut Window, cx: &mut Context<Self>) {
        let selected = asset.selected_location(&self.query.provider).cloned();
        let hash = asset.content_hash.clone();
        let image = hash
            .map(|h| self.root.join("cache").join(h))
            .filter(|p| p.is_file());
        window.open_dialog(cx, move |dialog, _, _| {
            let mut body = div().flex().flex_col().gap(px(10.)).child(label(
                format!("{} · {}", asset.content_type, model::size_label(asset.size)),
                12.,
                MUTED,
            ));
            if let Some(image) = &image {
                body = body.child(
                    img(image.clone())
                        .w_full()
                        .h(px(280.))
                        .object_fit(ObjectFit::Contain),
                );
            } else {
                body = body.child(label("尚未缓存预览，可下载远端图片", 12., MUTED));
            }
            for location in &asset.locations {
                body = body.child(
                    label(
                        format!(
                            "{} · {} · {}",
                            location.provider, location.availability, location.url
                        ),
                        12.,
                        TEXT,
                    )
                    .text_ellipsis(),
                );
            }
            let link = selected.clone();
            dialog
                .title(crate::i18n::text(asset.name.clone()))
                .child(body)
                .footer(
                    div()
                        .flex()
                        .justify_end()
                        .gap(px(8.))
                        .child(
                            action("detail-copy", "复制链接")
                                .disabled(link.is_none())
                                .on_click(move |_, _, cx| {
                                    if let Some(link) = &link {
                                        cx.write_to_clipboard(ClipboardItem::new_string(
                                            link.url.clone(),
                                        ));
                                    }
                                }),
                        )
                        .child(
                            action("detail-close", "关闭预览")
                                .on_click(|_, window, cx| window.close_dialog(cx)),
                        ),
                )
        });
    }
    fn cell(&self, asset: Asset, cx: &mut Context<Self>) -> AnyElement {
        let id = asset.id.clone();
        let target = asset.clone();
        let hash = asset
            .content_hash
            .as_ref()
            .map(|h| self.root.join("cache").join(h))
            .filter(|p| p.is_file());
        let preview = div()
            .w_full()
            .h(px(if self.grid { 132. } else { 45. }))
            .bg(crate::theme::color(DROP))
            .flex()
            .items_center()
            .justify_center()
            .when_some(hash, |d, path| {
                d.child(img(path).size_full().object_fit(ObjectFit::Contain))
            });
        div()
            .id(SharedString::from(format!("catalog-{}", asset.id)))
            .flex()
            .flex_col()
            .gap(px(5.))
            .p(px(10.))
            .h(px(if self.grid { 220. } else { 115. }))
            .rounded(px(9.))
            .border_1()
            .border_color(crate::theme::color(BORDER))
            .bg(crate::theme::color(CARD))
            .child(
                Button::new(SharedString::from(format!("preview-{}", asset.id)))
                    .ghost()
                    .w_full()
                    .h(px(if self.grid { 140. } else { 50. }))
                    .child(preview)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.detail(target.clone(), window, cx)
                    })),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .when(self.selecting, |d| {
                        d.child(
                            gpui_kit::component::checkbox::Checkbox::new(SharedString::from(
                                format!("select-{id}"),
                            ))
                            .checked(self.selected.contains(&id))
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    if !this.selected.remove(&id) {
                                        this.selected.insert(id.clone());
                                    }
                                    cx.notify();
                                },
                            )),
                        )
                    })
                    .child(label(asset.name, 12., TEXT).text_ellipsis()),
            )
            .child(label(
                format!(
                    "{} · {} 个地址",
                    model::size_label(asset.size),
                    asset.locations.len()
                ),
                11.,
                MUTED,
            ))
            .into_any_element()
    }
}
impl Render for Library {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut view = div().flex().flex_col().size_full().p(px(22.)).gap(px(12.));
        let weak = cx.entity().downgrade();
        let mut top = div()
            .flex()
            .items_center()
            .gap(px(8.))
            .child(label("图库", 20., TEXT))
            .child(label(format!("{} 项 · 已索引范围", self.total), 11., MUTED))
            .child(div().flex_1());
        top = top
            .child(
                action(
                    "catalog-provider",
                    if self.query.provider.is_empty() {
                        "全部图床"
                    } else {
                        &self.query.provider
                    },
                )
                .dropdown_menu({
                    let providers = self.providers.clone();
                    move |mut menu, _, _| {
                        for name in std::iter::once(String::new()).chain(providers.clone()) {
                            let weak = weak.clone();
                            menu = menu.item(
                                PopupMenuItem::new(crate::i18n::text(if name.is_empty() {
                                    "全部图床".into()
                                } else {
                                    name.clone()
                                }))
                                .on_click(move |_, _, cx| {
                                    let _ = weak.update(cx, |this, cx| {
                                        this.query.provider = name.clone();
                                        this.changed(cx);
                                    });
                                }),
                            );
                        }
                        menu
                    }
                }),
            )
            .child(
                action("catalog-view", if self.grid { "列表" } else { "网格" }).on_click(
                    cx.listener(|this, _, _, cx| {
                        this.grid = !this.grid;
                        cx.notify();
                    }),
                ),
            )
            .child(
                action(
                    "catalog-select",
                    if self.selecting { "完成" } else { "选择" },
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.selecting = !this.selecting;
                    if !this.selecting {
                        this.selected.clear();
                    }
                    cx.notify();
                })),
            );
        view = view.child(top).child(Input::new(&self.prefix).small());
        if self.selecting {
            let hidden = self
                .selected
                .iter()
                .filter(|id| !self.matching.contains(*id))
                .count();
            view = view.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(label(
                        format!(
                            "已选 {} 项，其中 {hidden} 项不在当前搜索结果中",
                            self.selected.len()
                        ),
                        12.,
                        MUTED,
                    ))
                    .child(
                        div()
                            .flex()
                            .gap(px(8.))
                            .child(
                                action("catalog-all", "全选当前结果")
                                    .disabled(self.busy)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.selected.extend(this.matching.iter().cloned());
                                        cx.notify();
                                    })),
                            )
                            .child(action("catalog-none", "取消全部").on_click(cx.listener(
                                |this, _, _, cx| {
                                    this.selected.clear();
                                    cx.notify();
                                },
                            )))
                            .child(
                                action("catalog-copy", "复制所选")
                                    .disabled(self.selected.is_empty())
                                    .on_click(cx.listener(|this, _, _, cx| this.copy(cx))),
                            )
                            .child(
                                action("catalog-check", "检查链接")
                                    .disabled(self.selected.is_empty() || self.busy)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        let mut args = vec!["library".into(), "check".into()];
                                        args.extend(this.selected.iter().cloned());
                                        if !this.query.provider.is_empty() {
                                            args.extend([
                                                "--provider".into(),
                                                this.query.provider.clone(),
                                            ]);
                                        }
                                        this.command(args, cx);
                                    })),
                            ),
                    ),
            );
        }
        if let Some(notice) = &self.notice {
            view = view.child(label(notice.clone(), 12., MUTED));
        }
        let columns = if self.grid {
            if window.viewport_size().width < px(1180.) {
                3
            } else {
                4
            }
        } else {
            1
        };
        view = view.child(
            uniform_list(
                "catalog-list",
                self.assets.len().div_ceil(columns),
                cx.processor(move |this, range: std::ops::Range<usize>, _, cx| {
                    range
                        .map(|row| {
                            let mut line = div()
                                .flex()
                                .gap(px(10.))
                                .pb(px(10.))
                                .h(px(if this.grid { 230. } else { 125. }));
                            for col in 0..columns {
                                line = line.child(
                                    div().flex_1().min_w_0().children(
                                        this.assets
                                            .get(row * columns + col)
                                            .cloned()
                                            .map(|asset| this.cell(asset, cx)),
                                    ),
                                );
                            }
                            line
                        })
                        .collect()
                }),
            )
            .track_scroll(&self.scroll)
            .flex_1()
            .min_h_0(),
        );
        view.child(
            div()
                .flex()
                .gap(px(8.))
                .child(
                    action("catalog-prev", "上一页")
                        .disabled(self.query.offset == 0 || self.busy)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.query.offset = this.query.offset.saturating_sub(200);
                            this.stamp.clear();
                            this.generation += 1;
                            this.refresh(cx);
                        })),
                )
                .child(label(
                    format!(
                        "{} / {}",
                        self.query.offset / 200 + 1,
                        self.total.div_ceil(200).max(1)
                    ),
                    12.,
                    MUTED,
                ))
                .child(
                    action("catalog-next", "下一页")
                        .disabled(self.query.offset + 200 >= self.total || self.busy)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.query.offset += 200;
                            this.stamp.clear();
                            this.generation += 1;
                            this.refresh(cx);
                        })),
                ),
        )
        .into_any_element()
    }
}
