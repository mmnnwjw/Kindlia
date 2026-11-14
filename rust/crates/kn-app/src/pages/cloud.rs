mod data;
pub use data::open_volume;
use data::{fetch, rank_options};
// AutoNovel 读者功能：复用 KinNovel 列表/分页/键盘，页面只在工作池中请求网络。
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
}
#[derive(Clone)]
enum Action {
    Web(NovelKey),
    Wenku(String),
    Folder(String),
    Volume(String, String, bool),
    Comment(Value),
    Info(Value),
}
mod input;
mod view;

#[derive(Clone)]
struct Item {
    title: String,
    subtitle: String,
    badge: String,
    action: Action,
    cover: String,
}
struct ResultMessage(u64, Result<(Vec<Item>, usize, String, usize), String>);
/// 缓存可立即操作，在线更新仍用同一个 generation 防止覆盖新筛选/翻页/修改。
struct CachedResult(ResultMessage);
struct Mutation(Result<(), String>);
struct EpubResult(Result<NovelKey, String>);
struct CoverResult(String, Result<Bitmap, String>, bool);

pub struct CloudPage {
    mode: Mode,
    root: bool,
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
    covers: crate::cover_cache::Cache<Option<Bitmap>>,
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
        if !self.root {
            return None;
        }
        match self.mode {
            Mode::Wenku(..) | Mode::Rank(..) => Some(2),
            Mode::Folders(..) | Mode::Favorites(..) => Some(0),
            Mode::History => Some(1),
            _ => None,
        }
    }

    pub fn root(mode: Mode) -> Self {
        Self {
            root: true,
            ..Self::new(mode)
        }
    }
    pub fn new(mode: Mode) -> Self {
        Self {
            mode,
            root: false,
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
            covers: Default::default(),
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
        self.editor
            .set_max_chars(if matches!(self.mode, Mode::Comments(..)) {
                3000
            } else {
                256
            });
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
            _ => {}
        }
    }
}
impl Page<Kindlia> for CloudPage {
    fn id(&self) -> PageId {
        PageId(if self.root && !self.searching {
            if matches!(self.mode, Mode::History) {
                14
            } else {
                12
            }
        } else {
            50
        })
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
        self.render_view(cx, frame)
    }

    fn on_input(&mut self, cx: &mut Cx<Kindlia>, event: &InputEvent) -> Transition<Kindlia> {
        self.handle_input(cx, event)
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
                        cx.toast("操作已完成");
                        self.load_with_cache(cx, false);
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
                    let failed = r.2;
                    self.covers.finish(r.0, r.1.ok(), failed);
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
