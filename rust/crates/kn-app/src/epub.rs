//! 文库 EPUB 在本地保留 ZIP；按 spine 懒读章节/插图，不将路径解压到文件系统。
use crate::{
    model::{digest, Chapter, Novel, NovelKey},
    store::Store,
};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, fs::File, io::Read, path::Path};

#[derive(Clone, Serialize, Deserialize)]
pub struct EpubChapter {
    pub path: String,
    pub title: String,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Navigation {
    pub title: String,
    pub chapter_id: String,
    pub depth: usize,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Book {
    pub title: String,
    pub chapters: Vec<EpubChapter>,
    #[serde(default)]
    pub plain_text: bool,
    #[serde(default)]
    pub navigation: Vec<Navigation>,
    #[serde(default)]
    pub index_version: u8,
    #[serde(default)]
    pub spine_len: usize,
    #[serde(default)]
    pub cover: Option<String>,
    #[serde(default)]
    pub authors: Vec<String>,
}
mod markup;
type ArchiveStamp = (std::path::PathBuf, u64, std::time::SystemTime);
fn archive_pool() -> &'static std::sync::Mutex<Vec<(ArchiveStamp, zip::ZipArchive<File>)>> {
    static POOL: std::sync::OnceLock<std::sync::Mutex<Vec<(ArchiveStamp, zip::ZipArchive<File>)>>> =
        std::sync::OnceLock::new();
    POOL.get_or_init(|| std::sync::Mutex::new(Vec::new()))
}
struct Archive {
    zip: Option<zip::ZipArchive<File>>,
    stamp: Option<ArchiveStamp>,
}
impl Drop for Archive {
    fn drop(&mut self) {
        if let (Some(stamp), Some(zip)) = (self.stamp.take(), self.zip.take()) {
            if let Ok(mut pool) = archive_pool().lock() {
                pool.retain(|(s, _)| s.0 != stamp.0);
                pool.push((stamp, zip));
                if pool.len() > 2 {
                    pool.remove(0);
                }
            }
        }
    }
}
impl Archive {
    fn open(path: &Path) -> Result<Self, String> {
        let zip = zip::ZipArchive::new(File::open(path).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        if zip.len() > 10000 {
            return Err("EPUB 文件条目过多".into());
        }
        Ok(Self {
            zip: Some(zip),
            stamp: None,
        })
    }
    fn cached(path: &Path) -> Result<Self, String> {
        let meta = std::fs::metadata(path).map_err(|e| e.to_string())?;
        let stamp = (
            path.to_owned(),
            meta.len(),
            meta.modified().map_err(|e| e.to_string())?,
        );
        if let Ok(mut pool) = archive_pool().lock() {
            if let Some(i) = pool.iter().position(|(s, _)| *s == stamp) {
                let (_, zip) = pool.remove(i);
                return Ok(Self {
                    zip: Some(zip),
                    stamp: Some(stamp),
                });
            }
            pool.retain(|(s, _)| s.0 != stamp.0);
        }
        let mut archive = Self::open(path)?;
        archive.stamp = Some(stamp);
        Ok(archive)
    }
    fn read(&mut self, name: &str, limit: u64) -> Result<Vec<u8>, String> {
        let mut f = self
            .zip
            .as_mut()
            .unwrap()
            .by_name(name)
            .map_err(|e| format!("EPUB {name}: {e}"))?;
        if f.size() > limit {
            return Err(format!("EPUB 条目超过限制: {name}"));
        }
        let mut bytes = Vec::new();
        (&mut f)
            .take(limit + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() as u64 > limit {
            return Err("EPUB 解压超限".into());
        }
        Ok(bytes)
    }
    fn text(&mut self, name: &str) -> Result<String, String> {
        decode_text(self.read(name, 8 * 1024 * 1024)?)
    }
}
fn decode_text(bytes: Vec<u8>) -> Result<String, String> {
    if bytes.starts_with(&[0xff, 0xfe]) || bytes.starts_with(&[0xfe, 0xff]) {
        if bytes.len() % 2 != 0 {
            return Err("EPUB UTF-16 长度无效".into());
        }
        let little = bytes[0] == 0xff;
        let units: Vec<u16> = bytes[2..]
            .chunks_exact(2)
            .map(|p| {
                if little {
                    u16::from_le_bytes([p[0], p[1]])
                } else {
                    u16::from_be_bytes([p[0], p[1]])
                }
            })
            .collect();
        String::from_utf16(&units).map_err(|e| e.to_string())
    } else {
        String::from_utf8(
            bytes
                .strip_prefix(&[0xef, 0xbb, 0xbf])
                .unwrap_or(&bytes)
                .to_vec(),
        )
        .map_err(|e| e.to_string())
    }
}
fn xml(text: &str) -> Result<roxmltree::Document<'_>, roxmltree::Error> {
    // roxmltree never fetches external DTDs; internal entities retain its expansion limits.
    roxmltree::Document::parse_with_options(
        text,
        roxmltree::ParsingOptions {
            allow_dtd: true,
            nodes_limit: 200_000,
        },
    )
}
pub fn unescape(value: &str) -> String {
    let b = value.as_bytes();
    let mut v = vec![];
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            let hex = |n: u8| match n {
                b'0'..=b'9' => Some(n - b'0'),
                b'a'..=b'f' => Some(n - b'a' + 10),
                b'A'..=b'F' => Some(n - b'A' + 10),
                _ => None,
            };
            if let (Some(a), Some(c)) = (hex(b[i + 1]), hex(b[i + 2])) {
                v.push(a * 16 + c);
                i += 3;
                continue;
            }
        }
        v.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&v).into_owned()
}
fn resolve(base: &str, href: &str) -> Result<String, String> {
    let url = url::Url::parse(&format!("https://epub.invalid/{base}"))
        .map_err(|e| e.to_string())?
        .join(href)
        .map_err(|e| e.to_string())?;
    if url.host_str() != Some("epub.invalid") {
        return Err("非 EPUB 内部路径".into());
    }
    Ok(unescape(url.path().trim_start_matches('/')))
}
pub fn file(store: &Store, key: &NovelKey) -> std::path::PathBuf {
    store
        .cache_path("epubs", &key.novel_id)
        .with_extension("epub")
}
pub fn index(store: &Store, key: &NovelKey) -> Result<Book, String> {
    let mut book: Book = store
        .read_cache("epub-index", key)
        .ok_or_else(|| "本地 EPUB 索引不存在".to_string())?;
    if !book.plain_text && book.index_version < 3 {
        let mut archive = Archive::cached(&file(store, key))?;
        book = parse_archive(&mut archive, &book.title)?;
        store.write_cache("epub-index", key, &book)?;
    }
    book.title = metadata_text(&book.title);
    for chapter in &mut book.chapters {
        chapter.title = metadata_text(&chapter.title);
    }
    Ok(book)
}

