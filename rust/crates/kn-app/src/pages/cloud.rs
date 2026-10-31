//! AutoNovel 读者功能：复用 KinNovel 列表/分页/键盘，页面只在工作池中请求网络。
use super::*;
use crate::{api::encode, model::NovelKey, repository::Repository, store::Config, Kindlia};
use kn_platform::{GestureKind, InputEvent};
use kn_render::{Bitmap, Point, Rect};
use kn_ui::{
    keyboard::{EditorEvent, TextEditor},
    Cx, Page, PageId, RefreshHint, Transition,
};
use serde_json::{json, Value};
use std::any::Any;
const WENKU_LEVELS: [&str; 7] = [
    "全部小说",
    "轻小说",
    "轻文学",
    "文学",
    "非小说",
    "R18男性向",
    "R18女性向",
];

#[derive(Clone)]
pub enum Mode {
    Wenku(String, u8),
    WenkuBook(String),
    Folders(bool, Option<NovelKey>),
    Favorites(bool, String),
    History,
    Rank(String, String),
    Comments(String, Option<u64>),
    Comment(String, Value),
}
#[derive(Clone)]
enum Action {
    Web(NovelKey),
    Wenku(String),
    Folder(String),
    Volume(String, String, bool),
    Comment(Value),
    Replies(u64),
    Info(Value),
    None,
}
#[derive(Clone)]
struct Item {
    title: String,
    subtitle: String,
    action: Action,
    cover: String,
}
struct ResultMessage(u64, Result<(Vec<Item>, usize, String, usize), String>);
/// 缓存可立即操作，在线更新仍用同一个 generation 防止覆盖新筛选/翻页/修改。
struct CachedResult(ResultMessage);
struct Mutation(Result<(), String>);
struct EpubResult(Result<NovelKey, String>);
struct CoverResult(String, Result<Bitmap, String>);

pub struct CloudPage {
    mode: Mode,
    book_catalog: bool,
    items: Vec<Item>,
    page: usize,
    pages: usize,
    rows: usize,
    loading: bool,
    generation: u64,
    notice: String,
    rank_type: usize,
    rank_genre: usize,
    rank_status: usize,
    rank_remote: usize,
    rank_source_pages: usize,
    editor: TextEditor,
    editing: bool,
    searching: bool,
    return_on_close: bool,
    choice: Option<(u8, choice::Choice)>,
    edit_kind: u8,
    pending: Option<Transition<Kindlia>>,
    covers: std::collections::HashMap<String, Option<Bitmap>>,
}
fn string(v: &Value, k: &str) -> String {
    v[k].as_str().unwrap_or("").to_owned()
}
fn title(v: &Value) -> String {
    let zh = string(v, "titleZh");
    if zh.is_empty() {
        let jp = string(v, "titleJp");
        if jp.is_empty() {
            string(v, "title")
        } else {
            jp
        }
    } else {
        zh
    }
}
impl CloudPage {
    pub fn wenku_search(query: String) -> Self {
        let mut page = Self::new(Mode::Wenku(query, 0));
        page.searching = true;
        page.return_on_close = true;
        page
    }
    pub fn rank(provider: &str, kind: usize) -> Self {
        Self {
            rank_type: kind,
            ..Self::new(Mode::Rank(provider.into(), "总计".into()))
        }
    }
    fn rank_choices(&self, field: u8) -> (&'static str, Vec<String>, usize) {
        let Mode::Rank(p, r) = &self.mode else {
            return ("", vec![], 0);
        };
        let (_, genres, status) = rank_options(p, self.rank_type);
        match field {
            0 => (
                "流派",
                genres.iter().map(|s| s.to_string()).collect(),
                self.rank_genre,
            ),
            1 => {
                let ranges: &[&str] = if p == "syosetu" {
                    &["总计", "每年", "季度", "每月", "每周", "每日"]
                } else {
                    &["总计", "每年", "每月", "每周", "每日"]
                };
                (
                    "范围",
                    ranges.iter().map(|s| s.to_string()).collect(),
                    ranges.iter().position(|s| *s == r).unwrap_or(0),
                )
            }
            _ => (
                "状态",
                status.iter().map(|s| s.to_string()).collect(),
                self.rank_status,
            ),
        }
    }
    fn tab(&self) -> Option<usize> {
        match self.mode {
            Mode::Wenku(..) | Mode::Rank(..) => Some(2),
            Mode::Folders(..) | Mode::Favorites(..) => Some(0),
            Mode::History => Some(1),
            _ => None,
        }
    }

