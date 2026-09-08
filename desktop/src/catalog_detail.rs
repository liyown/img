use super::*;
pub(super) struct Detail {
    asset: Asset,
    root: PathBuf,
    format: CopyFormat,
    thumbnails: Entity<crate::thumbnails::ThumbnailCache>,
    image: Option<std::sync::Arc<img_records::cache::CacheLease>>,
    copied: Option<(String, std::time::Instant)>,
    notice: Option<String>,
    versions: Vec<(Asset, String, bool)>,
}
impl Detail {
    pub fn new(
        asset: Asset,
        root: PathBuf,
        format: CopyFormat,
        thumbnails: Entity<crate::thumbnails::ThumbnailCache>,
        image: Option<std::sync::Arc<img_records::cache::CacheLease>>,
        cx: &mut Context<Self>,
    ) -> Self {
        let directory = root.clone();
        let id = asset.id.clone();
        let task = cx
            .background_executor()
            .spawn(async move { Catalog::open(&directory)?.related_versions(&id) });
        cx.spawn(async move |this, cx| {
            let result: anyhow::Result<_> = task.await;
            let _ = this.update(cx, |this, cx| {
                if let Ok(versions) = result {
                    this.versions = versions;
                }
                cx.notify();
            });
        })
        .detach();
        Self {
            asset,
            root,
            format,
            thumbnails,
            image,
            copied: None,
            notice: None,
            versions: vec![],
        }
    }
    fn copy(&mut self, id: String, url: String, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(
            self.format.render(&self.asset.name, &url),
        ));
        let feedback = (id.clone(), std::time::Instant::now());
        self.copied = Some(feedback.clone());
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_secs(2)).await;
            let _ = this.update(cx, |this, cx| {
                if this.copied.as_ref() == Some(&feedback) {
                    this.copied = None;
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
    fn prefer(&mut self, location: String, cx: &mut Context<Self>) {
        let root = self.root.clone();
        let id = self.asset.id.clone();
        let chosen = location.clone();
        let task = cx
            .background_executor()
            .spawn(async move { Catalog::open(&root)?.set_preferred(&id, &chosen) });
        cx.spawn(async move |this, cx| {
            let result: anyhow::Result<_> = task.await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(()) => this.asset.preferred_location = Some(location),
                    Err(_) => this.notice = Some("无法保存首选链接，请刷新后重试。".into()),
                }
                cx.notify();
            });
        })
        .detach();
    }
}
impl Render for Detail {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let height = (f32::from(window.viewport_size().height) * 0.55)
            .min(520.)
            .min((f32::from(window.viewport_size().height) - 300.).max(80.));
        let mut body = div().flex().flex_col().gap(px(10.)).child(label(
            format!(
                "{} · {}",
                self.asset.content_type,
                model::size_label(self.asset.size)
            ),
            12.,
            MUTED,
        ));
        if let Some(image) = &self.image {
            body = body.child(
                img(image.path.clone())
                    .image_cache(&self.thumbnails)
                    .w_full()
                    .h(px(height))
                    .object_fit(ObjectFit::Contain),
            );
        } else {
            body = body.child(label(
                "此版本尚无可用预览，原图可能已不可访问。",
                12.,
                MUTED,
            ));
        }
        let mut links = div()
            .id("detail-addresses")
            .max_h(px(160.))
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap(px(8.));
        for location in self.asset.locations.clone() {
            let id = location.id.clone();
            let url = location.url.clone();
            let preferred = self.asset.preferred_location.as_ref() == Some(&id);
            let copied = self
                .copied
                .as_ref()
                .is_some_and(|(copied, _)| copied == &id);
            let weak = cx.weak_entity();
            let chosen = id.clone();
            let state = match location.availability.as_str() {
                "available" => "可用",
                "deleted" => "已删除",
                "pending-missing" => "待确认不存在",
                "forbidden" => "无访问权限",
                _ => "尚未验证",
            };
            links = links.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(label(
                                format!(
                                    "{}{} · {}",
                                    location.provider,
                                    if preferred {
                                        format!(" · {}", crate::i18n::text("首选"))
                                    } else {
                                        String::new()
                                    },
                                    crate::i18n::text(state)
                                ),
                                12.,
                                TEXT,
                            ))
                            .child(label(location.url, 11., MUTED).text_ellipsis())
                            .when_some(location.last_checked, |body, checked| {
                                body.child(label(
                                    format!(
                                        "最后检查：{} 分钟前",
                                        std::time::SystemTime::now()
                                            .duration_since(std::time::UNIX_EPOCH)
                                            .unwrap_or_default()
                                            .as_secs()
                                            .saturating_sub(checked)
                                            / 60
                                    ),
                                    11.,
                                    MUTED,
                                ))
                            }),
                    )
                    .child(
                        Button::new(SharedString::from(format!("detail-copy-{id}")))
                            .ghost()
                            .icon(if copied {
                                IconName::Check
                            } else {
                                IconName::Copy
                            })
                            .w(px(30.))
                            .h(px(30.))
                            .accessibility_label(crate::i18n::text(if copied {
                                "已复制"
                            } else {
                                "复制链接"
                            }))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.copy(id.clone(), url.clone(), cx)
                            })),
                    )
                    .child(
                        Button::new(SharedString::from(format!("detail-more-{chosen}")))
                            .ghost()
                            .icon(IconName::Ellipsis)
                            .w(px(28.))
                            .accessibility_label(crate::i18n::text("地址操作"))
                            .dropdown_menu(move |menu, _, _| {
                                let weak = weak.clone();
                                let chosen = chosen.clone();
                                menu.item(
                                    PopupMenuItem::new(crate::i18n::text("设为首选链接"))
                                        .checked(preferred)
                                        .on_click(move |_, _, cx| {
                                            let _ = weak.update(cx, |this, cx| {
                                                this.prefer(chosen.clone(), cx)
                                            });
                                        }),
                                )
                            }),
                    ),
            );
        }
        body = body.child(links);
        let mut versions = div()
            .id("related-versions")
            .max_h(px(84.))
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap(px(6.));
        for (asset, _, ancestor) in &self.versions {
            versions = versions.child(label(
                format!(
                    "{}：{}{}",
                    crate::i18n::text(if *ancestor {
                        "来源版本"
                    } else {
                        "处理版本"
                    }),
                    asset.name,
                    if asset.locations.is_empty()
                        || asset
                            .locations
                            .iter()
                            .all(|location| location.availability == "deleted")
                    {
                        format!(" · {}", crate::i18n::text("已不可访问"))
                    } else {
                        String::new()
                    }
                ),
                12.,
                MUTED,
            ));
        }
        body = body.when(!self.versions.is_empty(), |body| body.child(versions));
        body.when_some(self.notice.as_ref(), |body, message| {
            body.child(label(message.clone(), 12., RED))
        })
    }
}