fn metadata_text(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}
pub fn import(
    store: &Store,
    identity: &impl Serialize,
    bytes: &[u8],
    fallback: &str,
) -> Result<NovelKey, String> {
    let key = NovelKey {
        provider_id: "epub".into(),
        novel_id: digest(identity),
    };
    let path = file(store, &key);
    // 验证后才发布文件和索引，失败不会覆盖可用的旧卷。
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let temporary = path.with_extension(format!(
        "download-{}-{}",
        std::process::id(),
        SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    crate::store::atomic_write(&temporary, bytes, false).map_err(|e| e.to_string())?;
    let result = (|| {
        if fallback.to_ascii_lowercase().ends_with(".txt") && !bytes.starts_with(b"PK") {
            let text = std::str::from_utf8(bytes).map_err(|_| "TXT 文件不是 UTF-8")?;
            if text.trim_start().starts_with("<!DOCTYPE html")
                || text.trim_start().starts_with("<html")
            {
                return Err("下载返回了网页而非小说".into());
            }
            let book = Book {
                title: fallback.into(),
                chapters: vec![EpubChapter {
                    path: "text".into(),
                    title: fallback.into(),
                }],
                plain_text: true,
                navigation: vec![],
                index_version: 3,
                spine_len: 1,
                cover: None,
                authors: vec![],
            };
            std::fs::rename(&temporary, &path).map_err(|e| e.to_string())?;
            store.write_cache("epub-index", &key, &book)?;
            return Ok(key);
        }
        let mut archive = Archive::open(&temporary)?;
        let book = parse_archive(&mut archive, fallback)?;
        drop(archive);
        std::fs::rename(&temporary, &path).map_err(|e| e.to_string())?;
        store.write_cache("epub-index", &key, &book)?;
        Ok(key)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}
pub fn novel(store: &Store, key: &NovelKey) -> Result<Novel, String> {
    let book = index(store, key)?;
    let toc: Vec<_> = if book.navigation.is_empty() {
        book.chapters
            .iter()
            .enumerate()
            .map(|(i, c)| serde_json::json!({"titleJp":c.title,"chapterId":i.to_string()}))
            .collect()
    } else {
        book.navigation.iter().map(|n|serde_json::json!({"titleJp":format!("{}{}","　".repeat(n.depth.min(5)),n.title),"chapterId":n.chapter_id})).collect()
    };
    serde_json::from_value(serde_json::json!({"titleJp":book.title,"authors":book.authors.iter().map(|a| serde_json::json!({"name":a})).collect::<Vec<_>>(),"attentions":[],"keywords":[],"introductionJp":"已下载的文库卷 · EPUB 本地阅读","glossary":{},"toc":toc,"visited":0,"syncAt":0,"jp":toc.len(),"youdao":0,"gpt":0,"sakura":0})).map_err(|e|e.to_string())
}
pub fn chapter(store: &Store, key: &NovelKey, id: &str) -> Result<Chapter, String> {
    let book = index(store, key)?;
    chapter_from(&book, id)
}
fn chapter_from(book: &Book, id: &str) -> Result<Chapter, String> {
    let i = id
        .split('#')
        .next()
        .unwrap_or(id)
        .parse::<usize>()
        .map_err(|_| "EPUB 章节无效")?;
    let c = book.chapters.get(i).ok_or("EPUB 章节越界")?;
    let title = book
        .navigation
        .iter()
        .find(|n| n.chapter_id == id)
        .map(|n| n.title.as_str())
        .unwrap_or(&c.title);
    let last = if i < book.spine_len {
        book.spine_len
    } else {
        book.chapters.len()
    };
    serde_json::from_value(serde_json::json!({"titleJp":title,"novelTitleJp":book.title,"paragraphs":[],"prevId":i.checked_sub(1).map(|n|n.to_string()),"nextId":(i+1<last).then(||(i+1).to_string())})).map_err(|e|e.to_string())
}
pub fn load_chapter(
    store: &Store,
    key: &NovelKey,
    id: &str,
) -> Result<(Chapter, kn_text::Document), String> {
    let book = index(store, key)?;
    Ok((
        chapter_from(&book, id)?,
        document_from(store, key, id, &book)?,
    ))
}
pub fn document(store: &Store, key: &NovelKey, id: &str) -> Result<kn_text::Document, String> {
    document_from(store, key, id, &index(store, key)?)
}
fn document_from(
    store: &Store,
    key: &NovelKey,
    id: &str,
    book: &Book,
) -> Result<kn_text::Document, String> {
    let i = id
        .split('#')
        .next()
        .unwrap_or(id)
        .parse::<usize>()
        .map_err(|_| "EPUB 章节无效")?;
    let chapter = book.chapters.get(i).ok_or("EPUB 章节越界")?;
    if book.plain_text {
        let bytes = std::fs::read(file(store, key)).map_err(|e| e.to_string())?;
        let text = String::from_utf8(bytes).map_err(|e| e.to_string())?;
        return Ok(kn_text::Document {
            blocks: text
                .lines()
                .enumerate()
                .map(|(i, s)| kn_text::Block {
                    text: s.into(),
                    path: format!("/p/{i}"),
                    ..Default::default()
                })
                .collect(),
            notes: vec![],
        });
    }
    let mut archive = Archive::cached(&file(store, key))?;
    let html = archive.text(&chapter.path)?;
    let source = store.read_cache::<serde_json::Value>("epub-source", key);
    let original_only = source
        .as_ref()
        .is_some_and(|s| s["translated"] == false && s["original"] == true && s["human"] == false);
    let html = markup::prepare(&mut archive, &chapter.path, &html, original_only)?;
    let base = format!(
        "https://epub.invalid/{}",
        chapter
            .path
            .split('/')
            .map(crate::api::encode)
            .collect::<Vec<_>>()
            .join("/")
    );
    let mut doc = kn_text::extract_document(&html, &base, true);
    // prepare resolves links while the original relative/root-relative distinction still exists.
    let resource = |src: &str| -> String {
        if let Some(path) = src.strip_prefix("https://epub.invalid/") {
            format!(
                "epub://{}/{}",
                key.novel_id,
                crate::api::encode(&unescape(path))
            )
        } else {
            src.into()
        }
    };
    let target = id.split_once('#').map(|(_, f)| unescape(f));
    let anchor = target
        .as_ref()
        .map(|f| format!("//*[@id=\"{}\"]", f.replace('"', "\\\"")));
    let mut found = false;
    for (i, b) in doc.blocks.iter_mut().enumerate() {
        let is_target = !found
            && anchor
                .as_ref()
                .is_some_and(|a| b.path == *a || b.path.starts_with(&format!("{a}/")));
        b.path = if is_target {
            found = true;
            format!("/epub-anchor/{}", target.as_ref().unwrap())
        } else {
            format!("/epub/{i}")
        };
        if b.kind == kn_text::BlockKind::Image {
            b.source_url = resource(&b.source_url);
        }
        for (_, obj) in &mut b.objects {
            if let kn_text::Inline::Image { url, .. } = obj {
                *url = resource(url);
            }
        }
    }
    if target.is_some() && !found {
        return Err("EPUB 目录锚点无法定位".into());
    }
    Ok(doc)
}
pub fn image(store: &Store, url: &str) -> Result<Vec<u8>, String> {
    let rest = url.strip_prefix("epub://").ok_or("插图协议无效")?;
    let (id, name) = rest.split_once('/').ok_or("插图路径无效")?;
    Archive::cached(&file(
        store,
        &NovelKey {
            provider_id: "epub".into(),
            novel_id: id.into(),
        },
    ))?
    .read(&unescape(name), 16 * 1024 * 1024)
}

fn parse_archive(archive: &mut Archive, fallback: &str) -> Result<Book, String> {
    let container = archive.text("META-INF/container.xml")?;
    let doc = xml(&container).map_err(|e| format!("EPUB container: {e}"))?;
    let opf = doc
        .descendants()
        .find(|n| n.has_tag_name("rootfile"))
        .and_then(|n| n.attribute("full-path"))
        .ok_or("EPUB 缺少 rootfile")?;
    let package_text = archive.text(opf)?;
    let package = xml(&package_text).map_err(|e| format!("EPUB OPF: {e}"))?;
    let title = metadata_text(
        package
            .descendants()
            .find(|n| n.tag_name().name() == "title")
            .and_then(|n| n.text())
            .unwrap_or(fallback),
    );
    let authors = package
        .descendants()
        .filter(|n| n.tag_name().name() == "creator")
        .filter_map(|n| n.text())
        .map(str::to_owned)
        .collect();
    let cover_id = package
        .descendants()
        .find(|n| n.has_tag_name("meta") && n.attribute("name") == Some("cover"))
        .and_then(|n| n.attribute("content"));
    let mut cover = None;
    let mut manifest = HashMap::new();
    let mut nav = None;
    let mut ncx = None;
    for item in package.descendants().filter(|n| n.has_tag_name("item")) {
        if let (Some(id), Some(href)) = (item.attribute("id"), item.attribute("href")) {
            // Remote optional manifest resources must not make a local spine unreadable.
            let Ok(name) = resolve(opf, href) else {
                continue;
            };
            if item
                .attribute("properties")
                .unwrap_or("")
                .split_whitespace()
                .any(|p| p == "nav")
            {
                nav = Some(name.clone());
            }
            if item.attribute("media-type") == Some("application/x-dtbncx+xml") {
                ncx = Some(name.clone());
            }
            if Some(id) == cover_id
                || item
                    .attribute("properties")
                    .unwrap_or("")
                    .split_whitespace()
                    .any(|s| s == "cover-image")
            {
                cover = Some(name.clone());
            }
            manifest.entry(id.to_owned()).or_insert(name);
        }
    }
    let mut labels = HashMap::new();
    let mut navigation_targets = Vec::new();
    for name in [nav, ncx].into_iter().flatten() {
        if !navigation_targets.is_empty() {
            break;
        }
        if let Ok(content) = archive.text(&name) {
            if let Ok(doc) = xml(&content) {
                let toc = doc
                    .descendants()
                    .find(|n| {
                        n.has_tag_name("nav")
                            && (n
                                .attribute(("http://www.idpf.org/2007/ops", "type"))
                                .unwrap_or("")
                                .split_whitespace()
                                .any(|t| t == "toc")
                                || n.attribute("role") == Some("doc-toc"))
                    })
                    .or_else(|| doc.descendants().find(|n| n.has_tag_name("nav")))
                    .or_else(|| doc.descendants().find(|n| n.has_tag_name("navMap")))
                    .unwrap_or(doc.root());
                for n in toc.descendants() {
                    if n.has_tag_name("a") {
                        if let Some(href) = n.attribute("href") {
                            if let Ok(p) = resolve(&name, href) {
                                let title = n
                                    .descendants()
                                    .filter(|t| t.is_text())
                                    .filter_map(|t| t.text())
                                    .collect::<String>();
                                navigation_targets.push((
                                    p.clone(),
                                    href.split_once('#').map(|(_, f)| unescape(f)),
                                    metadata_text(&title),
                                    n.ancestors()
                                        .filter(|n| n.has_tag_name("ol"))
                                        .count()
                                        .saturating_sub(1),
                                ));
                                labels.entry(p).or_insert(title);
                            }
                        }
                    }
                    if n.has_tag_name("navPoint") {
                        let href = n
                            .children()
                            .find(|t| t.has_tag_name("content"))
                            .and_then(|t| t.attribute("src"));
                        let label = n.children().find(|t| t.has_tag_name("navLabel")).map(|t| {
                            t.descendants()
                                .filter(|c| c.is_text())
                                .filter_map(|c| c.text())
                                .collect::<String>()
                        });
                        if let (Some(h), Some(l)) = (href, label) {
                            if let Ok(p) = resolve(&name, h) {
                                navigation_targets.push((
                                    p.clone(),
                                    h.split_once('#').map(|(_, f)| unescape(f)),
                                    metadata_text(&l),
                                    n.ancestors()
                                        .skip(1)
                                        .filter(|n| n.has_tag_name("navPoint"))
                                        .count(),
                                ));
                                labels.entry(p).or_insert(l);
                            }
                        }
                    }
                }
            }
        }
    }
    let mut chapters: Vec<_> = package
        .descendants()
        .filter(|n| n.has_tag_name("itemref") && n.attribute("linear") != Some("no"))
        .filter_map(|n| n.attribute("idref"))
        .filter_map(|id| manifest.get(id))
        .enumerate()
        .map(|(i, p)| EpubChapter {
            path: p.clone(),
            title: labels
                .get(p)
                .cloned()
                .unwrap_or_else(|| format!("第 {} 节", i + 1)),
        })
        .collect();
    if chapters.is_empty() {
        return Err("EPUB 没有可阅读的 spine".into());
    }
    let spine_len = chapters.len();
    let mut navigation = Vec::new();
    let guide_cover = package
        .descendants()
        .find(|n| n.has_tag_name("reference") && n.attribute("type") == Some("cover"))
        .and_then(|n| n.attribute("href"))
        .and_then(|h| resolve(opf, h).ok());
    for (mut path, fragment, title, depth) in navigation_targets {
        if !chapters.iter().any(|c| c.path == path)
            && matches!(title.trim(), "表紙" | "封面" | "Cover" | "cover")
        {
            if let Some(valid) = guide_cover
                .as_ref()
                .filter(|p| chapters.iter().any(|c| c.path == **p))
            {
                path = valid.clone();
            }
        }
        if title.is_empty() {
            continue;
        }
        let index = if let Some(i) = chapters.iter().position(|c| c.path == path) {
            i
        } else {
            // Navigation-only front matter is reachable without changing existing spine IDs.
            if archive.zip.as_mut().unwrap().by_name(&path).is_err() {
                continue;
            }
            chapters.push(EpubChapter {
                path,
                title: title.clone(),
            });
            chapters.len() - 1
        };
        let id = match fragment.filter(|f| !f.is_empty()) {
            Some(f) => format!("{index}#{}", crate::api::encode(&f)),
            None => index.to_string(),
        };
        if !navigation.iter().any(|n: &Navigation| n.chapter_id == id) {
            navigation.push(Navigation {
                title,
                chapter_id: id,
                depth,
            });
        }
    }
    let book = Book {
        title,
        chapters,
        plain_text: false,
        navigation,
        index_version: 3,
        spine_len,
        cover,
        authors,
    };

    Ok(book)
}