    pub fn new(mode: Mode) -> Self {
        Self {
            mode,
            book_catalog: false,
            items: vec![],
            page: 0,
            pages: 1,
            rows: 6,
            loading: false,
            generation: 0,
            notice: String::new(),
            rank_type: 0,
            rank_genre: 0,
            rank_status: 0,
            rank_remote: 0,
            rank_source_pages: 1,
            editor: TextEditor::new(1000, ""),
            editing: false,
            searching: false,
            return_on_close: false,
            choice: None,
            edit_kind: 0,
            pending: None,
            covers: std::collections::HashMap::new(),
        }
    }
    fn name(&self) -> &str {
        match self.mode {
            Mode::Wenku(..) => "文库小说",
            Mode::WenkuBook(_) => "文库详情与卷数",
            Mode::Folders(..) => "收藏夹",
            Mode::Favorites(..) => "我的收藏",
            Mode::History => "历史",
            Mode::Rank(ref p, _) => {
                if p == "kakuyomu" {
                    "Kakuyomu：流派"
                } else {
                    [
                        "成为小说家：综合",
                        "成为小说家：流派",
                        "成为小说家：异世界转移/转生",
                    ][self.rank_type % 3]
                }
            }
            Mode::Comments(..) => "书籍评论",
            Mode::Comment(..) => "评论与回复",
        }
    }
    fn load(&mut self, cx: &mut Cx<Kindlia>) {
        self.load_with_cache(cx, true);
    }
    fn load_with_cache(&mut self, cx: &mut Cx<Kindlia>, cache_first: bool) {
        if self.loading {
            return;
        }
        if matches!(self.mode, Mode::Wenku(..)) {
            let m = cx.app.metrics;
            let controls = if self.searching {
                m.touch + m.touch / 4
            } else {
                m.touch
            };
            self.rows = (cx
                .height
                .saturating_sub(m.header_h() + controls + m.pager_h() + m.tab_bar_h())
                / m.row_h_cover())
            .max(1) as usize;
        }
        self.loading = true;
        self.notice = "加载中…".into();
        self.items.clear();
        if self.covers.len() > 32 {
            self.covers.clear();
        }
        let mut repo = cx.app.repo.clone();
        repo.api = repo.api.account_snapshot();
        let mode = self.mode.clone();
        let page = self.page;
        let rows = self.rows;
        let rank = (
            self.rank_type,
            self.rank_genre,
            self.rank_status,
            self.rank_remote,
        );
        self.generation += 1;
        let generation = self.generation;
        let poster = cx.page_poster();
        let spawner = cx.spawner();
        cx.spawn_local_droppable(move || {
            if repo.offline || cache_first {
                let mut cached = repo.clone();
                cached.offline = true;
                let result = fetch(&cached, &mode, page, rows, rank);
                if repo.offline {
                    poster.post(ResultMessage(generation, result));
                    return;
                }
                if result.is_ok() {
                    poster.post(CachedResult(ResultMessage(generation, result)));
                }
            }
            if let Some(spawner) = spawner {
                spawner.spawn_on(kn_ui::Lane::Net, true, move || {
                    ResultMessage(generation, fetch(&repo, &mode, page, rows, rank))
                });
            }
        });
        cx.request_redraw(RefreshHint::Ui);
    }
    fn mutate(&mut self, cx: &mut Cx<Kindlia>, method: &str, path: String, body: Value) {
        if self.loading {
            return;
        }
        if cx.app.repo.offline {
            self.notice = "离线模式不可修改云端数据".into();
            return;
        }
        self.loading = true;
        self.generation += 1;
        let api = cx.app.repo.api.account_snapshot();
        let method = method.to_owned();
        let store = cx.app.repo.store.clone();
        cx.spawn(move || {
            Mutation(if method == "DELETE" && path == "user/read-history" {
                crate::history::clear_remote(&api, &store)
            } else {
                api.mutate(&method, &path, &body)
            })
        });
        cx.request_redraw(RefreshHint::Ui);
    }
    fn editor(&mut self, text: &str, kind: u8) {
        self.editor.set_max_chars(
            if matches!(self.mode, Mode::Comments(..) | Mode::Comment(..)) {
                3000
            } else {
                256
            },
        );
        self.editor.set_text(text);
        self.editing = true;
        self.edit_kind = kind;
    }
    fn submit(&mut self, cx: &mut Cx<Kindlia>) {
        let text = self.editor.text();
        self.editing = false;
        if text.trim().is_empty() {
            return;
        }
        match &self.mode.clone() {
            Mode::Wenku(_, level) => {
                self.mode = Mode::Wenku(text, *level);
                self.page = 0;
                self.load(cx);
            }
            Mode::Folders(wenku, _) => {
                let section = if *wenku { "wenku" } else { "web" };
                if self.edit_kind == 2 {
                    if let Some(Item {
                        action: Action::Folder(id),
                        ..
                    }) = self.items.get(self.page * self.rows)
                    {
                        self.mutate(
                            cx,
                            "PUT",
                            format!("user/favored-{section}/{}", encode(id)),
                            json!({"title":text}),
                        );
                    }
                } else {
                    self.mutate(
                        cx,
                        "POST",
                        format!("user/favored-{section}"),
                        json!({"title":text}),
                    );
                }
            }
            Mode::Comments(subject, root) => {
                let mut body = json!({"content":text});
                if let Some(root) = root {
                    body["rootId"] = json!(root);
                }
                self.mutate(
                    cx,
                    "POST",
                    format!("forum:external/comment/novel/{}", encode(subject)),
                    body,
                );
            }
            Mode::Comment(subject, comment) => {
                if self.edit_kind == 3 {
                    self.mutate(
                        cx,
                        "PATCH",
                        format!("forum:external/comment/novel/{}", comment["id"]),
                        json!({"content":text}),
                    );
                } else {
                    let root = comment["rootId"].as_u64().or(comment["id"].as_u64());
                    self.mutate(
                        cx,
                        "POST",
                        format!("forum:external/comment/novel/{}", encode(subject)),
                        json!({"content":text,"rootId":root}),
                    );
                }
            }
            _ => {}
        }
    }
}
pub fn open_volume(
    repo: &Repository,
    config: &Config,
    novel: &str,
    volume: &str,
    human: bool,
) -> Result<NovelKey, String> {
    let identity = (
        novel,
        volume,
        human,
        &config.translation,
        config.show_original,
        config.show_translation,
    );
    let key = NovelKey {
        provider_id: "epub".into(),
        novel_id: crate::model::digest(&identity),
    };
    if crate::epub::index(&repo.store, &key).is_ok()
        && crate::epub::file(&repo.store, &key).is_file()
    {
        return Ok(key);
    }
    if repo.offline {
        return Err("此卷尚未下载，无法离线阅读".into());
    }
    let path = if human {
        format!("/files-wenku/{}/{}", encode(novel), encode(volume))
    } else {
        let mode = if config.show_original { "zh-jp" } else { "zh" };
        let priority = if config.translation == "auto" {
            vec!["sakura", "gpt", "youdao"]
        } else {
            vec![config.translation.as_str()]
        };
        let qs = priority
            .iter()
            .filter(|s| **s != "baidu")
            .map(|s| format!("translations={}", encode(s)))
            .collect::<Vec<_>>()
            .join("&");
        if qs.is_empty() {
            return Err("文库源站不支持百度译文，请选择有道/GPT/Sakura".into());
        }
        format!(
            "/api/wenku/{}/file/{}?mode={mode}&translationsMode=priority&{qs}&filename={}",
            encode(novel),
            encode(volume),
            encode(&format!("{volume}.epub"))
        )
    };
    eprintln!("[wenku] download novel={novel} volume={volume} human={human}");
    let mut attempt = 0;
    let bytes = loop {
        let bytes = repo.api.download(&path).map_err(|e| {
            eprintln!("[wenku] download failed: {e}");
            format!("文库下载：{e}")
        })?;
        if let Some(error) = crate::epub::incomplete_download(&bytes) {
            eprintln!("[wenku] incomplete ZIP ({} bytes), attempt {}: {error}", bytes.len(), attempt + 1);
            if attempt == 2 {
                return Err(format!("文库下载文件不完整，重试后仍无效：{error}"));
            }
            // Start again at the generation endpoint, not the redirected file
            // URL: its previous HTTP body may have ended while still growing.
            drop(bytes);
            std::thread::sleep(std::time::Duration::from_secs(1 << attempt));
            attempt += 1;
            continue;
        }
        break bytes;
    };
    eprintln!("[wenku] downloaded {} bytes; parsing EPUB", bytes.len());
    let key = crate::epub::import(&repo.store, &identity, &bytes, volume).map_err(|e| {
        eprintln!("[wenku] import failed: {e}");
        format!("文库解析：{e}")
    })?;
    repo.store.write_cache("epub-source",&key,&json!({"novel":novel,"volume":volume,"human":human,"translation":config.translation,"original":config.show_original,"translated":config.show_translation}))?;
    Ok(key)
}
fn fetch(
    repo: &Repository,
    mode: &Mode,
    page: usize,
    size: usize,
    rank: (usize, usize, usize, usize),
) -> Result<(Vec<Item>, usize, String, usize), String> {
    let mut items = vec![];
    let mut pages = 1;
    let mut notice = String::new();
    let path = match mode {
        Mode::Wenku(query, level) => format!(
            "wenku?page={page}&pageSize={size}&level={level}&query={}",
            encode(query)
        ),
        Mode::WenkuBook(id) => format!("wenku/{}", encode(id)),
        Mode::Folders(..) => "user/favored".into(),
        Mode::Favorites(wenku, id) => format!(
            "user/favored-{}/{}?page={page}&pageSize={size}&sort=update&level=0&provider={}",
            if *wenku { "wenku" } else { "web" },
            encode(id),
            encode(crate::api::DEFAULT_PROVIDERS)
        ),
        Mode::History => format!("user/read-history?page={page}&pageSize={size}"),
        Mode::Rank(provider, range) => {
            let (kind, genre, status, source_page) = rank;
            let (types, genres, statuses) = rank_options(provider, kind);
            let mut path = format!(
                "novel/rank/{}?range={}&status={}",
                encode(provider),
                encode(range),
                encode(statuses[status % statuses.len()])
            );
            if provider == "syosetu" {
                path.push_str(&format!(
                    "&type={}&page={}",
                    encode(types[kind % types.len()]),
                    source_page + 1
                ));
            }
            if !genres.is_empty() {
                path.push_str(&format!("&genre={}", encode(genres[genre % genres.len()])));
            }
            path
        }
        Mode::Comments(subject, root) => {
            if let Some(root) = root {
                format!(
                    "forum:external/comment/novel/{}/{root}/reply?page={}&page_size={size}",
                    encode(subject),
                    page + 1
                )
            } else {
                format!(
                    "forum:external/comment/novel/{}?page={}&page_size={size}",
                    encode(subject),
                    page + 1
                )
            }
        }
        Mode::Comment(_, c) => {
            return Ok((
                vec![
                    Item {
                        title: "阅读全文".into(),
                        subtitle: string(c, "content"),
                        action: Action::None,
                        cover: String::new(),
                    },
                    Item {
                        title: format!("查看全部回复 ({})", c["replyCount"].as_u64().unwrap_or(0)),
                        subtitle: String::new(),
                        action: Action::Replies(
                            c["rootId"].as_u64().or(c["id"].as_u64()).unwrap_or(0),
                        ),
                        cover: String::new(),
                    },
                ],
                1,
                format!(
                    "{} · {}",
                    string(c, "authorUsername"),
                    string(c, "createdAt")
                ),
                1,
            ));
        }
    };
    // 私有列表按账号分区，退出账号不会将上个账号的数据展示给游客。
    let cache_key = (&path, repo.api.username());
    let v: Value = if repo.offline {
        repo.store
            .read_cache("cloud", &cache_key)
            .ok_or("离线状态，没有缓存此列表")?
    } else {
        let v: Value = repo.api.get(&path)?;
        repo.store.write_cache("cloud", &cache_key, &v)?;
        v
    };
    let source_pages = v["pageNumber"].as_u64().unwrap_or(1).max(1) as usize;
    match mode {
        Mode::Folders(wenku, _) => {
            for f in v[if *wenku { "favoredWenku" } else { "favoredWeb" }]
                .as_array()
                .ok_or("收藏夹响应格式无效")?
            {
                items.push(Item {
                    title: string(f, "title"),
                    subtitle: "收藏夹 · 长按管理".into(),
                    action: Action::Folder(string(f, "id")),
                    cover: String::new(),
                });
            }
        }
        Mode::WenkuBook(id) => {
            items.push(Item {
                title: "简介与书籍信息".into(),
                subtitle: string(&v, "introduction"),
                action: Action::Info(v.clone()),
                cover: string(&v, "cover"),
            });
            for vol in v["volumeZh"].as_array().into_iter().flatten() {
                if let Some(name) = vol.as_str() {
                    items.push(Item {
                        title: name.into(),
                        subtitle: "中文 EPUB · 点按下载阅读".into(),
                        action: Action::Volume(id.clone(), name.into(), true),
                        cover: String::new(),
                    });
                }
            }
            for vol in v["volumeJp"].as_array().into_iter().flatten() {
                let name = string(vol, "volumeId");
                items.push(Item {
                    title: name.clone(),
                    subtitle: format!(
                        "日文卷 · 有道 {} / GPT {} / Sakura {}",
                        vol["youdao"], vol["gpt"], vol["sakura"]
                    ),
                    action: Action::Volume(id.clone(), name, false),
                    cover: String::new(),
                });
            }
            pages = items.len().div_ceil(size).max(1);
        }
        Mode::Comments(..) => {
            pages = v["total"]
                .as_u64()
                .unwrap_or(0)
                .div_ceil(size as u64)
                .max(1) as usize;
            for c in v["items"].as_array().ok_or("评论响应格式无效")? {
                items.push(Item {
                    title: format!(
                        "{} · {} 回复",
                        string(c, "authorUsername"),
                        c["replyCount"].as_u64().unwrap_or(0)
                    ),
                    subtitle: if c["status"] == 0 {
                        crate::comment_format::preview(&string(c, "content"))
                    } else {
                        "评论已隐藏或删除".into()
                    },
                    action: Action::Comment(c.clone()),
                    cover: String::new(),
                });
            }
        }
        _ => {
            pages = v["pageNumber"].as_u64().unwrap_or(1).max(1) as usize;
            for b in v["items"].as_array().ok_or("列表响应格式无效")? {
                let action = if b["providerId"].is_string() {
                    Action::Web(NovelKey {
                        provider_id: string(b, "providerId"),
                        novel_id: string(b, "novelId"),
                    })
                } else {
                    Action::Wenku(string(b, "id"))
                };
                if let Action::Wenku(id) = &action {
                    crate::covers::remember(
                        repo,
                        &NovelKey {
                            provider_id: "wenku".into(),
                            novel_id: id.clone(),
                        },
                        b["authors"]
                            .as_array()
                            .map(|a| {
                                a.iter()
                                    .filter_map(Value::as_str)
                                    .collect::<Vec<_>>()
                                    .join(" / ")
                            })
                            .unwrap_or_default(),
                        string(b, "cover"),
                    );
                }
                let mut authors=crate::formatting::authors(b);
                if authors.is_empty() {
                    match &action {
                        Action::Web(key) => {if let Some(n)=repo.store.read_cache::<crate::model::Novel>("novels",key) {authors=n.authors.into_iter().map(|a|a.name).collect();}},
                        Action::Wenku(id) => {let meta=crate::covers::metadata(repo,&NovelKey {provider_id:"wenku".into(),novel_id:id.clone()},false);if !meta.author.is_empty(){authors.push(meta.author);}},
                        _=>{}
                    }
                }
                items.push(Item {
                    title: title(b),
                    subtitle: if authors.is_empty(){string(b,"providerId")}else{authors.join(" / ")},
                    action,
                    cover: b["cover"]
                        .as_str()
                        .filter(|s| !s.is_empty())
                        .unwrap_or(crate::covers::DEFAULT_URL)
                        .into(),
                });
            }
        }
    }
    if matches!(mode, Mode::Rank(..)) {
        notice = format!("来源第 {} 页 · 共 {} 本", rank.3 + 1, items.len());
        pages = items.len().div_ceil(size).max(1);
    }
    if matches!(mode, Mode::Folders(..)) {
        pages = items.len().div_ceil(size).max(1);
    }
    if items.is_empty() {
        notice = "这里还没有内容".into();
    }
    Ok((items, pages, notice, source_pages))
}
impl Page<Kindlia> for CloudPage {
    fn id(&self) -> PageId {
        PageId(50)
    }
    fn enter(&mut self, cx: &mut Cx<Kindlia>, _: bool) {
        let m = cx.app.metrics;
        let controls = match self.mode {
            Mode::Favorites(..) => 0,
            Mode::Rank(..) => m.touch + m.touch / 4,
            Mode::History | Mode::Wenku(..) => m.touch,
            _ => m.touch * 2,
        };
        self.rows = (cx.height.saturating_sub(
            m.header_h()
                + controls
                + m.pager_h()
                + if self.tab().is_some() {
                    m.tab_bar_h()
                } else {
                    0
                },
        ) / m.row_h_cover())
        .max(1) as usize;
        self.load(cx);
    }
    fn wants_backdrop(&self) -> kn_ui::Backdrop {
        self.choice
            .as_ref()
            .map(|(_, p)| p)
            .map(|p| p.wants_backdrop())
            .unwrap_or(kn_ui::Backdrop::None)
    }
    fn set_backdrop(&mut self, bg: Option<Bitmap>) {
        if let Some(p) = self.choice.as_mut().map(|(_, p)| p) {
            p.set_backdrop(bg);
        }
    }
    fn render(&mut self, cx: &mut Cx<Kindlia>, frame: &mut Bitmap) {
        if let Some((_, picker)) = &mut self.choice {
            picker.render(cx, frame);
            return;
        }
        if matches!(self.mode, Mode::WenkuBook(_)) && !self.book_catalog {
            if let Some(item) = self.items.first().cloned() {
                if let Action::Info(v) = item.action {
                    let cover = v["cover"]
                        .as_str()
                        .filter(|s| !s.is_empty())
                        .unwrap_or(crate::covers::DEFAULT_URL)
                        .to_owned();
                    let size = detail::cover_rect(cx);
                    let cover_key = format!("{cover}:{}x{}", size.w, size.h);
                    if !self.covers.contains_key(&cover_key) {
                        self.covers.insert(cover_key.clone(), None);
                        let repo = cx.app.repo.clone();
                        let url = cover.clone();
                        let tag = cover_key.clone();
                        crate::images::spawn_fit(cx,repo,url,(size.w,size.h),move |r|CoverResult(tag,r.or_else(|_|crate::covers::fallback((size.w,size.h)))));
                    }
                    let author = v["authors"]
                        .as_array()
                        .map(|a| {
                            a.iter()
                                .filter_map(Value::as_str)
                                .collect::<Vec<_>>()
                                .join(" / ")
                        })
                        .unwrap_or_default();
                    let tags = v["keywords"]
                        .as_array()
                        .map(|a| {
                            a.iter()
                                .filter_map(Value::as_str)
                                .collect::<Vec<_>>()
                                .join("、")
                        })
                        .unwrap_or_default();
                    let volumes = self
                        .items
                        .iter()
                        .filter(|i| matches!(i.action, Action::Volume(..)))
                        .count();
                    let mut meta = vec![
                        format!("{} · {} 卷", string(&v, "level"), volumes),
                        format!("出版社：{}", string(&v, "publisher")),
                        format!("标签：{tags}"),
                    ];
                    // Publication date is distinct from Web chapter update time.
                    let published = crate::formatting::date(v["latestPublishAt"].as_i64());
                    if !published.is_empty() {
                        meta.insert(1, format!("最新出版于 {}", &published[..10]));
                    }
                    detail::render(
                        cx,
                        frame,
                        detail::Detail {
                            title: &title(&v),
                            author: &author,
                            meta: &meta,
                            intro: &string(&v, "introduction"),
                            notice: if self.loading {
                                "下载/加载中…"
                            } else {
                                &self.notice
                            },
                        },
                        self.covers.get(&cover_key).and_then(|b| b.as_ref()),
                        &[
                            ("选卷阅读", 40, !self.loading),
                            ("加入书架", 10, !self.loading),
                            ("评论", 11, !self.loading),
                        ],
                        41,
                        Some(12),
                    );
                    return;
                }
            }
            header(cx, frame, "书籍详情", true);
            let m = cx.app.metrics;
            let area = Rect::new(
                0,
                m.header_h() as i32,
                cx.width,
                cx.height.saturating_sub(m.header_h()),
            );
            let mut ink = Ink {
                fonts: cx.fonts,
                glyphs: cx.glyphs,
                chain: &cx.app.ui_fonts,
            };
            widgets::state_message(
                &mut ink,
                frame,
                cx.hits,
                &cx.theme,
                &m,
                area,
                &self.notice,
                if self.loading {
                    None
                } else {
                    Some(("重试", HitId(12)))
                },
            );
            return;
        }

        if matches!(self.mode, Mode::Wenku(..)) {
            search_header(
                cx,
                frame,
                if self.searching {
                    "搜索文库小说"
                } else {
                    "发现"
                },
                self.searching,
                10,
            );
        } else {
            header(
                cx,
                frame,
                if self.book_catalog && matches!(self.mode, Mode::WenkuBook(_)) {
                    "选择卷数"
                } else {
                    self.name()
                },
                !matches!(self.mode, Mode::History),
            );
        }
        let m = cx.app.metrics;
        let w = cx.width;
        let cell = w / 3;
        let y = m.header_h();
        let actions = match self.mode {
            Mode::Wenku(..) => ["搜索", "分级", "刷新"],
            Mode::WenkuBook(..) => ["云收藏", "书籍评论", "刷新"],
            Mode::Folders(..) => ["Web/文库", "新建收藏夹", "刷新"],
            Mode::History => ["暂停/恢复", "清空历史", "刷新"],
            Mode::Rank(..) => ["来源", "范围", "刷新"],
            Mode::Comments(..) => ["发表评论", "", ""],
            Mode::Comment(..) => ["回复", "编辑", "删除"],
            _ => ["收藏夹", "返回", "刷新"],
        };
        let list_top = if self.book_catalog && matches!(self.mode, Mode::WenkuBook(_)) {
            if self.loading || !self.notice.is_empty() {
                label(
                    cx,
                    frame,
                    Rect::new(0, y as i32, w, m.touch),
                    if self.loading {
                        "正在下载并解析…"
                    } else {
                        &self.notice
                    },
                    true,
                );
                y + m.touch
            } else {
                y
            }
        } else if matches!(self.mode, Mode::Wenku(..)) && self.searching {
            let level = if let Mode::Wenku(_, l) = self.mode {
                l as usize
            } else {
                0
            };
            option_button(
                cx,
                frame,
                Rect::new(m.margin as i32, y as i32, w - 2 * m.margin, m.touch),
                "分级",
                WENKU_LEVELS[level.min(6)],
                11,
            );
            y + m.touch + m.touch / 4
        } else if matches!(self.mode, Mode::Wenku(..)) {
            let mut ink = Ink {
                fonts: cx.fonts,
                glyphs: cx.glyphs,
                chain: &cx.app.ui_fonts,
            };
            widgets::segmented(
                &mut ink,
                frame,
                cx.hits,
                &cx.theme,
                &m,
                Rect::new(m.margin as i32, y as i32, w - 2 * m.margin, m.touch),
                &["Web小说", "文库小说", "排行"],
                1,
                60_000,
            );
            y + m.touch
        } else if matches!(self.mode, Mode::Comments(..)) {
            button(
                cx,
                frame,
                Rect::new(m.margin as i32, y as i32, w - 2 * m.margin, m.touch),
                "发表评论",
                10,
                !self.loading,
            );
            y + m.touch
        } else if matches!(self.mode, Mode::Favorites(..)) {
            y
        } else if matches!(self.mode, Mode::Rank(..)) {
            let fields: Vec<u8> = if let Mode::Rank(p, _) = &self.mode {
                if rank_options(p, self.rank_type).1.is_empty() {
                    vec![1, 2]
                } else {
                    vec![0, 1, 2]
                }
            } else {
                vec![]
            };
            let n = fields.len() as u32;
            let cell = w / n;
            let height = m.touch + m.touch / 4;
            for (i, field) in fields.iter().enumerate() {
                let (name, options, selected) = self.rank_choices(*field);
                let value = options.get(selected).map(String::as_str).unwrap_or("");
                let rect = Rect::new((i as u32 * cell) as i32, y as i32, cell, height).inflate(-5);
                let mut ink = Ink {
                    fonts: cx.fonts,
                    glyphs: cx.glyphs,
                    chain: &cx.app.ui_fonts,
                };
                frame.rounded_rect(
                    rect,
                    m.radius,
                    Some(cx.theme.background),
                    Some(cx.theme.foreground),
                    2,
                );
                ink.text_centered(
                    frame,
                    Rect::new(rect.x, rect.y, rect.w, rect.h * 3 / 5),
                    name,
                    m.body,
                    cx.theme.foreground,
                );
                let text = ink.fit(value, m.small, (rect.w - m.margin / 2) as f32);
                ink.text_centered(
                    frame,
                    Rect::new(
                        rect.x,
                        rect.y + (rect.h * 3 / 5) as i32,
                        rect.w,
                        rect.h * 2 / 5,
                    ),
                    &text,
                    m.small,
                    cx.theme.muted,
                );
                cx.hits
                    .add(HitId(80 + *field as u32), rect)
                    .enabled(!self.loading);
            }
            y + height
        } else if matches!(self.mode, Mode::History) {
            button(
                cx,
                frame,
                Rect::new(m.margin as i32, y as i32, w - 2 * m.margin, m.touch),
                "清除历史",
                11,
                !self.loading,
            );
            y + m.touch
        } else {
            for (i, t) in actions.iter().enumerate() {
                button(
                    cx,
                    frame,
                    Rect::new((i as u32 * cell) as i32, y as i32, cell, m.touch),
                    t,
                    10 + i as u32,
                    !self.loading,
                );
            }
            label(
                cx,
                frame,
                Rect::new(0, (y + m.touch) as i32, w, m.touch),
                &self.notice,
                true,
            );
            y + m.touch * 2
        };
        let volume_catalog = self.book_catalog && matches!(self.mode, Mode::WenkuBook(_));
        let row_height = if volume_catalog {
            m.row_h()
        } else {
            m.row_h_cover()
        };
        if volume_catalog {
            self.rows =
                (cx.height.saturating_sub(list_top + m.pager_h()) / row_height).max(1) as usize;
            self.pages = self
                .items
                .len()
                .saturating_sub(1)
                .div_ceil(self.rows)
                .max(1);
            self.page = self.page.min(self.pages - 1);
        }
        let offset = if matches!(
            self.mode,
            Mode::WenkuBook(_) | Mode::Folders(..) | Mode::Rank(..)
        ) {
            self.page * self.rows + usize::from(volume_catalog)
        } else {
            0
        };
        if matches!(
            self.mode,
            Mode::Favorites(..) | Mode::History | Mode::Rank(..)
        ) && (self.items.is_empty() || self.notice.starts_with("再次"))
        {
            label(
                cx,
                frame,
                Rect::new(0, (cx.height / 2) as i32, w, m.touch),
                &self.notice,
                true,
            );
        }
        if !self.editing {
            for (i, item) in self.items.iter().skip(offset).take(self.rows).enumerate() {
                let rect = Rect::new(0, (list_top + i as u32 * row_height) as i32, w, row_height);
                if !item.cover.is_empty() {
                    let size = kn_ui::widgets::cover_slot(&m, rect);
                    let cover_key = format!("{}:{}x{}", item.cover, size.w, size.h);
                    if !self.covers.contains_key(&cover_key) {
                        self.covers.insert(cover_key.clone(), None);
                        let url = item.cover.clone();
                        let repo = cx.app.repo.clone();
                        let tag = cover_key.clone();
                        crate::images::spawn_fit(cx,repo,url,(size.w,size.h),move |r|CoverResult(tag,r.or_else(|_|crate::covers::fallback((size.w,size.h)))));
                    }
                    let mut ink = kn_ui::widgets::Ink {
                        fonts: cx.fonts,
                        glyphs: cx.glyphs,
                        chain: &cx.app.ui_fonts,
                    };
                    kn_ui::widgets::list_row_cover(
                        &mut ink,
                        frame,
                        &cx.theme,
                        &m,
                        rect,
                        self.covers.get(&cover_key).and_then(|b| b.as_ref()),
                        &item.title,
                        &item.subtitle,
                        "",
                    );
                    cx.hits.add(kn_ui::HitId(200 + i as u32), rect).long_press();
                } else {
                    row(
                        cx,
                        frame,
                        rect,
                        &item.title,
                        &item.subtitle,
                        "",
                        Some(200 + i as u32),
                    );
                }
            }
            if let Mode::Rank(p, _) = &self.mode {
                if p == "syosetu" {
                    let rect = Rect::new(
                        0,
                        cx.height.saturating_sub(m.tab_bar_h() + m.pager_h()) as i32,
                        w,
                        m.pager_h(),
                    );
                    let mut ink = Ink {
                        fonts: cx.fonts,
                        glyphs: cx.glyphs,
                        chain: &cx.app.ui_fonts,
                    };
                    widgets::pager_with_label(
                        &mut ink,
                        frame,
                        cx.hits,
                        &cx.theme,
                        &m,
                        rect,
                        &format!("{}·{}/{}", self.rank_remote + 1, self.page + 1, self.pages),
                        self.page > 0 || self.rank_remote > 0,
                        self.page + 1 < self.pages || self.rank_remote + 1 < self.rank_source_pages,
                        HitId(2),
                        HitId(3),
                    );
                } else {
                    pager(cx, frame, self.page, self.pages, m.tab_bar_h());
                }
            } else {
                pager(
                    cx,
                    frame,
                    self.page,
                    self.pages,
                    if self.tab().is_some() {
                        m.tab_bar_h()
                    } else {
                        0
                    },
                );
            }
            if let Some(tab) = self.tab() {
                draw_tabs(cx, frame, tab);
            }
        } else {
            let h = TextEditor::height(&m);
            let mut ink = kn_ui::widgets::Ink {
                fonts: cx.fonts,
                glyphs: cx.glyphs,
                chain: &cx.app.ui_fonts,
            };
            self.editor.render(
                &mut ink,
                frame,
                cx.hits,
                &cx.theme,
                &m,
                Rect::new(0, cx.height.saturating_sub(h) as i32, w, h),
                "输入内容",
                "确定",
            );
        }
    }
    fn on_input(&mut self, cx: &mut Cx<Kindlia>, event: &InputEvent) -> Transition<Kindlia> {
        // 按下反馈由运行时负责；等完整手势再修改页面，避免按下/抬起各整页刷新一次。
        if matches!(event, InputEvent::Gesture(g) if g.kind == kn_platform::GestureKind::Down) {
            return Transition::None;
        }
        if let Some((kind, picker)) = &mut self.choice {
            match picker.input(cx, event) {
                choice::Event::Selected(n) => {
                    let kind = *kind;
                    self.choice = None;
                    match kind {
                        0 => {
                            if let Mode::Wenku(_, level) = &mut self.mode {
                                *level = n as u8;
                            }
                            self.page = 0;
                            if !self.editing {
                                self.load(cx);
                            }
                        }
                        1 => {
                            if let Some(Item {
                                action: Action::Info(v),
                                ..
                            }) = self.items.first()
                            {
                                if let Some(tag) = v["keywords"]
                                    .as_array()
                                    .and_then(|a| a.get(n))
                                    .and_then(Value::as_str)
                                {
                                    return Transition::Push(Box::new(Self::wenku_search(
                                        format!("{tag}$"),
                                    )));
                                }
                            }
                        }
                        2 => {
                            self.mode = Mode::Folders(n == 1, None);
                            self.page = 0;
                            self.load(cx);
                        }
                        field @ 10..=12 => {
                            let (_, options, _) = self.rank_choices(field - 10);
                            match field - 10 {
                                0 => self.rank_genre = n,
                                1 => {
                                    if let Mode::Rank(_, r) = &mut self.mode {
                                        *r = options[n].clone();
                                    }
                                }
                                _ => self.rank_status = n,
                            }
                            self.page = 0;
                            self.rank_remote = 0;
                            self.load(cx);
                        }
                        _ => {}
                    }
                    cx.request_redraw(RefreshHint::Clean);
                }
                choice::Event::Cancel => {
                    self.choice = None;
                    cx.request_redraw(RefreshHint::Clean);
                }
                _ => {}
            }
            return Transition::None;
        }
        if hit(cx, event) == Some(1) && self.searching {
            if self.return_on_close {
                return Transition::Back;
            }
            self.editing = false;
            self.searching = false;
            self.loading = false;
            self.generation += 1;
            if let Mode::Wenku(ref mut q, _) = self.mode {
                q.clear();
            }
            self.page = 0;
            self.load(cx);
            cx.request_redraw(RefreshHint::Ui);
            return Transition::None;
        }
        if hit(cx, event) == Some(18) {
            if let Some(Item {
                action: Action::Info(v),
                ..
            }) = self.items.first()
            {
                self.choice = Some((
                    1,
                    choice::Choice::new(
                        "选择标签搜索",
                        v["keywords"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter_map(Value::as_str),
                        None,
                    ),
                ));
                cx.request_redraw(RefreshHint::Ui);
            }
            return Transition::None;
        }

        if self.editing {
            if let InputEvent::Gesture(g) = event {
                if let Some(id) = cx
                    .hits
                    .at(Point {
                        x: g.end.0,
                        y: g.end.1,
                    })
                    .filter(|id| self.editor.owns(*id))
                {
                    if self.editor.on_gesture(id, g.kind) == EditorEvent::Submit {
                        self.submit(cx);
                    }
                    cx.request_redraw(RefreshHint::Ui);
                    return Transition::None;
                }
            }
        }
        if let Some(tab) = self.tab() {
            if let Some(t) = tab_input(cx, event, tab) {
                return t;
            }
        }
        if self.loading {
            return if hit(cx, event) == Some(1) {
                Transition::Back
            } else {
                Transition::None
            };
        }
        let h = hit(cx, event);
        match h {
            Some(60_000) => return Transition::Root(Box::new(library::LibraryPage::discover())),
            Some(60_001) => return Transition::None,
            Some(60_002) => return Transition::Push(Box::new(ranks::RankCategories)),
            Some(1) => {
                if self.book_catalog && matches!(self.mode, Mode::WenkuBook(_)) {
                    self.book_catalog = false;
                    self.page = 0;
                    cx.request_redraw(RefreshHint::Ui);
                } else {
                    return Transition::Back;
                }
            }
            Some(19) => {
                if let Some(Item {
                    action: Action::Info(v),
                    ..
                }) = self.items.first()
                {
                    if let Some(a) = v["authors"]
                        .as_array()
                        .and_then(|a| a.first())
                        .and_then(Value::as_str)
                    {
                        return Transition::Push(Box::new(Self::wenku_search(format!("\"{a}\""))));
                    }
                }
            }
            Some(40) => {
                self.book_catalog = true;
                self.page = 0;
                cx.request_redraw(RefreshHint::Ui);
            }
            Some(41) => {
                if let Some(Item {
                    action: Action::Info(v),
                    ..
                }) = self.items.first()
                {
                    return Transition::Push(Box::new(text::TextPage::new(
                        &title(v),
                        &string(v, "introduction"),
                    )));
                }
            }
            Some(id) if (80..83).contains(&id) => {
                let field = (id - 80) as u8;
                let (name, options, selected) = self.rank_choices(field);
                self.choice = Some((
                    10 + field,
                    choice::Choice::new(name, options, Some(selected)),
                ));
                cx.request_redraw(RefreshHint::Ui);
            }
            Some(2) if self.page > 0 => {
                self.page -= 1;
                if matches!(self.mode, Mode::Rank(..)) {
                    cx.request_redraw(RefreshHint::Ui)
                } else {
                    self.load(cx);
                }
            }
            Some(2)
                if matches!(&self.mode,Mode::Rank(p,_) if p=="syosetu") && self.rank_remote > 0 =>
            {
                self.rank_remote -= 1;
                self.page = 0;
                self.load(cx);
            }
            Some(3) if self.page + 1 < self.pages => {
                self.page += 1;
                if matches!(self.mode, Mode::Rank(..)) {
                    cx.request_redraw(RefreshHint::Ui)
                } else {
                    self.load(cx);
                }
            }
            Some(3)
                if matches!(&self.mode,Mode::Rank(p,_) if p=="syosetu")
                    && self.rank_remote + 1 < self.rank_source_pages =>
            {
                self.rank_remote += 1;
                self.page = 0;
                self.load(cx);
            }
            Some(10) => {
                if matches!(
                    self.mode,
                    Mode::WenkuBook(_) | Mode::Comments(..) | Mode::Comment(..)
                ) && !cx.app.repo.api.signed_in()
                {
                    return Transition::Push(Box::new(login::LoginPage::new()));
                }
                match self.mode.clone() {
                    Mode::Wenku(q, _) => {
                        self.searching = true;
                        self.editor(&q, 0);
                    }
                    Mode::WenkuBook(id) => {
                        return Transition::Push(Box::new(Self::new(Mode::Folders(
                            true,
                            Some(NovelKey {
                                provider_id: "wenku".into(),
                                novel_id: id,
                            }),
                        ))))
                    }
                    Mode::Folders(w, _) => {
                        self.choice = Some((
                            2,
                            choice::Choice::new(
                                "收藏类型",
                                ["Web小说", "文库小说"],
                                Some(w as usize),
                            ),
                        ));
                    }
                    Mode::History => {
                        let api = cx.app.repo.api.clone();
                        self.loading = true;
                        cx.spawn(move || {
                            Mutation(api.get::<bool>("user/read-history/paused").and_then(|p| {
                                api.request(
                                    if p { "DELETE" } else { "PUT" },
                                    "user/read-history/paused",
                                    b"",
                                    "text/plain",
                                )
                                .map(|_| ())
                            }))
                        });
                    }
                    Mode::Rank(p, r) => {
                        self.mode = Mode::Rank(
                            if p == "syosetu" {
                                "kakuyomu"
                            } else {
                                "syosetu"
                            }
                            .into(),
                            r,
                        );
                        self.page = 0;
                        self.load(cx);
                    }
                    Mode::Comments(..) | Mode::Comment(..) => self.editor("", 0),
                    Mode::Favorites(w, _) => {
                        return Transition::Push(Box::new(Self::new(Mode::Folders(w, None))))
                    }
                }
            }
            Some(11) => match self.mode.clone() {
                Mode::Wenku(_, l) => {
                    self.choice = Some((
                        0,
                        choice::Choice::new("选择分级", WENKU_LEVELS, Some(l as usize)),
                    ));
                    cx.request_redraw(RefreshHint::Ui);
                }
                Mode::WenkuBook(id) => {
                    return Transition::Push(Box::new(Self::new(Mode::Comments(
                        format!("wenku-{id}"),
                        None,
                    ))))
                }
                Mode::Folders(..) => self.editor("", 0),
                Mode::History => {
                    self.notice = "再次点击清除历史以确认删除云端及本地历史".into();
                    self.edit_kind = if self.edit_kind == 9 {
                        self.mutate(cx, "DELETE", "user/read-history".into(), Value::Null);
                        0
                    } else {
                        9
                    };
                }
                Mode::Rank(..) => {
                    let (name, options, selected) = self.rank_choices(1);
                    self.choice = Some((11, choice::Choice::new(name, options, Some(selected))));
                }
                Mode::Comments(s, _) => {
                    self.mode = Mode::Comments(s, None);
                    self.page = 0;
                    self.load(cx);
                }
                Mode::Comment(_, c) => {
                    if string(&c, "authorUsername") == cx.app.repo.api.username() {
                        self.editor(&string(&c, "content"), 3);
                    } else {
                        self.notice = "只能编辑自己的评论".into();
                    }
                }
                _ => return Transition::Back,
            },
            Some(12) => {
                if let Mode::Comment(_, c) = self.mode.clone() {
                    if string(&c, "authorUsername") != cx.app.repo.api.username() {
                        self.notice = "只能删除自己的评论".into();
                    } else if self.edit_kind == 8 {
                        self.mutate(
                            cx,
                            "DELETE",
                            format!("forum:external/comment/novel/{}", c["id"]),
                            Value::Null,
                        );
                    } else {
                        self.edit_kind = 8;
                        self.notice = "再次点击删除以确认".into();
                    }
                } else {
                    self.load(cx);
                }
            }
            Some(id) if id >= 200 => {
                let offset = if matches!(
                    self.mode,
                    Mode::WenkuBook(_) | Mode::Folders(..) | Mode::Rank(..)
                ) {
                    self.page * self.rows
                        + usize::from(self.book_catalog && matches!(self.mode, Mode::WenkuBook(_)))
                } else {
                    0
                };
                if let Some(item) = self.items.get(offset + (id - 200) as usize).cloned() {
                    match item.action {
                        Action::Web(key) => {
                            return Transition::Push(Box::new(book::BookPage::new(key)))
                        }
                        Action::Wenku(id) => {
                            return Transition::Push(Box::new(Self::new(Mode::WenkuBook(id))))
                        }
                        Action::Folder(id) => {
                            if let Mode::Folders(wenku, Some(key)) = self.mode.clone() {
                                let path = if wenku {
                                    format!(
                                        "user/favored-wenku/{}/{}",
                                        encode(&id),
                                        encode(&key.novel_id)
                                    )
                                } else {
                                    format!(
                                        "user/favored-web/{}/{}/{}",
                                        encode(&id),
                                        encode(&key.provider_id),
                                        encode(&key.novel_id)
                                    )
                                };
                                self.mutate(cx, "PUT", path, Value::Null);
                            } else {
                                let wenku = matches!(self.mode, Mode::Folders(true, _));
                                return Transition::Push(Box::new(Self::new(Mode::Favorites(
                                    wenku, id,
                                ))));
                            }
                        }
                        Action::Volume(n, v, human) => {
                            self.loading = true;
                            self.notice = "正在下载 EPUB 并解析目录…".into();
                            let repo = cx.app.repo.clone();
                            let config = cx.app.config.clone();
                            cx.spawn(move || {
                                EpubResult(open_volume(&repo, &config, &n, &v, human))
                            });
                        }
                        Action::Comment(c) => {
                            let s = match &self.mode {
                                Mode::Comments(s, _) => s.clone(),
                                _ => String::new(),
                            };
                            return Transition::Push(Box::new(comments::Thread::new(s, c)));
                        }
                        Action::Replies(root) => {
                            if let Mode::Comment(s, _) = &self.mode {
                                return Transition::Push(Box::new(Self::new(Mode::Comments(
                                    s.clone(),
                                    Some(root),
                                ))));
                            }
                        }
                        Action::None | Action::Info(_) => {
                            return Transition::Push(Box::new(super::text::TextPage::new(
                                &item.title,
                                &item.subtitle,
                            )))
                        }
                    }
                }
            }
            _ => {}
        }
        // 长按收藏夹进入重命名/删除确认页；长按收藏项目移出所在收藏夹。
        if let InputEvent::Gesture(g) = event {
            if g.kind == GestureKind::Long {
                if let Some(id) = cx
                    .hits
                    .at(Point {
                        x: g.end.0,
                        y: g.end.1,
                    })
                    .filter(|i| i.0 >= 200 && i.0 < 1000)
                {
                    let offset = if matches!(self.mode, Mode::Folders(..)) {
                        self.page * self.rows
                    } else {
                        0
                    };
                    if let Some(item) = self.items.get(offset + (id.0 - 200) as usize) {
                        match (&self.mode, &item.action) {
                            (Mode::Folders(w, _), Action::Folder(id)) => {
                                return Transition::Push(Box::new(
                                    super::manage::ManagePage::folder(
                                        *w,
                                        id.clone(),
                                        item.title.clone(),
                                    ),
                                ))
                            }
                            (Mode::Favorites(w, f), a) => {
                                let path = match a {
                                    Action::Web(k) => format!(
                                        "user/favored-web/{}/{}/{}",
                                        encode(f),
                                        encode(&k.provider_id),
                                        encode(&k.novel_id)
                                    ),
                                    Action::Wenku(id) => {
                                        format!("user/favored-wenku/{}/{}", encode(f), encode(id))
                                    }
                                    _ => String::new(),
                                };
                                if !path.is_empty() {
                                    return Transition::Push(Box::new(
                                        super::manage::ManagePage::remove(path),
                                    ));
                                }
                                let _ = w;
                            }
                            (Mode::History, Action::Web(k)) => {
                                return Transition::Push(Box::new(
                                    super::manage::ManagePage::remove(format!(
                                        "user/read-history/{}/{}",
                                        encode(&k.provider_id),
                                        encode(&k.novel_id)
                                    )),
                                ))
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
        cx.request_redraw(RefreshHint::Ui);
        Transition::None
    }
    fn on_message(&mut self, cx: &mut Cx<Kindlia>, msg: Box<dyn Any + Send>) {
        let msg = match msg.downcast::<CachedResult>() {
            Ok(cached) => {
                self.on_message(cx, Box::new(cached.0));
                return;
            }
            Err(msg) => msg,
        };
        let msg = match msg.downcast::<ResultMessage>() {
            Ok(r) => {
                if r.0 != self.generation {
                    return;
                }
                self.loading = false;
                match r.1 {
                    Ok((items, pages, notice, source_pages)) => {
                        self.rank_source_pages = source_pages;
                        self.items = items;
                        self.pages = pages;
                        self.page = self.page.min(pages - 1);
                        self.notice = notice;
                    }
                    Err(e) => self.notice = e,
                }
                cx.request_redraw(RefreshHint::Ui);
                return;
            }
            Err(m) => m,
        };
        let msg = match msg.downcast::<Mutation>() {
            Ok(r) => {
                self.loading = false;
                match r.0 {
                    Ok(()) => {
                        self.notice = "操作已完成".into();
                        if matches!(self.mode, Mode::Comment(..)) {
                            self.pending = Some(Transition::Back);
                        } else {
                            self.load_with_cache(cx, false);
                        }
                    }
                    Err(e) => self.notice = e,
                }
                cx.request_redraw(RefreshHint::Ui);
                return;
            }
            Err(m) => m,
        };
        let msg = match msg.downcast::<CoverResult>() {
            Ok(r) => {
                if self.covers.contains_key(&r.0) {
                    self.covers.insert(r.0, r.1.ok());
                }
                cx.request_redraw(RefreshHint::Ui);
                return;
            }
            Err(m) => m,
        };
        if let Ok(r) = msg.downcast::<EpubResult>() {
            self.loading = false;
            match r.0 {
                Ok(key) => {
                    let id = cx
                        .app
                        .repo
                        .store
                        .progress(&key)
                        .map(|p| p.chapter_id)
                        .unwrap_or("0".into());
                    self.pending =
                        Some(Transition::Push(Box::new(reader::ReaderPage::new(key, id))));
                }
                Err(e) => self.notice = e,
            }
            cx.request_redraw(RefreshHint::Ui);
        }
    }
    fn take_transition(&mut self) -> Transition<Kindlia> {
        self.pending.take().unwrap_or(Transition::None)
    }
}

fn rank_options(
    provider: &str,
    kind: usize,
) -> (
    &'static [&'static str],
    &'static [&'static str],
    &'static [&'static str],
) {
    if provider == "syosetu" {
        let genres: &[&str] = match kind % 3 {
            0 => &[],
            1 => &[
                "恋爱：异世界",
                "恋爱：现实世界",
                "幻想：高幻想",
                "幻想：低幻想",
                "文学：纯文学",
                "文学：人性剧",
                "文学：历史",
                "文学：推理",
                "文学：恐怖",
                "文学：动作",
                "文学：喜剧",
                "科幻：VR游戏",
                "科幻：宇宙",
                "科幻：空想科学",
                "科幻：惊悚",
                "其他：童话",
                "其他：诗",
                "其他：散文",
                "其他：其他",
            ],
            _ => &["恋爱", "幻想", "文学/科幻/其他"],
        };
        (
            &["综合", "流派", "异世界转生/转移"],
            genres,
            &["全部", "短篇", "连载", "完结"],
        )
    } else {
        (
            &["流派"],
            &[
                "综合",
                "异世界幻想",
                "现代幻想",
                "科幻",
                "恋爱",
                "浪漫喜剧",
                "现代戏剧",
                "恐怖",
                "推理",
                "散文·纪实",
                "历史·时代·传奇",
                "创作论·评论",
                "诗·童话·其他",
            ],
            &["全部", "长篇", "短篇"],
        )
    }
}
