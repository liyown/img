use crate::{
    assets::icon,
    storage::{self, ExtraField, ProviderDraft, ProviderKind},
    theme::*,
};
use gpui_kit::{
    component::{
        button::*,
        input::{Input, InputState},
        menu::{DropdownMenu, PopupMenuItem},
        switch::Switch,
        *,
    },
    prelude::*,
    *,
};
use std::collections::BTreeMap;

pub struct StorageChanged;
pub struct EditorClosed;
pub struct StorageSettings {
    engine: std::path::PathBuf,
    pub uploading: bool,
    providers: Vec<(String, String)>,
    default_provider: String,
    editor: Option<ProviderEditor>,
    saving: bool,
    notice: Option<(String, bool)>,
    import_plan: Option<img_records::migration::Plan>,
}
struct ProviderEditor {
    draft: ProviderDraft,
    name: Entity<InputState>,
    fields: BTreeMap<String, Entity<InputState>>,
    headers: Vec<ExtraEditor>,
    extras: Vec<ExtraEditor>,
}
struct ExtraEditor {
    key: Entity<InputState>,
    value: Entity<InputState>,
    original_key: Option<String>,
}
fn extra_editor(
    row: &ExtraField,
    window: &mut Window,
    cx: &mut Context<StorageSettings>,
) -> ExtraEditor {
    ExtraEditor {
        key: cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("名称")
                .default_value(row.key.clone())
        }),
        value: cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(if row.original_key.is_some() {
                    "留空保留已保存的值"
                } else {
                    "值"
                })
                .default_value(row.value.clone())
                .masked(true)
        }),
        original_key: row.original_key.clone(),
    }
}
impl EventEmitter<StorageChanged> for StorageSettings {}
impl EventEmitter<EditorClosed> for StorageSettings {}

fn text(value: impl Into<SharedString>, size: f32, color: u32) -> Div {
    div()
        .text_size(px(size))
        .text_color(crate::theme::color(color))
        .child(value.into())
}
fn button(id: impl Into<ElementId>, title: &str) -> Button {
    Button::new(id)
        .label(title.to_owned())
        .small()
        .h(px(34.))
        .rounded(px(9.))
        .text_size(px(12.))
}

