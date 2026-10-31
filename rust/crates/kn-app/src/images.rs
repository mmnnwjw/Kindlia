//! Adapted from KinNovel net::spawn_image: local cache -> media download -> local decode.
use crate::{repository::Repository, Kindlia};
use kn_ui::{Cx, Lane};
use std::path::PathBuf;
pub const RETRY_AFTER: std::time::Duration = std::time::Duration::from_secs(20);
pub struct CachedImage {
    pub bytes: Vec<u8>,
    file: Option<PathBuf>,
}
impl CachedImage {
    pub fn decode(
        &self,
        size: Option<(u32, u32)>,
        budget: kn_render::image::ImageBudget,
    ) -> Result<kn_render::Bitmap, kn_render::ImageError> {
        let result = kn_render::image::decode_gray_with_budget(&self.bytes, size, budget);
        if let Err(err) = &result {
            let corrupt = match err {
                kn_render::ImageError::Decode(_) => true,
                kn_render::ImageError::Unsupported => {
                    !(self.bytes.starts_with(b"GIF8") || self.bytes.get(4..8) == Some(b"ftyp"))
                }
                _ => false,
            };
            if corrupt {
                if let Some(file) = &self.file {
                    let _ = std::fs::remove_file(file);
                }
            }
        }
        result
    }
}
/// Finish executes on Local, including decode. Cached and EPUB images never wait for Net.
pub fn spawn<
    M: std::any::Any + Send,
    F: FnOnce(Result<CachedImage, String>) -> M + Send + 'static,
>(
    cx: &Cx<Kindlia>,
    repo: Repository,
    url: String,
    finish: F,
) {
    let Some(spawner) = cx.spawner() else { return };
    spawn_on(spawner, repo, url, finish);
}
pub fn spawn_on<
    M: std::any::Any + Send,
    F: FnOnce(Result<CachedImage, String>) -> M + Send + 'static,
>(
    spawner: kn_ui::Spawner<Kindlia>,
    repo: Repository,
    url: String,
    finish: F,
) {
    let local = spawner.clone();
    local.spawn_on(Lane::Local, true, move || {
        let local = if url.starts_with("epub://") {
            Some(
                crate::epub::image(&repo.store, &url)
                    .map(|bytes| CachedImage { bytes, file: None }),
            )
        } else {
            let file = repo.store.cache_path("images", &url).with_extension("img");
            std::fs::read(&file).ok().map(|bytes| {
                Ok(CachedImage {
                    bytes,
                    file: Some(file),
                })
            })
        };
        if let Some(result) = local {
            spawner.spawn_on(Lane::Local, true, move || finish(result));
        } else if repo.offline {
            spawner.spawn_on(Lane::Local, true, move || finish(Err("插图未缓存".into())));
        } else {
            let decode = spawner.clone();
            spawner.spawn_on(Lane::Media, true, move || {
                let result = repo.api.image(&url).map(|bytes| {
                    let file = repo.store.cache_path("images", &url).with_extension("img");
                    if let Err(e) = crate::store::atomic_write(&file, &bytes, false) {
                        eprintln!("[image-cache] {e}");
                    }
                    CachedImage {
                        bytes,
                        file: Some(file),
                    }
                });
                decode.spawn_on(Lane::Local, true, move || finish(result));
            });
        }
    });
}
pub fn spawn_fit<M: std::any::Any + Send>(
    cx: &Cx<Kindlia>,
    repo: Repository,
    url: String,
    size: (u32, u32),
    finish: impl FnOnce(Result<kn_render::Bitmap, String>) -> M + Send + 'static,
) {
    spawn(cx, repo, url, move |bytes| {
        finish(bytes.and_then(|b| {
            b.decode(Some(size), kn_render::image::ImageBudget::Normal)
                .map(|b| b.fit_within(size.0, size.1).dithered())
                .map_err(|e| e.to_string())
        }))
    });
}
