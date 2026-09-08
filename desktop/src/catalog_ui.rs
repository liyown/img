use super::*;
#[path = "copy_dialog.rs"]
mod copy_dialog;
#[path = "catalog_detail.rs"]
mod detail;
#[path = "scope_dialog.rs"]
mod scope_dialog;
use img_records::catalog::{Asset, Catalog, CatalogQuery};
use std::collections::HashSet;

pub(super) enum PreferenceChanged {
    Tools {
        paths: Vec<PathBuf>,
        sources: HashMap<PathBuf, String>,
    },
    Format(CopyFormat),
    View(bool),
}
impl EventEmitter<PreferenceChanged> for Library {}
pub(super) struct Library {
    root: PathBuf,
    engine: PathBuf,
    assets: Vec<Asset>,
    previews: HashMap<String, String>,
    preview_jobs: HashMap<String, Control>,
    preview_failures: HashMap<String, std::time::Instant>,
    stopping: bool,
    thumbnails: Entity<crate::thumbnails::ThumbnailCache>,
    pub total: usize,
    pub all_total: usize,
    matching: HashSet<String>,
    selected: crate::selection::Ids,
    selecting: bool,
    query: CatalogQuery,
    busy: bool,
    loading: bool,
    control: Option<Control>,
    generation: u64,
    stamp: String,
    notice: Option<String>,
    copied: Option<(String, std::time::Instant)>,
    grid: bool,
    format: CopyFormat,
    providers: Vec<String>,
    manageable: HashSet<String>,
    scroll: UniformListScrollHandle,
}
impl Library {
    pub fn new(
        root: PathBuf,
        engine: PathBuf,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(2)).await;
                if this.update(cx, |this, cx| this.refresh(cx)).is_err() {
                    break;
                }
            }
        })
        .detach();
        let weak = cx.weak_entity();
        cx.defer(move |cx| {
            let _ = weak.update(cx, |this, cx| this.refresh(cx));
        });
        let thumbnails = crate::thumbnails::ThumbnailCache::new(root.clone(), cx);
        let preferences = Preferences::load(&root).unwrap_or_default();
        Self {
            root,
            engine,
            assets: vec![],
            previews: HashMap::new(),
            preview_jobs: HashMap::new(),
            preview_failures: HashMap::new(),
            stopping: false,
            thumbnails,
            total: 0,
            all_total: 0,
            matching: HashSet::new(),
            selected: Default::default(),
            selecting: false,
            query: CatalogQuery {
                limit: 200,
                ..Default::default()
            },
            busy: false,
            loading: false,
            control: None,
            generation: 0,
            stamp: String::new(),
            notice: None,
            copied: None,
            grid: preferences.library_view == LibraryView::Grid,
            format: preferences.copy_format,
            providers: vec![],
            manageable: HashSet::new(),
            scroll: UniformListScrollHandle::new(),
        }
    }
    pub fn preferences(&mut self, preferences: Preferences, cx: &mut Context<Self>) {
        self.format = preferences.copy_format;
        self.grid = preferences.library_view == LibraryView::Grid;
        cx.notify();
    }
    fn filter_menu(&self, cx: &mut Context<Self>) -> AnyElement {
        let active = !self.query.content_type.is_empty() || self.query.since.is_some();
        let content_type = self.query.content_type.clone();
        let since = self.query.since;
        let weak = cx.weak_entity();
        Button::new("catalog-filters")
            .ghost()
            .icon(IconName::Settings2)
            .w(px(32.))
            .h(px(32.))
            .selected(active)
            .accessibility_label(crate::i18n::text("筛选图片"))
            .tooltip(crate::i18n::text(if active {
                "筛选已启用"
            } else {
                "筛选图片"
            }))
            .dropdown_menu(move |menu, window, cx| {
                let format_view = weak.clone();
                let time_view = weak.clone();
                let reset_view = weak.clone();
                let selected_type = content_type.clone();
                menu.submenu(
                    crate::i18n::text("格式"),
                    window,
                    cx,
                    move |mut menu, _, _| {
                        for (value, title) in [
                            ("", "全部格式"),
                            ("image/png", "PNG"),
                            ("image/jpeg", "JPEG"),
                            ("image/webp", "WebP"),
                            ("image/gif", "GIF"),
                            ("image/svg+xml", "SVG"),
                            ("image/avif", "AVIF"),
                        ] {
                            let view = format_view.clone();
                            menu = menu.item(
                                PopupMenuItem::new(crate::i18n::text(title))
                                    .checked(value == selected_type)
                                    .on_click(move |_, _, cx| {
                                        let _ = view.update(cx, |this, cx| {
                                            this.query.content_type = value.into();
                                            this.changed(cx);
                                        });
                                    }),
                            );
                        }
                        menu
                    },
                )
                .submenu(
                    crate::i18n::text("加入时间"),
                    window,
                    cx,
                    move |mut menu, _, _| {
                        let now = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs();
                        let selected_days =
                            since.map(|start| (now.saturating_sub(start) + 43200) / 86400);
                        for (days, title) in [
                            (None, "全部时间"),
                            (Some(7), "最近 7 天"),
                            (Some(30), "最近 30 天"),
                            (Some(90), "最近 90 天"),
                        ] {
                            let view = time_view.clone();
                            menu = menu.item(
                                PopupMenuItem::new(crate::i18n::text(title))
                                    .checked(days == selected_days)
                                    .on_click(move |_, _, cx| {
                                        let _ = view.update(cx, |this, cx| {
                                            this.query.since =
                                                days.map(|days| now.saturating_sub(days * 86400));
                                            this.changed(cx);
                                        });
                                    }),
                            );
                        }
                        menu
                    },
                )
                .separator()
                .item(
                    PopupMenuItem::new(crate::i18n::text("重置筛选"))
                        .disabled(!active)
                        .on_click(move |_, _, cx| {
                            let _ = reset_view.update(cx, |this, cx| {
                                this.query.content_type.clear();
                                this.query.since = None;
                                this.changed(cx);
                            });
                        }),
                )
            })
            .into_any_element()
    }
    fn format_picker(&self, cx: &mut Context<Self>) -> AnyElement {
        let weak = cx.weak_entity();
        let format = self.format;
        action("catalog-format", format.label())
            .dropdown_menu(move |mut menu, _, _| {
                for value in CopyFormat::ALL {
                    let weak = weak.clone();
                    menu = menu.item(
                        PopupMenuItem::new(crate::i18n::text(value.label()))
                            .checked(value == format)
                            .on_click(move |_, _, cx| {
                                let _ = weak.update(cx, |this, cx| {
                                    this.format = value;
                                    cx.emit(PreferenceChanged::Format(value));
                                    cx.notify();
                                });
                            }),
                    );
                }
                menu
            })
            .into_any_element()
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
        if self.loading || self.stopping {
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
        self.loading = true;
        let root = self.root.clone();
        let query = self.query.clone();
        let generation = self.generation;
        let task = cx.background_executor().spawn(async move {
            (|| -> anyhow::Result<_> {
                let mut c = Catalog::open(&root)?;
                c.import_legacy()?;
                let page = c.query(&query)?;
                let ids = c.query_ids(&query)?;
                let sources = storage::configured_providers()?.0;
                let manageable = sources
                    .iter()
                    .filter(|(_, kind)| matches!(kind.as_str(), "s3" | "github" | "webdav"))
                    .map(|(name, _)| name.clone())
                    .collect();
                let providers = sources.into_iter().map(|p| p.0).collect::<Vec<_>>();
                let previews = page
                    .assets
                    .iter()
                    .filter_map(|a| c.preview_key(a).ok().flatten().map(|k| (a.id.clone(), k)))
                    .collect();
                let valid = c
                    .query_ids(&CatalogQuery {
                        include_hidden: true,
                        ..Default::default()
                    })?
                    .into_iter()
                    .collect::<HashSet<_>>();
                Ok((page, ids, providers, previews, valid, manageable))
            })()
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.loading = false;
                if this.generation != generation {
                    this.stamp.clear();
                    this.refresh(cx);
                    return;
                }
                match result {
                    Ok((page, ids, providers, previews, valid, manageable)) => {
                        this.assets = page.assets;
                        this.previews = previews;

                        this.total = page.total;
                        this.all_total = valid.len();
                        this.matching = ids.into_iter().collect();
                        this.selected.reconcile(&this.matching, &valid);
                        this.providers = providers;
                        this.manageable = manageable;
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
    pub fn stop(&mut self) -> Vec<Control> {
        self.stopping = true;
        let mut controls = self.preview_jobs.values().cloned().collect::<Vec<_>>();
        controls.extend(self.control.take());
        for control in &controls {
            control.stop(engine::CANCEL);
        }
        controls
    }
    fn load_visible_preview(&mut self, asset: &Asset, cx: &mut Context<Self>) {
        if self.stopping
            || self.preview_jobs.len() >= 2
            || self.preview_jobs.contains_key(&asset.id)
        {
            return;
        }
        if self
            .previews
            .get(&asset.id)
            .is_some_and(|key| self.root.join("cache").join(key).is_file())
            || self
                .preview_failures
                .get(&asset.id)
                .is_some_and(|time| time.elapsed() < Duration::from_secs(60))
        {
            return;
        }
        let Some(location) = asset.selected_location(&self.query.provider) else {
            return;
        };
        if location.path.is_none() || !self.manageable.contains(&location.provider) {
            return;
        }
        let id = asset.id.clone();
        let provider = location.provider.clone();
        let root = self.root.clone();
        let engine = self.engine.clone();
        let control = Control::default();
        self.preview_jobs.insert(id.clone(), control.clone());
        let requested = id.clone();
        let task = cx.background_executor().spawn(async move {
            let _completion = control.completion();
            (|| -> anyhow::Result<String> {
                let mut command = std::process::Command::new(engine);
                command
                    .arg("--config")
                    .arg(storage::config_path()?)
                    .args([
                        "library",
                        "preview",
                        &requested,
                        "--provider",
                        &provider,
                        "--cache-only",
                    ])
                    .env("IMG_DATA_DIR", root);
                let output = engine::run(command, &control)?;
                anyhow::ensure!(output.success && output.stopped == 0, "preview unavailable");
                let result: serde_json::Value = serde_json::from_slice(&output.stdout)?;
                Ok(result["cache_key"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("invalid preview"))?
                    .to_owned())
            })()
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.preview_jobs.remove(&id);
                match result {
                    Ok(key) => {
                        this.previews.insert(id.clone(), key);
                        this.preview_failures.remove(&id);
                    }
                    Err(_) => {
                        this.preview_failures.insert(id, std::time::Instant::now());
                    }
                }
                this.stamp.clear();
                cx.notify();
            });
        })
        .detach();
    }
    fn command(&mut self, args: Vec<String>, cx: &mut Context<Self>) {
        self.command_file(args, None, cx);
    }
    fn command_file(
        &mut self,
        args: Vec<String>,
        file: Option<tempfile::NamedTempFile>,
        cx: &mut Context<Self>,
    ) {
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
                let _file = file;
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
                if let Some(complete) = result["complete"].as_bool() {
                    return Ok(if complete {
                        format!(
                            "索引完成：{} 张图片",
                            result["seen"].as_array().map(|a| a.len()).unwrap_or(0)
                        )
                    } else {
                        "索引已暂停，可继续扫描".into()
                    });
                }
                if let Some(count) = result["updated"].as_u64() {
                    return Ok(format!("已更新 {count} 项图库记录"));
                }
                if let Some(bytes) = result["removed_bytes"].as_u64() {
                    return Ok(format!(
                        "已清理 {} 本机缓存，图库记录和远端文件保留",
                        model::size_label(bytes)
                    ));
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
    fn delete_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy || self.selected.is_empty() {
            return;
        }
        let ids = self.selected.snapshot();
        let hidden = self.selected.hidden();
        let provider = self.query.provider.clone();
        let root = self.root.clone();
        let task=cx.background_executor().spawn(async move{(||->anyhow::Result<_>{
            let c=Catalog::open(&root)?;let mut assets=ids.iter().map(|id|c.get(id)).collect::<anyhow::Result<Vec<_>>>()?;
            assets.sort_by(|a,b|b.added_at.cmp(&a.added_at).then(a.id.cmp(&b.id)));
            let mut targets=vec![];let mut counts=std::collections::BTreeMap::<String,usize>::new();
            for asset in assets{let location=asset.selected_location(&provider).ok_or_else(||anyhow::anyhow!("所选图片没有对应地址"))?;
                anyhow::ensure!(location.path.is_some()&&!location.version.is_empty(),"部分存储源不支持远端删除，或记录缺少远端版本。请仅选择可管理的图片");
                *counts.entry(location.provider.clone()).or_default()+=1;
                targets.push(serde_json::json!({"input_id":asset.id,"name":asset.name,"location":location}));
            }
            let plan=serde_json::json!({"version":1,"task_id":uuid::Uuid::new_v4().to_string(),"targets":targets});
            Ok((plan,counts))
        })()});
        cx.spawn_in(window,async move |this,cx|{
            let prepared=task.await;
            let Ok((plan,counts))=prepared else {let _=this.update_in(cx,|this,_,cx|{this.notice=Some(prepared.err().unwrap().to_string());cx.notify();});return;};
            let summary=format!("将删除 {} 个远端文件，其中 {hidden} 项不在当前搜索结果中。\n{}\n仅删除列出的远端位置，其他副本、原文件和本机缓存保留。相关链接可能立即失效。",plan["targets"].as_array().map(|a|a.len()).unwrap_or(0),counts.iter().map(|(p,n)|format!("{p}：{n} 个文件")).collect::<Vec<_>>().join("\n"));
            let prompt=this.update_in(cx,|_,window,cx|crate::i18n::prompt(window,PromptLevel::Warning,"删除远端图片？",Some(&summary),&["取消","删除远端文件"],cx));
            let Ok(prompt)=prompt else{return;};if prompt.await.ok()!=Some(1){return;}
            let file=(||->anyhow::Result<_>{use std::io::Write;let mut file=tempfile::NamedTempFile::new()?;file.write_all(&serde_json::to_vec(&plan)?)?;file.as_file().sync_all()?;Ok(file)})();
            let _=this.update_in(cx,|this,_,cx|match file{Ok(file)=>{let args=vec!["library".into(),"delete".into(),"--plan".into(),file.path().to_string_lossy().into_owned()];this.command_file(args,Some(file),cx);},Err(error)=>{this.notice=Some(error.to_string());cx.notify();}});
        }).detach();
    }
    fn read_selected(&mut self, download: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy || self.stopping || self.selected.is_empty() {
            return;
        }
        self.busy = true;
        let ids = self.selected.snapshot().into_iter().collect::<Vec<_>>();
        let provider = self.query.provider.clone();
        let root = self.root.clone();
        let task = cx.background_executor().spawn(async move {
            (|| -> anyhow::Result<_> {
                use std::io::Write;
                let catalog = Catalog::open(&root)?;
                let plan = img_records::targets::TargetPlan::capture(&catalog, &ids, &provider)?;
                let mut file = tempfile::NamedTempFile::new()?;
                file.write_all(&serde_json::to_vec(&plan)?)?;
                file.as_file().sync_all()?;
                Ok(file)
            })()
        });
        cx.spawn_in(window, async move |this, cx| {
            let prepared = task.await;
            let file = match prepared {
                Ok(file) => file,
                Err(error) => {
                    let _ = this.update_in(cx, |this, _, cx| {
                        this.busy = false;
                        this.notice = Some(error.to_string());
                        cx.notify();
                    });
                    return;
                }
            };
            let mut args = vec![
                "library".into(),
                if download { "download" } else { "check" }.into(),
                "--plan".into(),
                file.path().to_string_lossy().into_owned(),
            ];
            if download {
                let prompt = this.update_in(cx, |_, _, cx| {
                    cx.prompt_for_paths(PathPromptOptions {
                        files: false,
                        directories: true,
                        multiple: false,
                        prompt: Some(crate::i18n::text("选择下载目录")),
                    })
                });
                let paths = match prompt {
                    Ok(prompt) => prompt.await.ok().and_then(Result::ok).flatten(),
                    Err(_) => None,
                };
                let Some(path) = paths.and_then(|paths| paths.into_iter().next()) else {
                    let _ = this.update_in(cx, |this, _, cx| {
                        this.busy = false;
                        cx.notify();
                    });
                    return;
                };
                args.extend(["--output-dir".into(), path.to_string_lossy().into_owned()]);
            }
            let _ = this.update_in(cx, |this, _, cx| {
                this.busy = false;
                if !this.stopping {
                    this.command_file(args, Some(file), cx);
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
    fn tools_from(&mut self, ids: Vec<String>, cx: &mut Context<Self>) {
        if self.busy || ids.is_empty() {
            return;
        }
        self.busy = true;
        let root = self.root.clone();
        let engine = self.engine.clone();
        let provider = self.query.provider.clone();
        let control = Control::default();
        self.control = Some(control.clone());
        let task = cx.background_executor().spawn(async move {
            let _completion = control.completion();
            (|| -> anyhow::Result<(Vec<PathBuf>, HashMap<PathBuf, String>)> {
                use std::io::Write;
                let plan = img_records::targets::TargetPlan::capture(
                    &Catalog::open(&root)?,
                    &ids,
                    &provider,
                )?;
                let directory = root
                    .join("tool-inputs")
                    .join(uuid::Uuid::new_v4().to_string());
                std::fs::create_dir_all(&directory)?;
                let mut file = tempfile::NamedTempFile::new()?;
                file.write_all(&serde_json::to_vec(&plan)?)?;
                file.as_file().sync_all()?;
                let mut command = std::process::Command::new(engine);
                command
                    .arg("--config")
                    .arg(storage::config_path()?)
                    .args(["library", "download", "--plan"])
                    .arg(file.path())
                    .arg("--output-dir")
                    .arg(&directory)
                    .env("IMG_DATA_DIR", root);
                let output = engine::run(command, &control.child())?;
                anyhow::ensure!(output.stopped == 0, "操作已取消");
                let result: serde_json::Value = serde_json::from_slice(&output.stdout)?;
                let mut paths = vec![];
                let mut sources = HashMap::new();
                for result in result["files"]
                    .as_array()
                    .ok_or_else(|| anyhow::anyhow!("无法读取图库图片"))?
                {
                    if result["success"] == true
                        && let (Some(path), Some(id)) =
                            (result["output"].as_str(), result["id"].as_str())
                    {
                        let path = PathBuf::from(path);
                        sources.insert(path.clone(), id.to_owned());
                        paths.push(path);
                    }
                }
                anyhow::ensure!(!paths.is_empty(), "没有可读取的图片，请检查来源是否可用");
                Ok((paths, sources))
            })()
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.busy = false;
                this.control = None;
                match result {
                    Ok((paths, sources)) => cx.emit(PreferenceChanged::Tools { paths, sources }),
                    Err(error) => this.notice = Some(error.to_string()),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
    fn copy_to(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy || self.selected.is_empty() {
            return;
        }
        self.busy = true;
        let ids = self.selected.snapshot().into_iter().collect::<Vec<_>>();
        let hidden = self.selected.hidden();
        let root = self.root.clone();
        let provider = self.query.provider.clone();
        let task = cx.background_executor().spawn(async move {
            img_records::targets::TargetPlan::capture(&Catalog::open(&root)?, &ids, &provider)
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.busy = false;
                match result {
                    Ok(targets) => {
                        let parent = cx.weak_entity();
                        let panel = cx.new(|cx| {
                            copy_dialog::CopyDialog::new(parent, targets, hidden, window, cx)
                        });
                        window.open_dialog(cx, move |dialog, window, _| {
                            dialog
                                .title(crate::i18n::text("复制到其他图床"))
                                .w(px((f32::from(window.viewport_size().width) - 80.)
                                    .clamp(280., 660.)))
                                .child(panel.clone())
                        });
                    }
                    Err(error) => this.notice = Some(error.to_string()),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
    fn index_scope(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.query.provider.is_empty() {
            return;
        }
        let parent = cx.weak_entity();
        let provider = self.query.provider.clone();
        let prefix = self.query.prefix.clone();
        let panel = cx.new(|cx| {
            scope_dialog::ScopeDialog::new(parent, self.root.clone(), provider, prefix, window, cx)
        });
        window.open_dialog(cx, move |dialog, window, _| {
            dialog
                .title(crate::i18n::text("索引远端图片"))
                .w(px(
                    (f32::from(window.viewport_size().width) - 80.).clamp(280., 500.)
                ))
                .child(panel.clone())
        });
    }
    fn copy(&mut self, cx: &mut Context<Self>) {
        let ids = self.selected.snapshot();
        let root = self.root.clone();
        let provider = self.query.provider.clone();
        let format = self.format;
        let task = cx.background_executor().spawn(async move {
            (|| -> anyhow::Result<_> {
                let c = Catalog::open(&root)?;
                let mut assets = ids
                    .iter()
                    .filter_map(|id| c.get(id).ok())
                    .collect::<Vec<_>>();
                assets.sort_by(|a, b| b.added_at.cmp(&a.added_at).then(a.id.cmp(&b.id)));
                let lines = assets
                    .iter()
                    .filter_map(|a| {
                        a.selected_location(&provider)
                            .map(|l| format.render(&a.name, &l.url))
                    })
                    .collect::<Vec<_>>();
                Ok((lines.join("\n"), lines.len(), ids.len() - lines.len()))
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
    fn detail(&mut self, asset: Asset, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        self.busy = true;
        let root = self.root.clone();
        let engine = self.engine.clone();
        let original = asset.clone();
        let key = self.previews.get(&asset.id).cloned();
        let provider = self.query.provider.clone();
        let target_provider = provider.clone();
        let control = Control::default();
        self.control = Some(control.clone());
        let task = cx.background_executor().spawn(async move {
            let _completion = control.completion();
            (|| -> anyhow::Result<_> {
                let cache = img_records::cache::Cache::open(&root)?;
                if let Some(key) = key
                    && let Ok(lease) = cache.lease(&key)
                {
                    return Ok((asset, key, std::sync::Arc::new(lease)));
                }
                let mut command = std::process::Command::new(engine);
                command
                    .arg("--config")
                    .arg(storage::config_path()?)
                    .args(["library", "preview", &asset.id, "--provider", &provider])
                    .env("IMG_DATA_DIR", &root);
                let output = engine::run(command, &control)?;
                anyhow::ensure!(
                    output.success && output.stopped == 0,
                    "无法读取所选远端版本，请检查连接或刷新索引"
                );
                let result: serde_json::Value = serde_json::from_slice(&output.stdout)?;
                let key = result["cache_key"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("预览缓存无效"))?
                    .to_string();
                let asset = Catalog::open(&root)?.get(result["id"].as_str().unwrap_or(""))?;
                let lease = std::sync::Arc::new(cache.lease(&key)?);
                Ok((asset, key, lease))
            })()
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.busy = false;
                this.control = None;
                match result {
                    Ok((asset, key, lease)) => {
                        this.previews.insert(asset.id.clone(), key);
                        this.open_detail(asset, &target_provider, Some(lease), window, cx);
                    }
                    Err(error) => {
                        this.notice = Some(error.to_string());
                        this.open_detail(original, &target_provider, None, window, cx);
                    }
                }
                this.stamp.clear();
                this.refresh(cx);
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
    fn open_detail(
        &self,
        asset: Asset,
        provider: &str,
        image: Option<std::sync::Arc<img_records::cache::CacheLease>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let _ = provider;
        let title = asset.name.clone();
        let panel = cx.new(|cx| {
            detail::Detail::new(
                asset,
                self.root.clone(),
                self.format,
                self.thumbnails.clone(),
                image,
                cx,
            )
        });
        let parent = cx.weak_entity();
        let asset_id = panel.read(cx).asset_id();
        window.open_dialog(cx, move |dialog, window, _| {
            let parent = parent.clone();
            let asset_id = asset_id.clone();
            let viewport = window.viewport_size();
            let width = (f32::from(viewport.width) * 0.85).clamp(240., 960.);
            dialog
                .w(px(width))
                .margin_top(px(24.))
                .title(crate::i18n::text(title.clone()))
                .child(panel.clone())
                .footer(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(action("detail-tools", "在图片工具中打开").on_click(
                            move |_, window, cx| {
                                window.close_dialog(cx);
                                let _ = parent.update(cx, |this, cx| {
                                    this.tools_from(vec![asset_id.clone()], cx)
                                });
                            },
                        ))
                        .child(div().flex_1())
                        .child(
                            action("detail-close", "关闭预览")
                                .on_click(|_, window, cx| window.close_dialog(cx)),
                        ),
                )
        });
    }
    fn copy_link(&mut self, id: &str, name: &str, url: &str, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(self.format.render(name, url)));
        let feedback = (id.to_owned(), std::time::Instant::now());
        self.copied = Some(feedback.clone());
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_secs(2)).await;
            let _ = this.update(cx, |this, cx| {
                if this.copied.as_ref() == Some(&feedback) {
                    this.copied = None;
                    cx.notify();
                }
            });
        })
        .detach();
        cx.notify();
    }
    fn cell(&self, asset: Asset, cx: &mut Context<Self>) -> AnyElement {
        let id = asset.id.clone();
        let copied = self
            .copied
            .as_ref()
            .is_some_and(|(copied, _)| copied == &id);
        let copy_id = id.clone();
        let target = asset.clone();
        let hash = self
            .previews
            .get(&asset.id)
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
                d.child(
                    img(path)
                        .image_cache(&self.thumbnails)
                        .size_full()
                        .object_fit(ObjectFit::Contain),
                )
            });
        if !self.grid {
            let location = asset.selected_location(&self.query.provider);
            let copy_url = location.map(|location| location.url.clone());
            let copy_name = asset.name.clone();
            let detail = location
                .map(|location| location.provider.clone())
                .unwrap_or_else(|| "没有可用地址".into());
            let name = asset.name.clone();
            return div()
                .id(SharedString::from(format!("catalog-row-{id}")))
                .w_full()
                .h(px(76.))
                .flex()
                .items_center()
                .gap(px(12.))
                .p(px(10.))
                .rounded(px(8.))
                .border_1()
                .border_color(crate::theme::color(BORDER))
                .bg(crate::theme::color(CARD))
                .when(self.selecting, |row| {
                    row.child(
                        gpui_kit::component::checkbox::Checkbox::new(SharedString::from(format!(
                            "select-{id}"
                        )))
                        .accessibility_label(crate::i18n::text(format!("选择 {name}")))
                        .checked(self.selected.contains(&id))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.selected.toggle(id.clone());
                            cx.notify();
                        })),
                    )
                })
                .child(
                    Button::new(SharedString::from(format!("preview-{}", asset.id)))
                        .ghost()
                        .accessibility_label(crate::i18n::text(format!("预览 {name}")))
                        .w(px(62.))
                        .h(px(56.))
                        .child(preview)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.detail(target.clone(), window, cx)
                        })),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(6.))
                        .child(label(name, 13., TEXT).text_ellipsis())
                        .child(label(detail, 11., MUTED).text_ellipsis()),
                )
                .child(label(model::size_label(asset.size), 12., MUTED))
                .child(label(
                    format!("{} 个地址", asset.locations.len()),
                    11.,
                    MUTED,
                ))
                .child(
                    action(
                        SharedString::from(format!("copy-{}", asset.id)),
                        if copied { "" } else { "复制链接" },
                    )
                    .when(copied, |button| button.icon(IconName::Check))
                    .accessibility_label(crate::i18n::text(if copied {
                        "已复制"
                    } else {
                        "复制链接"
                    }))
                    .w(px(84.))
                    .h(px(28.))
                    .px(px(10.))
                    .flex_shrink_0()
                    .disabled(copy_url.as_ref().is_none_or(|url| url.is_empty()))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(url) = &copy_url {
                            this.copy_link(&copy_id, &copy_name, url, cx);
                        }
                    })),
                )
                .into_any_element();
        }
        let copy_url = asset
            .selected_location(&self.query.provider)
            .map(|location| location.url.clone());
        let copy_name = asset.name.clone();
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
                    .accessibility_label(crate::i18n::text(format!("预览 {}", asset.name)))
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
                            .accessibility_label(crate::i18n::text(format!("选择 {}", asset.name)))
                            .checked(self.selected.contains(&id))
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    this.selected.toggle(id.clone());
                                    cx.notify();
                                },
                            )),
                        )
                    })
                    .child(
                        label(asset.name, 12., TEXT)
                            .flex_1()
                            .min_w_0()
                            .text_ellipsis(),
                    )
                    .child(
                        Button::new(SharedString::from(format!("copy-{}", asset.id)))
                            .ghost()
                            .icon(if copied {
                                IconName::Check
                            } else {
                                IconName::Copy
                            })
                            .accessibility_label(crate::i18n::text(if copied {
                                "已复制"
                            } else {
                                "复制链接"
                            }))
                            .tooltip(crate::i18n::text(if copied {
                                "已复制"
                            } else {
                                "复制链接"
                            }))
                            .w(px(28.))
                            .h(px(28.))
                            .flex_shrink_0()
                            .disabled(copy_url.as_ref().is_none_or(|url| url.is_empty()))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if let Some(url) = &copy_url {
                                    this.copy_link(&copy_id, &copy_name, url, cx);
                                }
                            })),
                    ),
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
        let mut view = div()
            .flex()
            .flex_col()
            .size_full()
            .p(px(22.))
            .pb(px(10.))
            .gap(px(12.));
        let weak = cx.entity().downgrade();
        let mut top = div()
            .flex()
            .items_center()
            .gap(px(8.))
            .child(label("图库", 20., TEXT))
            .child(label(format!("{} 项 · 已索引范围", self.total), 11., MUTED))
            .child(div().flex_1());
        top = top
            .child(self.filter_menu(cx))
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
                    let can_index = self.manageable.contains(&self.query.provider);
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
                        if can_index {
                            let start = weak.clone();
                            menu = menu.separator().item(
                                PopupMenuItem::new(crate::i18n::text("索引当前图床")).on_click(
                                    move |_, window, cx| {
                                        let _ = start
                                            .update(cx, |this, cx| this.index_scope(window, cx));
                                    },
                                ),
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
                        cx.emit(PreferenceChanged::View(this.grid));
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
        view = view.child(top);
        if self.selecting {
            let hidden = self.selected.hidden();
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
                            .flex_wrap()
                            .child(self.format_picker(cx))
                            .child(
                                action("catalog-all", "全选当前结果")
                                    .disabled(self.busy || self.matching.is_empty())
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.selected.select_visible();
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
                                action("catalog-more", "更多操作")
                                    .disabled(self.selected.is_empty() || self.busy)
                                    .dropdown_menu({
                                        let weak = cx.weak_entity();
                                        move |menu, _, _| {
                                            let download = weak.clone();
                                            let tools = weak.clone();
                                            let check = weak.clone();
                                            let copy = weak.clone();
                                            let hide = weak.clone();
                                            let delete = weak.clone();
                                            menu.item(
                                                PopupMenuItem::new(crate::i18n::text(
                                                    "在图片工具中打开",
                                                ))
                                                .on_click(move |_, _, cx| {
                                                    let _ = tools.update(cx, |this, cx| {
                                                        this.tools_from(
                                                            this.selected
                                                                .snapshot()
                                                                .into_iter()
                                                                .collect(),
                                                            cx,
                                                        )
                                                    });
                                                }),
                                            )
                                            .item(
                                                PopupMenuItem::new(crate::i18n::text("下载所选"))
                                                    .on_click(move |_, window, cx| {
                                                        let _ = download.update(cx, |this, cx| {
                                                            this.read_selected(true, window, cx)
                                                        });
                                                    }),
                                            )
                                            .item(
                                                PopupMenuItem::new(crate::i18n::text("检查链接"))
                                                    .on_click(move |_, window, cx| {
                                                        let _ = check.update(cx, |this, cx| {
                                                            this.read_selected(false, window, cx)
                                                        });
                                                    }),
                                            )
                                            .item(
                                                PopupMenuItem::new(crate::i18n::text(
                                                    "复制到其他图床",
                                                ))
                                                .on_click(move |_, window, cx| {
                                                    let _ = copy.update(cx, |this, cx| {
                                                        this.copy_to(window, cx)
                                                    });
                                                }),
                                            )
                                            .separator()
                                            .item(
                                                PopupMenuItem::new(crate::i18n::text("隐藏所选"))
                                                    .on_click(move |_, _, cx| {
                                                        let _ = hide.update(cx, |this, cx| {
                                                            let mut args = vec![
                                                                "library".into(),
                                                                "hide".into(),
                                                            ];
                                                            args.extend(
                                                                this.selected.iter().cloned(),
                                                            );
                                                            this.command(args, cx);
                                                        });
                                                    }),
                                            )
                                            .item(
                                                PopupMenuItem::new(crate::i18n::text(
                                                    "删除远端文件",
                                                ))
                                                .on_click(move |_, window, cx| {
                                                    let _ = delete.update(cx, |this, cx| {
                                                        this.delete_selected(window, cx)
                                                    });
                                                }),
                                            )
                                        }
                                    }),
                            ),
                    ),
            );
        }
        if let Some(notice) = &self.notice {
            view = view.child(label(notice.clone(), 12., MUTED));
        }
        if self.assets.is_empty() && !self.loading {
            view = view.child(label(
                if self.query.text.is_empty() && self.total == 0 {
                    "当前范围没有图片。选择存储源和目录后可开始索引，也可以先上传图片。"
                } else {
                    "没有匹配结果，已选图片会继续保留。"
                },
                13.,
                MUTED,
            ));
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
                                .w_full()
                                .flex()
                                .gap(px(10.))
                                .pb(px(10.))
                                .h(px(if this.grid { 230. } else { 86. }));
                            for col in 0..columns {
                                let asset = this.assets.get(row * columns + col).cloned();
                                if let Some(asset) = &asset {
                                    this.load_visible_preview(asset, cx);
                                }
                                line = line.child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .children(asset.map(|asset| this.cell(asset, cx))),
                                );
                            }
                            line
                        })
                        .collect()
                }),
            )
            .track_scroll(&self.scroll)
            .w_full()
            .flex_1()
            .min_h_0(),
        );
        view.child(
            div()
                .flex()
                .w_full()
                .h(px(28.))
                .items_center()
                .flex_shrink_0()
                .child(
                    action("catalog-prev", "上一页")
                        .w(px(80.))
                        .h(px(28.))
                        .px(px(10.))
                        .rounded(px(6.))
                        .disabled(self.query.offset == 0 || self.busy)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.query.offset = this.query.offset.saturating_sub(200);
                            this.stamp.clear();
                            this.generation += 1;
                            this.refresh(cx);
                        })),
                )
                .child(
                    div()
                        .flex_1()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(label(
                            format!(
                                "{} / {}",
                                self.query.offset / 200 + 1,
                                self.total.div_ceil(200).max(1)
                            ),
                            12.,
                            MUTED,
                        )),
                )
                .child(
                    action("catalog-next", "下一页")
                        .w(px(80.))
                        .h(px(28.))
                        .px(px(10.))
                        .rounded(px(6.))
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