impl StorageSettings {
    pub fn new(engine: std::path::PathBuf) -> Self {
        let result = storage::configured_providers();
        let notice = result.as_ref().err().map(|e| (e.to_string(), true));
        let (providers, default_provider) = result.unwrap_or_default();
        Self {
            engine,
            uploading: false,
            providers,
            default_provider,
            editor: None,
            saving: false,
            notice,
            import_plan: None,
        }
    }
    fn choose_import(&mut self, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("选择 PicGo / PicList 配置 JSON".into()),
        });
        let existing = self
            .providers
            .iter()
            .map(|(name, _)| name.clone())
            .collect::<Vec<_>>();
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = paths.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let task = cx.background_executor().spawn(async move {
                img_records::migration::parse(&std::fs::read(path)?, &existing)
            });
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(plan) => {
                        this.notice = Some((
                            format!(
                                "找到 {} 项配置，{} 项无法导入。逐项检查后保存，原文件不会修改。",
                                plan.candidates.len(),
                                plan.skipped.len()
                            ),
                            false,
                        ));
                        this.import_plan = Some(plan);
                    }
                    Err(_) => {
                        this.notice = Some((
                            "无法读取配置，请选择有效的 PicGo / PicList JSON 文件。原文件已保留。"
                                .into(),
                            true,
                        ))
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }
    fn edit(&mut self, draft: ProviderDraft, window: &mut Window, cx: &mut Context<Self>) {
        let name = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("例如：个人图床")
                .default_value(draft.name.clone())
        });
        let fields = draft
            .kind
            .fields()
            .into_iter()
            .map(|field| {
                let value = draft.values.get(field.key).cloned().unwrap_or_default();
                let placeholder = if field.secret && draft.original_name.is_some() {
                    "留空保留已保存的凭据"
                } else {
                    field.placeholder
                };
                (
                    field.key.into(),
                    cx.new(|cx| {
                        InputState::new(window, cx)
                            .placeholder(placeholder)
                            .default_value(value)
                            .masked(field.secret)
                    }),
                )
            })
            .collect();
        let headers = draft
            .headers
            .iter()
            .map(|row| extra_editor(row, window, cx))
            .collect();
        let extras = draft
            .fields
            .iter()
            .map(|row| extra_editor(row, window, cx))
            .collect();
        self.editor = Some(ProviderEditor {
            headers,
            extras,
            draft,
            name,
            fields,
        });
        self.notice = None;
        cx.notify();
    }
    fn refresh(&mut self, cx: &mut Context<Self>) {
        match storage::configured_providers() {
            Ok((providers, default_provider)) => {
                self.providers = providers;
                self.default_provider = default_provider;
                cx.emit(StorageChanged);
            }
            Err(e) => self.notice = Some((e.to_string(), true)),
        }
        cx.notify();
    }
    fn save(&mut self, cx: &mut Context<Self>) {
        if self.saving || self.uploading {
            return;
        }
        let Some(editor) = &self.editor else {
            return;
        };
        let mut draft = editor.draft.clone();
        draft.name = editor.name.read(cx).value().trim().to_owned();
        draft.values = editor
            .fields
            .iter()
            .map(|(key, state)| (key.clone(), state.read(cx).value().to_string()))
            .collect();
        let read_rows = |rows: &[ExtraEditor]| {
            rows.iter()
                .map(|row| ExtraField {
                    key: row.key.read(cx).value().to_string(),
                    value: row.value.read(cx).value().to_string(),
                    original_key: row.original_key.clone(),
                })
                .collect()
        };
        draft.headers = read_rows(&editor.headers);
        draft.fields = read_rows(&editor.extras);
        self.saving = true;
        self.notice = None;
        let task = cx.background_executor().spawn(async move {
            storage::save_provider(
                &storage::config_path()?,
                &draft,
                &storage::SystemCredentials,
            )
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.saving = false;
                match result {
                    Ok(()) => {
                        this.editor = None;
                        cx.emit(EditorClosed);
                        this.notice = Some(("存储源已保存，可以开始上传".into(), false));
                        this.refresh(cx);
                    }
                    Err(e) => this.notice = Some((e.to_string(), true)),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
    fn test(&mut self, name: String, cx: &mut Context<Self>) {
        if self.saving || self.uploading {
            return;
        }
        self.saving = true;
        self.notice = Some(("正在测试连接…".into(), false));
        let engine = self.engine.clone();
        let task = cx
            .background_executor()
            .spawn(async move { storage::test_provider(&name, &engine) });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.saving = false;
                this.notice = Some(match result {
                    Ok(()) => (
                        "连接可用。此测试检查接口或存储访问，不会上传图片。".into(),
                        false,
                    ),
                    Err(e) => (e.to_string(), true),
                });
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
    fn remove(&mut self, name: String, window: &mut Window, cx: &mut Context<Self>) {
        if self.saving || self.uploading {
            return;
        }
        let prompt = window.prompt(
            PromptLevel::Warning,
            &format!("删除存储源“{name}”？"),
            Some("移除本机配置和不再使用的凭据。远端图片与本地历史记录会保留。"),
            &["取消", "删除"],
            cx,
        );
        cx.spawn(async move |this, cx| {
            if prompt.await.ok() != Some(1) {
                return;
            }
            let _ = this.update(cx, |this, cx| {
                if this.uploading {
                    return;
                }
                this.saving = true;
                let task = cx.background_executor().spawn(async move {
                    storage::remove_provider(
                        &storage::config_path()?,
                        &name,
                        &storage::SystemCredentials,
                    )
                });
                cx.spawn(async move |this, cx| {
                    let result = task.await;
                    let _ = this.update(cx, |this, cx| {
                        this.saving = false;
                        match result {
                            Ok(()) => {
                                this.notice = Some(("存储源已删除".into(), false));
                                this.refresh(cx);
                            }
                            Err(e) => this.notice = Some((e.to_string(), true)),
                        }
                        cx.notify();
                    });
                })
                .detach();
                cx.notify();
            });
        })
        .detach();
    }
    fn extra_rows(
        &self,
        editor: &ProviderEditor,
        group: &'static str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let rows = if group == "headers" {
            &editor.headers
        } else {
            &editor.extras
        };
        let title = if group == "headers" {
            "额外请求头"
        } else {
            "额外表单字段"
        };
        let mut section = div().flex().flex_col().gap(px(8.)).child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(text(title, 12., NAV_TEXT))
                .child(
                    button(SharedString::from(format!("add-{group}")), "添加一项")
                        .ghost()
                        .disabled(self.saving || self.uploading)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            let row = extra_editor(&ExtraField::default(), window, cx);
                            if let Some(editor) = &mut this.editor {
                                if group == "headers" {
                                    editor.headers.push(row);
                                } else {
                                    editor.extras.push(row);
                                }
                            }
                            cx.notify();
                        })),
                ),
        );
        for (index, row) in rows.iter().enumerate() {
            section = section.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        div().flex_1().min_w(px(0.)).child(
                            Input::new(&row.key)
                                .aria_label(format!("{title}名称 {}", index + 1))
                                .disabled(self.saving || self.uploading)
                                .h(px(34.)),
                        ),
                    )
                    .child(
                        div().flex_1().min_w(px(0.)).child(
                            Input::new(&row.value)
                                .aria_label(format!("{title}值 {}", index + 1))
                                .disabled(self.saving || self.uploading)
                                .h(px(34.)),
                        ),
                    )
                    .child(
                        button(
                            SharedString::from(format!("remove-{group}-{index}")),
                            "移除",
                        )
                        .ghost()
                        .disabled(self.saving || self.uploading)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if let Some(editor) = &mut this.editor {
                                if group == "headers" {
                                    editor.headers.remove(index);
                                } else {
                                    editor.extras.remove(index);
                                }
                            }
                            cx.notify();
                        })),
                    ),
            );
        }
        section.into_any_element()
    }
    fn editor(&self, editor: &ProviderEditor, cx: &mut Context<Self>) -> AnyElement {
        let kind = editor.draft.kind;
        let weak = cx.entity().downgrade();
        let editing = editor.draft.original_name.is_some();
        let mut grid = div().grid().grid_cols(2).gap(px(14.));
        for field in kind.fields() {
            let state = &editor.fields[field.key];
            grid = grid.child(
                div()
                    .min_w(px(0.))
                    .flex()
                    .flex_col()
                    .gap(px(7.))
                    .child(text(
                        format!("{}{}", field.label, if field.required { " *" } else { "" }),
                        12.,
                        NAV_TEXT,
                    ))
                    .child(
                        Input::new(state)
                            .aria_label(field.label)
                            .disabled(self.saving || self.uploading)
                            .h(px(36.))
                            .text_size(px(12.))
                            .bg(crate::theme::color(CANVAS)),
                    ),
            );
        }
        div()
            .flex()
            .flex_col()
            .gap(px(16.))
            .p(px(18.))
            .bg(crate::theme::color(DROP))
            .rounded(px(12.))
            .child(
                text(
                    if editing {
                        "编辑存储源"
                    } else {
                        "添加存储源"
                    },
                    14.,
                    TEXT,
                )
                .font_weight(FontWeight::SEMIBOLD),
            )
            .child(
                div()
                    .grid()
                    .grid_cols(2)
                    .gap(px(14.))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(7.))
                            .child(text("存储源名称 *", 12., NAV_TEXT))
                            .child(
                                Input::new(&editor.name)
                                    .aria_label("存储源名称")
                                    .disabled(self.saving || self.uploading || editing)
                                    .h(px(36.))
                                    .text_size(px(12.))
                                    .bg(crate::theme::color(CANVAS)),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(7.))
                            .child(text("存储服务", 12., NAV_TEXT))
                            .child(
                                button("storage-kind", kind.label())
                                    .disabled(self.saving || self.uploading || editing)
                                    .w_full()
                                    .child(
                                        icon("caret-down", 12.)
                                            .text_color(crate::theme::color(TEXT)),
                                    )
                                    .dropdown_menu(move |mut menu, _, _| {
                                        for next in ProviderKind::ALL {
                                            let weak = weak.clone();
                                            menu = menu.item(
                                                PopupMenuItem::new(next.label())
                                                    .checked(next == kind)
                                                    .on_click(move |_, window, cx| {
                                                        let _ = weak.update(cx, |this, cx| {
                                                            let name = this
                                                                .editor
                                                                .as_ref()
                                                                .map(|e| {
                                                                    e.name
                                                                        .read(cx)
                                                                        .value()
                                                                        .to_string()
                                                                })
                                                                .unwrap_or_default();
                                                            let mut draft =
                                                                ProviderDraft::new(next);
                                                            draft.name = name;
                                                            this.edit(draft, window, cx);
                                                        });
                                                    }),
                                            );
                                        }
                                        menu
                                    }),
                            ),
                    ),
            )
            .child(grid)
            .when(kind == ProviderKind::Http, |this| {
                let method = editor.draft.method.clone();
                let weak = cx.entity().downgrade();
                this.child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(text("请求方式", 12., NAV_TEXT))
                        .child(
                            button("http-method", &method)
                                .disabled(self.saving || self.uploading)
                                .dropdown_menu(move |mut menu, _, _| {
                                    for next in ["POST", "PUT", "PATCH"] {
                                        let weak = weak.clone();
                                        menu = menu.item(
                                            PopupMenuItem::new(next)
                                                .checked(next == method)
                                                .on_click(move |_, _, cx| {
                                                    let _ = weak.update(cx, |this, cx| {
                                                        if let Some(editor) = &mut this.editor {
                                                            editor.draft.method = next.into();
                                                        }
                                                        cx.notify();
                                                    });
                                                }),
                                        );
                                    }
                                    menu
                                }),
                        ),
                )
                .child(self.extra_rows(editor, "headers", cx))
                .child(self.extra_rows(editor, "fields", cx))
            })
            .child(
                Switch::new("allow-insecure-storage")
                    .label("允许 HTTP 服务地址（仅用于可信服务）")
                    .checked(editor.draft.allow_insecure)
                    .disabled(self.saving || self.uploading)
                    .on_click(cx.listener(|this, checked: &bool, _, cx| {
                        if let Some(editor) = &mut this.editor {
                            editor.draft.allow_insecure = *checked;
                        }
                        cx.notify();
                    })),
            )
            .when(
                matches!(
                    kind,
                    ProviderKind::S3 | ProviderKind::R2 | ProviderKind::Oss
                ),
                |this| {
                    this.child(
                        Switch::new("path-style")
                            .label("使用路径形式访问存储桶")
                            .checked(editor.draft.path_style)
                            .disabled(self.saving || self.uploading)
                            .on_click(cx.listener(|this, checked: &bool, _, cx| {
                                if let Some(editor) = &mut this.editor {
                                    editor.draft.path_style = *checked;
                                }
                                cx.notify();
                            })),
                    )
                },
            )
            .child(text(
                "凭据安全保存在系统钥匙串中。已有存储源的凭据留空即可保留。",
                11.,
                MUTED,
            ))
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap(px(8.))
                    .child(
                        button("cancel-storage", "取消")
                            .ghost()
                            .disabled(self.saving || self.uploading)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.editor = None;
                                cx.emit(EditorClosed);
                                this.notice = None;
                                cx.notify();
                            })),
                    )
                    .child(
                        button(
                            "save-storage",
                            if self.saving {
                                "正在保存…"
                            } else {
                                "保存存储源"
                            },
                        )
                        .primary()
                        .disabled(self.saving || self.uploading)
                        .on_click(cx.listener(|this, _, _, cx| this.save(cx))),
                    ),
            )
            .into_any_element()
    }
}
impl Render for StorageSettings {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut body = div().flex().flex_col().gap(px(14.)).child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(5.))
                        .child(text("存储源", 16., TEXT).font_weight(FontWeight::SEMIBOLD))
                        .child(text("选择默认图床，或添加、编辑存储服务。", 12., MUTED)),
                )
                .child(
                    button("add-storage", "添加存储源")
                        .outline()
                        .disabled(self.saving || self.uploading || self.editor.is_some())
                        .icon(Icon::default().path("icons/plus.svg"))
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.edit(ProviderDraft::new(ProviderKind::R2), window, cx)
                        })),
                ),
        );
        body = body.child(
            button("import-config", "导入 PicGo / PicList 配置")
                .disabled(self.saving || self.uploading || self.editor.is_some())
                .on_click(cx.listener(|this, _, _, cx| this.choose_import(cx))),
        );
        if let Some(plan) = &self.import_plan {
            for (index, candidate) in plan.candidates.iter().enumerate() {
                let candidate = candidate.clone();
                let summary = format!(
                    "{} · {}{}",
                    candidate.name,
                    candidate.kind,
                    if candidate.warnings.is_empty() {
                        String::new()
                    } else {
                        format!(" · {}", candidate.warnings.join("; "))
                    }
                );
                body = body.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(12.))
                        .child(text(summary, 12., MUTED).flex_1())
                        .child(
                            button(
                                SharedString::from(format!("review-import-{index}")),
                                "检查并添加",
                            )
                            .disabled(self.saving || self.uploading || self.editor.is_some())
                            .on_click(cx.listener(
                                move |this, _, window, cx| {
                                    let kind = match candidate.kind.as_str() {
                                        "github" => ProviderKind::Github,
                                        "s3" => ProviderKind::S3,
                                        _ => ProviderKind::Oss,
                                    };
                                    let mut draft = ProviderDraft::new(kind);
                                    draft.name = candidate.name.clone();
                                    draft.path_style = candidate.path_style;
                                    draft.values.extend(candidate.values.clone());
                                    this.edit(draft, window, cx);
                                },
                            )),
                        ),
                );
            }
            for skipped in &plan.skipped {
                body = body.child(text(skipped.clone(), 12., MUTED));
            }
            body = body.child(
                button("dismiss-import", "关闭导入预览").on_click(cx.listener(|this, _, _, cx| {
                    this.import_plan = None;
                    cx.notify();
                })),
            );
        }
        if self.providers.is_empty() && self.editor.is_none() {
            body = body.child(
                div()
                    .p(px(18.))
                    .rounded(px(10.))
                    .bg(crate::theme::color(DROP))
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(text("添加第一个存储源", 14., TEXT))
                    .child(text(
                        "支持 R2、S3、OSS、GitHub 和自定义 HTTP；直接填写并保存即可。",
                        12.,
                        MUTED,
                    )),
            );
        }
        for (name, kind) in &self.providers {
            let edit_name = name.clone();
            let test_name = name.clone();
            let remove_name = name.clone();
            let default_name = name.clone();
            let selected = name == &self.default_provider;
            body = body.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .p(px(12.))
                    .rounded(px(10.))
                    .border_1()
                    .border_color(crate::theme::color(if selected {
                        ORANGE_BORDER
                    } else {
                        BORDER
                    }))
                    .bg(crate::theme::color(if selected {
                        ORANGE_SOFT
                    } else {
                        CANVAS
                    }))
                    .child(icon("folder-open", 22.).text_color(crate::theme::color(NAV_ACTIVE)))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .flex()
                            .flex_col()
                            .gap(px(4.))
                            .child(text(name.clone(), 13., TEXT).text_ellipsis())
                            .child(text(kind.to_uppercase(), 10., MUTED)),
                    )
                    .child(
                        button(SharedString::from(format!("test-{name}")), "测试连接")
                            .ghost()
                            .disabled(self.saving || self.uploading)
                            .on_click(
                                cx.listener(move |this, _, _, cx| this.test(test_name.clone(), cx)),
                            ),
                    )
                    .child(
                        button(SharedString::from(format!("delete-{name}")), "删除")
                            .ghost()
                            .text_color(crate::theme::color(RED))
                            .disabled(self.saving || self.uploading || self.editor.is_some())
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.remove(remove_name.clone(), window, cx)
                            })),
                    )
                    .child(
                        button(
                            SharedString::from(format!("default-{name}")),
                            if selected {
                                "默认存储源"
                            } else {
                                "设为默认"
                            },
                        )
                        .ghost()
                        .disabled(selected || self.saving || self.uploading)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            match storage::set_default(&default_name) {
                                Ok(()) => {
                                    this.refresh(cx);
                                    this.notice = Some(("默认存储源已更新".into(), false));
                                }
                                Err(e) => this.notice = Some((e.to_string(), true)),
                            }
                            cx.notify();
                        })),
                    )
                    .child(
                        button(SharedString::from(format!("edit-{name}")), "编辑")
                            .outline()
                            .disabled(self.saving || self.uploading || self.editor.is_some())
                            .on_click(cx.listener(move |this, _, window, cx| {
                                match storage::edit_provider(&edit_name) {
                                    Ok(draft) => this.edit(draft, window, cx),
                                    Err(e) => {
                                        this.notice = Some((e.to_string(), true));
                                        cx.notify();
                                    }
                                }
                            })),
                    ),
            );
        }
        if let Some(editor) = &self.editor {
            body = body.child(self.editor(editor, cx));
        }
        if let Some((notice, error)) = &self.notice {
            body = body.child(text(
                notice.clone(),
                12.,
                if *error { RED } else { NAV_ACTIVE },
            ));
        }
        body
    }
}
