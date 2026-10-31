use crate::{model::NovelKey, repository::Repository};
use kn_render::Bitmap;
use serde::{Deserialize, Serialize};
pub const DEFAULT_URL: &str = "https://n-cdn.novelia.cc/static/no_cover.c90924af53e4.webp";
#[derive(Default, Clone, Serialize, Deserialize)]
pub struct Metadata {
    pub author: String,
    pub cover: String,
}
pub fn fallback(size: (u32,u32))->Result<Bitmap,String>{
    static IMAGE: std::sync::OnceLock<Result<Bitmap,String>> = std::sync::OnceLock::new();
    match IMAGE.get_or_init(||kn_render::image::decode_gray(include_bytes!("../../../../assets/no_cover.webp"),None).map_err(|e|e.to_string())) {
        Ok(image)=>Ok(image.fit_within(size.0,size.1)),Err(e)=>Err(e.clone())
    }
}
pub fn image(repo: &Repository, url: &str, size: (u32, u32)) -> Result<Bitmap, String> {
    if url.is_empty() || url == DEFAULT_URL {
        return fallback(size);
    }
    repo.image(url, size).or_else(|_| fallback(size))
}
pub fn remember(repo: &Repository, key: &NovelKey, author: String, cover: String) {
    let prior = repo.store.read_cache::<Metadata>("book-meta", key).unwrap_or_default();
    let author = if author.is_empty() { prior.author } else { author };
    let cover = if cover.is_empty() { prior.cover } else { cover };
    let _ = repo.store.write_cache("book-meta", key, &Metadata { author, cover });
}
pub fn metadata(repo: &Repository, key: &NovelKey, network: bool) -> Metadata {
    if let Some(meta) = repo.store.read_cache("book-meta", key) {
        return meta;
    }
    let mut source = key.clone();
    if key.provider_id == "epub" {
        if let Ok(book) = crate::epub::index(&repo.store, key) {
            if let Some(path) = book.cover {
                return Metadata {
                    author: book.authors.join(" / "),
                    cover: format!("epub://{}/{}", key.novel_id, crate::api::encode(&path)),
                };
            }
        }
        if let Some(v) = repo
            .store
            .read_cache::<serde_json::Value>("epub-source", key)
        {
            if let Some(id) = v["novel"].as_str() {
                source = NovelKey {
                    provider_id: "wenku".into(),
                    novel_id: id.into(),
                };
            }
        }
    }
    if source.provider_id != "wenku" && source.provider_id != "epub" {
        if let Some(n) = repo
            .store
            .read_cache::<crate::model::Novel>("novels", &source)
        {
            if let Some(id) = n.wenku_id {
                source = NovelKey {
                    provider_id: "wenku".into(),
                    novel_id: id,
                };
            }
        }
    }
    if source.provider_id == "wenku" {
        let path = format!("wenku/{}", crate::api::encode(&source.novel_id));
        let value = repo
            .store
            .read_cache::<serde_json::Value>("cloud", &(&path, repo.api.username()))
            .or_else(|| {
                if network && !repo.offline {
                    repo.api.get(&path).ok()
                } else {
                    None
                }
            });
        if let Some(v) = value {
            let meta = Metadata {
                author: v["authors"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|s| s.as_str())
                            .collect::<Vec<_>>()
                            .join(" / ")
                    })
                    .unwrap_or_default(),
                cover: v["cover"].as_str().unwrap_or("").into(),
            };
            let _ = repo.store.write_cache("book-meta", key, &meta);
            return meta;
        }
    } else if let Some(novel) = repo
        .store
        .read_cache::<crate::model::Novel>("novels", &source)
    {
        return Metadata {
            author: novel
                .authors
                .iter()
                .map(|a| a.name.as_str())
                .collect::<Vec<_>>()
                .join(" / "),
            cover: String::new(),
        };
    }
    Metadata::default()
}
pub fn for_book(repo: &Repository, key: &NovelKey, size: (u32, u32)) -> Result<Bitmap, String> {
    image(repo, &metadata(repo, key, true).cover, size)
}

/// Metadata lookup and original-byte download are separated from local image decoding.
pub fn spawn<M:std::any::Any+Send>(cx:&kn_ui::Cx<crate::Kindlia>,key:NovelKey,size:(u32,u32),finish:impl FnOnce(Metadata,Result<Bitmap,String>)->M+Send+'static) {
 let repo=cx.app.repo.clone();let Some(spawner)=cx.spawner() else{return;};let worker=spawner.clone();
 worker.spawn_on(kn_ui::Lane::Local,true,move || {
  let meta=metadata(&repo,&key,false);
  let decode=move |meta:Metadata,spawner:kn_ui::Spawner<crate::Kindlia>,repo:Repository| {
   let url=meta.cover.clone();
   crate::images::spawn_on(spawner,repo,url,move |bytes| {
    let result=bytes.and_then(|b|b.decode(Some(size),kn_render::image::ImageBudget::Normal).map(|b|b.fit_within(size.0,size.1)).map_err(|e|e.to_string())).or_else(|_|fallback(size));finish(meta,result)
   });
  };
  if meta.cover.is_empty() && !repo.offline {
   let local=spawner.clone();spawner.spawn_on(kn_ui::Lane::Media,true,move || {let meta=metadata(&repo,&key,true);decode(meta,local,repo);});
  } else {decode(meta,spawner,repo);}
 });
}
