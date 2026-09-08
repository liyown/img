use gpui_kit::{
    App, AppContext, Entity, ImageCache, ImageCacheError, RenderImage, Resource,
    RetainAllImageCache, WeakEntity, Window,
};
use std::{
    collections::{HashMap, HashSet},
    hash::Hash,
    path::{Path, PathBuf},
    sync::Arc,
};

pub const BUDGET: usize = 64 * 1024 * 1024;
#[cfg(test)]
const RESERVATION: usize = 512 * 512 * 4;

#[derive(Default)]
struct Budget<K> {
    entries: HashMap<K, (usize, u64)>,
    bytes: usize,
    clock: u64,
}
impl<K: Clone + Eq + Hash> Budget<K> {
    fn touch(&mut self, key: K, bytes: usize) -> Vec<K> {
        self.clock += 1;
        if let Some((old, _)) = self.entries.insert(key.clone(), (bytes, self.clock)) {
            self.bytes -= old;
        }
        self.bytes += bytes;
        let mut removed = vec![];
        while self.bytes > BUDGET {
            let oldest = self
                .entries
                .iter()
                .min_by_key(|(_, (_, stamp))| stamp)
                .map(|(k, _)| k.clone());
            let Some(oldest) = oldest else {
                break;
            };
            self.bytes -= self.entries.remove(&oldest).unwrap().0;
            removed.push(oldest);
        }
        removed
    }
}
pub struct ThumbnailCache {
    root: PathBuf,
    side: u32,
    entity: WeakEntity<Self>,
    inner: Entity<RetainAllImageCache>,
    resolved: HashMap<Resource, Option<Resource>>,
    preparing: HashSet<Resource>,
    budget: Budget<Resource>,
}
impl ThumbnailCache {
    pub fn new(root: PathBuf, cx: &mut App) -> Entity<Self> {
        Self::with_side(root, 512, cx)
    }
    pub fn preview(root: PathBuf, cx: &mut App) -> Entity<Self> {
        Self::with_side(root, 1440, cx)
    }
    fn with_side(root: PathBuf, side: u32, cx: &mut App) -> Entity<Self> {
        let inner = RetainAllImageCache::new(cx);
        cx.new(|cx| Self {
            root,
            side,
            entity: cx.weak_entity(),
            inner,
            resolved: HashMap::new(),
            preparing: HashSet::new(),
            budget: Budget {
                entries: HashMap::new(),
                bytes: 0,
                clock: 0,
            },
        })
    }
    #[cfg(feature = "perf")]
    pub fn bytes(&self) -> usize {
        self.budget.bytes
    }
    pub fn clear(&mut self, window: &mut Window, cx: &mut App) {
        self.inner.update(cx, |inner, cx| inner.clear(window, cx));
        self.budget.entries.clear();
        self.budget.bytes = 0;
    }
}
impl ImageCache for ThumbnailCache {
    fn load(
        &mut self,
        source: &Resource,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<Result<Arc<RenderImage>, ImageCacheError>> {
        let mapped = match source {
            Resource::Path(path) => {
                if let Some(mapped) = self.resolved.get(source) {
                    mapped.clone()?
                } else {
                    if self.preparing.contains(source) || self.preparing.len() >= 4 {
                        return None;
                    }
                    self.preparing.insert(source.clone());
                    let key = source.clone();
                    let path = path.to_path_buf();
                    let root = self.root.clone();
                    let side = self.side;
                    let task = cx
                        .background_executor()
                        .spawn(async move { prepare_side(&root, &path, side).map(Resource::from) });
                    let weak = self.entity.clone();
                    let view = window.current_view();
                    window
                        .spawn(cx, async move |cx| {
                            let result = task.await.ok();
                            let _ = weak.update(cx, |this, cx| {
                                this.preparing.remove(&key);
                                this.resolved.insert(key, result);
                                App::notify(cx, view);
                            });
                        })
                        .detach();
                    return None;
                }
            }
            Resource::Embedded(_) => source.clone(),
            Resource::Uri(_) => return None,
        };
        // Reserve a bounded slot before starting a decode; evict all CPU/GPU cache references.
        let current = self
            .budget
            .entries
            .get(&mapped)
            .map(|e| e.0)
            .unwrap_or(self.side as usize * self.side as usize * 4);
        for resource in self.budget.touch(mapped.clone(), current) {
            self.inner
                .update(cx, |inner, cx| inner.remove(&resource, window, cx));
        }
        let result = self
            .inner
            .update(cx, |inner, cx| inner.load(&mapped, window, cx));
        if let Some(Ok(image)) = &result {
            let bytes = (0..image.frame_count())
                .filter_map(|n| image.as_bytes(n))
                .map(<[u8]>::len)
                .sum();
            for resource in self.budget.touch(mapped, bytes) {
                self.inner
                    .update(cx, |inner, cx| inner.remove(&resource, window, cx));
            }
        }
        result
    }
}

pub fn prepare(root: &Path, preview: &Path) -> anyhow::Result<PathBuf> {
    prepare_side(root, preview, 512)
}
fn prepare_side(root: &Path, preview: &Path, side: u32) -> anyhow::Result<PathBuf> {
    use anyhow::{Context, ensure};
    if let Ok(relative) = preview.strip_prefix(root.join("cache")) {
        use image::ImageDecoder;
        let key = relative.to_str().context("缓存键无效")?;
        let cache = img_records::cache::Cache::open(root)?;
        let catalog = img_records::catalog::Catalog::open(root)?;
        let original = cache.lease(key)?;
        let setting = if side == 512 {
            format!("thumbnail:{key}")
        } else {
            format!("thumbnail-{side}:{key}")
        };
        if let Some(thumbnail) = catalog.setting(&setting)?
            && let Ok(lease) = cache.lease(&thumbnail)
        {
            return Ok(lease.path.clone());
        }
        let mut reader = image::ImageReader::open(&original.path)?.with_guessed_format()?;
        let mut limits = image::Limits::default();
        limits.max_alloc = Some(256 << 20);
        limits.max_image_width = Some(32768);
        limits.max_image_height = Some(32768);
        reader.limits(limits);
        let mut decoder = reader.into_decoder()?;
        let (w, h) = decoder.dimensions();
        ensure!(
            u64::from(w) * u64::from(h) <= 40_000_000,
            "图片超过 40 MP 预览限制"
        );
        let orientation = decoder.orientation()?;
        let mut image = image::DynamicImage::from_decoder(decoder)?;
        image.apply_orientation(orientation);
        let mut bytes = std::io::Cursor::new(Vec::new());
        image
            .thumbnail(side, side)
            .write_to(&mut bytes, image::ImageFormat::Png)?;
        let thumbnail = cache.put(&bytes.into_inner())?;
        catalog.set_setting(&setting, &thumbnail)?;
        let protected = cache.lease(&thumbnail)?;
        cache.trim(cache.limit()?)?;
        return Ok(protected.path.clone());
    }
    let images = root.join("images");
    let relative = preview
        .strip_prefix(&images)
        .context("预览不在应用缓存目录中")?;
    let parts: Vec<_> = relative.components().collect();
    ensure!(
        parts.len() == 2 && parts[1].as_os_str() == "preview.png",
        "预览路径无效"
    );
    uuid::Uuid::parse_str(parts[0].as_os_str().to_str().context("预览路径无效")?)?;
    let parent = preview.parent().unwrap();
    ensure!(
        !parent.symlink_metadata()?.file_type().is_symlink(),
        "预览目录不能是符号链接"
    );
    img_records::cache::Cache::open(root)?.touch_legacy(preview)?;
    let target = parent.join("thumbnail-512.png");
    if target.is_file()
        && !target.symlink_metadata()?.file_type().is_symlink()
        && image::image_dimensions(&target)
            .is_ok_and(|(w, h)| w > 0 && h > 0 && w <= 512 && h <= 512)
    {
        return Ok(target);
    }
    let mut reader = image::ImageReader::open(preview)?.with_guessed_format()?;
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(64 * 1024 * 1024);
    limits.max_image_width = Some(1600);
    limits.max_image_height = Some(1600);
    reader.limits(limits);
    let image = reader.decode()?.thumbnail(512, 512);
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    image.write_to(&mut file, image::ImageFormat::Png)?;
    file.as_file().sync_all()?;
    file.persist(&target).map_err(|e| e.error)?;
    Ok(target)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lru_evicts_old_entries_and_never_exceeds_budget() {
        let mut budget = Budget::default();
        for n in 0..128 {
            budget.touch(n, RESERVATION);
            assert!(budget.bytes <= BUDGET);
        }
        budget.touch(64, RESERVATION);
        assert_eq!(budget.touch(128, RESERVATION), vec![65]);
        assert!(budget.entries.contains_key(&64));
    }
    #[test]
    fn old_previews_are_resized_without_changing_originals_or_following_directories() {
        let root = tempfile::tempdir().unwrap();
        let dir = root
            .path()
            .join("images")
            .join(uuid::Uuid::new_v4().to_string());
        std::fs::create_dir_all(&dir).unwrap();
        let preview = dir.join("preview.png");
        image::RgbaImage::new(1200, 900).save(&preview).unwrap();
        let original = std::fs::read(&preview).unwrap();
        let path = prepare(root.path(), &preview).unwrap();
        assert_eq!(image::image_dimensions(path).unwrap(), (512, 384));
        assert_eq!(std::fs::read(&preview).unwrap(), original);
        assert!(prepare(root.path(), &root.path().join("elsewhere.png")).is_err());
    }
}
