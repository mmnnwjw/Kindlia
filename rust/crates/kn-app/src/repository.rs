//! 可克隆仓储句柄传给 kn-ui 工作者；页面线程不做 HTTP。
use crate::{
    api::{ApiClient, ListQuery},
    model::{Chapter, Novel, NovelKey, NovelOutline, Page},
    store::Store,
};

pub struct Loaded<T> {
    pub value: T,
    pub notice: Option<String>,
}

/// 本地缓存先走独立队列；未命中才交给网络队列。read 在离线快照上执行，
/// 确保损坏的缓存也不会让 HTTP 请求占住唯一的本地工作线程。
pub fn read_on_workers<A, T, M>(
    cx: &kn_ui::Cx<A>,
    repo: Repository,
    refresh: bool,
    read: impl Fn(&Repository) -> Result<Loaded<T>, String> + Send + 'static,
    message: impl FnOnce(Result<Loaded<T>, String>) -> M + Send + 'static,
) where
    A: kn_ui::App,
    T: Send + 'static,
    M: std::any::Any + Send + 'static,
{
    let poster = cx.page_poster();
    let spawner = cx.spawner();
    cx.spawn_local_droppable(move || {
        if repo.offline || !refresh {
            let mut local = repo.clone();
            local.offline = true;
            let mut result = read(&local);
            if result.is_ok() || repo.offline {
                if !repo.offline {
                    if let Ok(loaded) = &mut result {
                        if loaded.notice.as_deref() == Some("离线缓存") {
                            loaded.notice = None;
                        }
                    }
                }
                poster.post(message(result));
                return;
            }
        }
        if let Some(spawner) = spawner {
            spawner.spawn_on(kn_ui::Lane::Net, true, move || message(read(&repo)));
        }
    });
}
#[derive(Clone)]
pub struct Repository {
    pub api: ApiClient,
    pub store: Store,
    pub offline: bool,
}
impl Repository {
    fn load<T: serde::Serialize + serde::de::DeserializeOwned>(
        &self,
        kind: &str,
        key: &impl serde::Serialize,
        refresh: bool,
        fetch: impl FnOnce() -> Result<T, String>,
    ) -> Result<Loaded<T>, String> {
        let cached = self.store.read_cache(kind, key);
        if self.offline || !refresh {
            if let Some(value) = cached {
                return Ok(Loaded {
                    value,
                    notice: self.offline.then(|| "离线缓存".into()),
                });
            }
            if self.offline {
                return Err("离线模式：此内容尚未缓存".into());
            }
        }
        match fetch() {
            Ok(value) => {
                let notice = self
                    .store
                    .write_cache(kind, key, &value)
                    .err()
                    .map(|e| format!("已联网，但缓存写入失败: {e}"));
                Ok(Loaded { value, notice })
            }
            Err(error) => {
                eprintln!("[repository:{kind}] {error}");
                match self.store.read_cache(kind, key) {
                    Some(value) => Ok(Loaded {
                        value,
                        notice: Some(format!("{error}；显示本地缓存")),
                    }),
                    None => Err(error),
                }
            }
        }
    }
    pub fn list(
        &self,
        query: &ListQuery,
        refresh: bool,
    ) -> Result<Loaded<Page<NovelOutline>>, String> {
        self.load("lists", query, refresh, || self.api.list(query))
    }
    pub fn novel(&self, key: &NovelKey, refresh: bool) -> Result<Loaded<Novel>, String> {
        if key.provider_id == "epub" {
            return crate::epub::novel(&self.store, key).map(|value| Loaded {
                value,
                notice: None,
            });
        }
        self.load("novels", key, refresh, || self.api.novel(key))
    }
    pub fn chapter(
        &self,
        key: &NovelKey,
        id: &str,
        refresh: bool,
    ) -> Result<Loaded<Chapter>, String> {
        if key.provider_id == "epub" {
            return crate::epub::chapter(&self.store, key, id).map(|value| Loaded {
                value,
                notice: None,
            });
        }
        self.load("chapters", &(key, id), refresh, || {
            self.api.chapter(key, id)
        })
    }
    pub fn image_bytes(&self,url:&str)->Result<Vec<u8>,String> {
        if url.starts_with("epub://") { return crate::epub::image(&self.store,url); }
        let path=self.store.cache_path("images",&url).with_extension("img");
        if let Ok(bytes)=std::fs::read(&path) {return Ok(bytes);}
        if self.offline {return Err("插图未缓存".into());}
        let bytes=self.api.image(url)?;
        crate::store::atomic_write(&path,&bytes,false).map_err(|e|e.to_string())?;
        Ok(bytes)
    }
    pub fn image(&self, url: &str, size: (u32, u32)) -> Result<kn_render::Bitmap, String> {
        if url.starts_with("epub://") {
            return kn_render::image::decode_gray(
                &crate::epub::image(&self.store, url)?,
                Some(size),
            )
            .map(|b| b.fit_within(size.0, size.1))
            .map_err(|e| e.to_string());
        }
        let path = self.store.cache_path("images", &url).with_extension("img");
        if let Ok(bytes) = std::fs::read(&path) {
            if let Ok(bitmap) = kn_render::image::decode_gray(&bytes, Some(size)) {
                return Ok(bitmap.fit_within(size.0, size.1));
            }
            let _ = std::fs::remove_file(&path);
        }
        if self.offline {
            return Err("插图未缓存".into());
        }
        let bytes = self.api.image(url)?;
        let bitmap =
            kn_render::image::decode_gray(&bytes, Some(size)).map_err(|e| e.to_string())?;
        if let Err(e) = crate::store::atomic_write(&path, &bytes, false) {
            eprintln!("[cache] {e}");
        }
        Ok(bitmap.fit_within(size.0, size.1))
    }
}
