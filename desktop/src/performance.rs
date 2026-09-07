//! Opt-in native release benchmark. Requires an explicitly marked, isolated fixture directory.
use super::*;

impl ImgDesktop {
    pub fn start_benchmark(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(output) = std::env::var_os("IMG_PERF_OUTPUT").map(PathBuf::from) else {
            return;
        };
        if !self.root.join(".img-perf-fixture").is_file() {
            return;
        }
        self.page = Page::Library;
        self.preferences.library_view = LibraryView::Grid;
        let count = self.queue.items.len();
        let baseline = std::env::var_os("IMG_PERF_BASELINE").is_some();
        let mut searches = Vec::new();
        for n in 0..60 {
            let query = format!("photo-{:05}", n * 37 % count.max(1));
            let start = std::time::Instant::now();
            if baseline {
                std::hint::black_box(
                    self.queue
                        .items
                        .iter()
                        .filter(|i| i.matches(&query))
                        .cloned()
                        .collect::<Vec<_>>(),
                );
            } else {
                std::hint::black_box(self.record_index.borrow_mut().rows(
                    &self.queue.items,
                    self.records_revision,
                    &query,
                    1,
                    0,
                ));
            }
            searches.push(start.elapsed().as_secs_f64() * 1000.);
        }
        searches.sort_by(f64::total_cmp);
        cx.spawn_in(window, async move |this, cx| {
            let mut before = None;
            let mut cache_peak = 0;
            for step in 0..if baseline && count >= 10_000 { 40 } else { 180 } {
                cx.background_executor().timer(Duration::from_millis(17)).await;
                let _ = this.update_in(cx, |this, window, cx| {
                    if step == if baseline && count >= 10_000 { 5 } else { 30 } { before = Some(window.frame_duration_snapshot()); }
                    if baseline {
                        this.legacy_scroll.set_offset(point(px(0.), px(-(step as f32 * 272.))));
                    } else {
                        this.list_scroll.scroll_to_item(step % count.div_ceil(4).max(1), ScrollStrategy::Top);
                    }
                    cache_peak = cache_peak.max(this.thumbnails.read(cx).bytes());
                    cx.notify();
                });
            }
            let report = this.update_in(cx, |_, window, _| {
                let mut after = window.frame_duration_snapshot();
                if let Some(before) = before {
                    let _ = after.draw_duration_histogram.subtract(&before.draw_duration_histogram);
                    let _ = after.dirty_to_present_histogram.subtract(&before.dirty_to_present_histogram);
                    let _ = after.present_interval_histogram.subtract(&before.present_interval_histogram);
                }
                serde_json::json!({
                    "records":count,"baseline":baseline,"search_p95_ms":searches[57],
                    "draw_samples":after.draw_duration_histogram.len(),
                    "draw_p95_ms":after.draw_duration_histogram.value_at_quantile(0.95) as f64 / 1_000_000.,
                    "dirty_to_present_p95_ms":after.dirty_to_present_histogram.value_at_quantile(0.95) as f64 / 1_000_000.,
                    "present_interval_p95_ms":after.present_interval_histogram.value_at_quantile(0.95) as f64 / 1_000_000.,
                    "thumbnail_cache_peak_bytes":cache_peak
                })
            });
            if let Ok(report) = report {
                cx.background_executor().spawn(async move {
                    std::fs::write(output, serde_json::to_vec_pretty(&report).unwrap())
                }).await.ok();
            }
            let _ = cx.update(|_, cx| cx.quit());
        }).detach();
    }

    // Controlled baseline retains the original full-row cloning and full element construction.
    // Layout and runtime are shared so the benchmark isolates indexing/rendering/cache changes.
    pub(super) fn baseline_records(&self, _window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let rows: Vec<_> = self
            .queue
            .items
            .iter()
            .filter(|i| i.status == Status::Done)
            .cloned()
            .collect();
        div()
            .id("baseline-scroll")
            .flex_1()
            .min_h(px(0.))
            .overflow_y_scroll()
            .track_scroll(&self.legacy_scroll)
            .child(
                div().w_full().grid().grid_cols(4).gap(px(14.)).children(
                    rows.into_iter()
                        .map(|item| div().h(px(258.)).child(self.library_grid_card(item, cx))),
                ),
            )
            .into_any_element()
    }
}
