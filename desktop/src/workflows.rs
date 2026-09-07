use super::*;
use std::io::Write;

impl ImgDesktop {
    pub(crate) fn show_restore_result(&mut self, cx: &mut Context<Self>) {
        let path = self.root.join("restore-result.json");
        if let Ok(bytes) = std::fs::read(&path)
            && let Ok((message, error)) = serde_json::from_slice::<(String, bool)>(&bytes)
        {
            self.message(message, error, cx);
            let _ = std::fs::remove_file(path);
        }
    }
    fn choose_backup(&mut self, restore: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.workflow_busy || self.shutting_down {
            return;
        }
        self.workflow_busy = true;
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some(crate::i18n::text(if restore {
                "选择备份目录"
            } else {
                "选择备份保存位置"
            })),
        });
        cx.spawn_in(window,async move |this,cx| {
            let path=match paths.await {Ok(Ok(Some(paths)))=>paths.into_iter().next(),_=>None};
            let Some(path)=path else {let _=this.update(cx,|this,cx|{this.workflow_busy=false;cx.notify();});return;};
            let inspected=if restore {
                let source=path.clone();
                cx.background_executor().spawn(async move{img_records::backup::inspect(&source).map(|m|m.files.len())}).await
            }else{Ok(0)};
            let prompt=this.update_in(cx,|this,window,cx|match inspected {
                Err(e)=>{this.workflow_busy=false;this.message(e.to_string(),true,cx);None},
                Ok(count)=>Some(crate::i18n::prompt(window, PromptLevel::Info,
                    if restore{"恢复备份并重启？"}else{"选择备份内容"},
                    Some(&if restore{format!("已校验 {count} 个文件。应用会安全退出，替换备份中的设置与记录，并保留恢复前的数据副本。远端图片与原文件不受影响。凭据可单独选择。")}else{"默认包含设置和记录。可加入图片缓存；包含凭据时备份不加密，请存放在私人目录。".into()}),
                    if restore{&["取消","恢复，不导入凭据","恢复并导入凭据"][..]}else{&["取消","设置与记录","包含缓存","包含缓存与凭据"][..]},cx))
            }).ok().flatten();
            let Some(prompt)=prompt else{return;};
            let choice=prompt.await.unwrap_or(0);
            if choice==0 {let _=this.update(cx,|this,cx|{this.workflow_busy=false;cx.notify();});return;}
            if restore {
                let _=this.update(cx,|this,cx|{
                    this.workflow_busy=false;
                    match crate::backup::schedule(&this.root,&path,choice==2) {
                        Ok(())=>this.begin_shutdown(cx),
                        Err(e)=>this.message(e.to_string(),true,cx),
                    }
                });
            }else{
                let settings=this.update(cx,|this,_|(this.root.clone(),crate::storage::config_path())).ok();
                if let Some((root,config))=settings {
                    let result=cx.background_executor().spawn(async move{
                        let destination=path.join(format!("img-backup-{}",uuid::Uuid::new_v4()));
                        img_records::backup::export(&root,&config?,&destination,img_records::backup::Options{config:true,records:true,cache:choice>=2,credentials:choice==3})?;
                        Ok::<_,anyhow::Error>(destination)
                    }).await;
                    let _=this.update(cx,|this,cx|{this.workflow_busy=false;match result{
                        Ok(path)=>this.message(format!("备份已保存到 {}",path.display()),false,cx),
                        Err(e)=>this.message(format!("备份未完成：{e}"),true,cx),
                    }});
                }
            }
        }).detach();
    }
    pub(super) fn workflow_controls(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(label("文章与目录", 16., TEXT))
            .child(label(
                "转存文章前预览引用并保留原文备份。目录监听会自动上传新建或修改的图片。",
                12.,
                MUTED,
            ))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(10.))
                    .child(
                        action("rewrite-document", "转存 Markdown 文章")
                            .disabled(self.workflow_busy)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.choose_workflow(false, window, cx)
                            })),
                    )
                    .child(
                        action(
                            "watch-directory",
                            if self.watch_control.is_some() {
                                "停止目录监听"
                            } else {
                                "选择目录并监听"
                            },
                        )
                        .disabled(self.workflow_busy)
                        .on_click(cx.listener(|this, _, window, cx| {
                            if let Some(control) = &this.watch_control {
                                control.stop(engine::CANCEL);
                                cx.notify();
                            } else {
                                this.choose_workflow(true, window, cx);
                            }
                        })),
                    ),
            )
            .child(
                div()
                    .flex()
                    .gap(px(10.))
                    .child(
                        action("backup-data", "备份设置与图库")
                            .disabled(self.workflow_busy)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.choose_backup(false, window, cx)
                            })),
                    )
                    .child(
                        action("restore-data", "恢复备份")
                            .disabled(self.workflow_busy)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.choose_backup(true, window, cx)
                            })),
                    ),
            )
            .into_any_element()
    }
    fn choose_workflow(&mut self, watch: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.workflow_busy || self.shutting_down {
            return;
        }
        if self.provider.is_empty() {
            self.message("请先配置默认存储源", true, cx);
            return;
        }
        self.workflow_busy = true;
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: !watch,
            directories: watch,
            multiple: false,
            prompt: Some(crate::i18n::text(if watch {
                "选择自动上传目录"
            } else {
                "选择 Markdown 文件"
            })),
        });
        let binary = self.engine.clone();
        cx.spawn_in(window, async move |this,cx| {
            let path = match paths.await {
                Ok(Ok(Some(paths))) => paths.into_iter().next(),
                _ => None,
            };
            let Some(path) = path else {
                let _ = this.update(cx,|this,cx| {this.workflow_busy=false;cx.notify();});return;
            };
            let preview_path = path.clone();
            let preview = cx.background_executor().spawn(async move {
                if watch {return Ok((String::from("会上传此目录及子目录中的现有、新建和修改后的图片。关闭应用后停止。"),None));}
                anyhow::ensure!(preview_path.extension().and_then(|v|v.to_str()).is_some_and(|v|matches!(v.to_ascii_lowercase().as_str(),"md"|"markdown")),"请选择 Markdown 文件");
                let before = std::fs::read(&preview_path)?;
                let mut command = std::process::Command::new(binary);
                command.arg("rewrite").arg(&preview_path).arg("--dry-run");
                let out = engine::run(command,&Control::default())?;
                anyhow::ensure!(out.success,"无法预览文章");
                let rows: Vec<serde_json::Value> = serde_json::from_slice(&out.stdout)?;
                let refs = rows.first().and_then(|r|r["references"].as_array()).ok_or_else(||anyhow::anyhow!("无法读取预览"))?;
                let missing = refs.iter().filter(|r|r["exists"] == false).count();
                let summary = format!("找到 {} 个图片引用，其中 {} 个本地文件缺失。成功项会替换链接，失败项保留；原文保存为同目录备份。\n{}",refs.len(),missing,
                    refs.iter().take(12).filter_map(|r|r["source"].as_str()).collect::<Vec<_>>().join("\n"));
                Ok::<_,anyhow::Error>((summary,Some(before)))
            }).await;
            let prompt = this.update_in(cx,|this,window,cx| {
                match preview {
                    Ok((summary,before)) => Some((crate::i18n::prompt(window, PromptLevel::Info,
                        if watch {"开始监听此目录？"} else {"转存文章图片？"},Some(&summary), &["取消","开始"],cx),before)),
                    Err(error) => {this.workflow_busy=false;this.message(error.to_string(),true,cx);None}
                }
            }).ok().flatten();
            if let Some((prompt,before)) = prompt {
                let accepted = prompt.await.ok() == Some(1);
                let _ = this.update(cx,|this,cx| {
                    this.workflow_busy=false;
                    if accepted {this.run_workflow(path,watch,before,cx);} else {cx.notify();}
                });
            }
        }).detach();
    }
    fn run_workflow(
        &mut self,
        path: PathBuf,
        watch: bool,
        before: Option<Vec<u8>>,
        cx: &mut Context<Self>,
    ) {
        let target = self.provider.clone();
        let binary = self.engine.clone();
        let root = self.root.clone();
        let options = self.upload_options.clone();
        let control = Control::default();
        self.queue.auxiliary.push(control.clone());
        if watch {
            self.watch_control = Some(control.clone());
        } else {
            self.workflow_busy = true;
        }
        self.message(
            if watch {
                "目录监听已启动"
            } else {
                "正在转存文章图片…"
            },
            false,
            cx,
        );
        let task = cx.background_executor().spawn(async move {
            if let Some(before) = before {
                anyhow::ensure!(
                    std::fs::read(&path)? == before,
                    "文章在预览后发生变化，请重新预览"
                );
            }
            let snapshot = model::UploadConfiguration::capture(&target)?;
            let temporary = tempfile::tempdir_in(&root)?;
            let mut config = tempfile::NamedTempFile::new_in(temporary.path())?;
            config.write_all(options.engine_config(&snapshot.config)?.as_bytes())?;
            let mut command = std::process::Command::new(binary);
            command
                .current_dir(temporary.path())
                .envs(snapshot.environment)
                .env("IMG_DATA_DIR", &root)
                .env("IMG_DESKTOP_UPLOAD", "0")
                .env_remove("IMG_PROVIDER")
                .env_remove("IMG_DEFAULT_PROVIDER")
                .arg("--config")
                .arg(config.path())
                .arg(if watch { "watch" } else { "rewrite" })
                .arg(&path);
            let report = if watch {
                temporary.path().join("unused.json")
            } else {
                path.with_file_name(format!(
                    "{}.img-report-{}.json",
                    path.file_name().unwrap().to_string_lossy(),
                    uuid::Uuid::new_v4()
                ))
            };
            if !watch {
                command.arg("--report").arg(&report);
            }
            if options.optimize {
                command.arg("--optimize");
            }
            let out = engine::run(command, &control)?;
            if watch {
                return Ok((
                    "目录监听已停止".to_string(),
                    !out.success && out.stopped == 0,
                ));
            }
            let rows: Vec<serde_json::Value> = serde_json::from_slice(&std::fs::read(report)?)?;
            let ok = rows.iter().filter(|r| r["success"] == true).count();
            Ok::<_, anyhow::Error>((
                format!(
                    "文章转存：{} 项成功，{} 项失败；结果报告与原文备份保留在文章目录。",
                    ok,
                    rows.len() - ok
                ),
                !out.success,
            ))
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                if watch {
                    this.watch_control = None;
                } else {
                    this.workflow_busy = false;
                }
                match result {
                    Ok((message, error)) => this.message(message, error, cx),
                    Err(_) => {
                        this.message("操作未完成，请检查配置、文件权限与存储连接。", true, cx)
                    }
                }
            });
        })
        .detach();
    }
}
