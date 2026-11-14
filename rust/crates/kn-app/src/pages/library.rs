//! KinNovel 的书架/发现页面结构与组件，数据源替换为本地书架及 AutoNovel。
use super::{book::BookPage, settings::SettingsPage, *};
use crate::{
    api::ListQuery,
    model::{NovelOutline, Page as ApiPage},
    repository::Loaded,
    store::ShelfEntry,
    Kindlia,
};
use kn_platform::{GestureKind, InputEvent};
use kn_render::{Bitmap, Rect};
use kn_ui::{
    keyboard::{EditorEvent, TextEditor},
    widgets::{self, Ink},
    Cx, Page, PageId, RefreshHint, Transition,
};
use std::any::Any;

mod input;
mod view;

struct ListResult(u64, Result<Loaded<ApiPage<NovelOutline>>, String>);
struct FoldersResult(u64, Result<Vec<serde_json::Value>, String>);
struct FolderMutation(Result<(), String>);
struct ShelfResult(Result<Vec<ShelfEntry>, String>);
struct ClearResult(Result<(), String>);
struct ArtResult(
    String,
    crate::covers::Metadata,
    Result<Bitmap, String>,
    bool,
);
fn art(
    art: &mut crate::cover_cache::Cache<Option<(crate::covers::Metadata, Bitmap)>>,
    cx: &mut Cx<Kindlia>,
    key: &crate::model::NovelKey,
    size: (u32, u32),
) -> String {
    let tag = format!("{}:{}x{}", key.cache_id(), size.0, size.1);
    if !art.contains_key(&tag) {
        art.insert(tag.clone(), None);
        let key = key.clone();
        let id = tag.clone();
        crate::covers::spawn(cx, key, size, move |meta, image| {
            let failed = image.is_err();
            ArtResult(
                id,
                meta,
                image.or_else(|_| crate::covers::fallback(size)),
                failed,
            )
        });
    }
    tag
}
pub struct LibraryPage {
    discover: bool,
    local: bool,
    history: bool,
    shelf_source: u8,
    folders: Vec<serde_json::Value>,
    query: ListQuery,
    books: Vec<NovelOutline>,
    shelf: Vec<ShelfEntry>,
    pages: usize,
    page: usize,
    rows: usize,
    loading: bool,
    notice: String,
    generation: u64,
    editor: TextEditor,
    editing: bool,
    searching: bool,
    return_on_close: bool,
    picker: Option<(usize, choice::Choice)>,
    managing_folder: Option<(String, String)>,
    renaming_folder: bool,
    art: crate::cover_cache::Cache<Option<(crate::covers::Metadata, Bitmap)>>,
}
impl LibraryPage {
    pub fn local_books() -> Self {
        Self {
            local: true,
            ..Self::new(false)
        }
    }
    pub fn shelf() -> Self {
        Self::new(false)
    }
    pub fn history() -> Self {
        let mut p = Self::new(false);
        p.history = true;
        p
    }
    pub fn search(query: String) -> Self {
        let mut page = Self::new(true);
        page.query.query = query;
        page.searching = true;
        page.return_on_close = true;
        page
    }
    pub fn discover() -> Self {
        Self::new(true)
    }
    fn new(discover: bool) -> Self {
        Self {
            discover,
            local: false,
            history: false,
            shelf_source: 1,
            folders: vec![],
            query: ListQuery::default(),
            books: vec![],
            shelf: vec![],
            pages: 1,
            page: 0,
            rows: 6,
            loading: false,
            notice: String::new(),
            generation: 0,
            editor: TextEditor::new(1000, ""),
            editing: false,
            searching: false,
            return_on_close: false,
            picker: None,
            managing_folder: None,
            renaming_folder: false,
            art: Default::default(),
        }
    }
    pub(super) fn active_tip(&self) -> Option<(&'static tips::Tip, bool)> {
        if self.editing {
            Some((&tips::KEYBOARD, true))
        } else if self.discover {
            Some((&tips::SEARCH, self.picker.is_none() && !self.loading))
        } else if !self.history && !self.local {
            Some((&tips::SHELF, self.picker.is_none() && !self.loading))
        } else {
            None
        }
    }
    fn load(&mut self, cx: &mut Cx<Kindlia>, refresh: bool) {
        if self.discover {
            let m = cx.app.metrics;
            let controls = if self.searching {
                m.option_h()
            } else {
                m.touch
            };
            self.rows = (cx.height.saturating_sub(
                m.header_h()
                    + controls
                    + if self.return_on_close {
                        0
                    } else {
                        m.tab_bar_h()
                    }
                    + m.pager_h(),
            ) / m.row_h_cover())
            .max(1) as usize;
        }
        self.loading = true;

        self.notice = "加载中…".into();
        self.generation += 1;
        let repo = cx.app.repo.clone();
        if self.discover {
            self.query.page = self.page;
            self.query.page_size = self.rows;
            let query = self.query.clone();
            let generation = self.generation;
            crate::repository::read_on_workers(
                cx,
                repo,
                refresh,
                move |r| r.list(&query, refresh),
                move |r| ListResult(generation, r),
            );
        } else {
            let store = repo.store.clone();
            cx.spawn_local(move || ShelfResult(store.shelf()));
            if !self.history && !self.local {
                if !repo.api.signed_in() {
                    self.folders.clear();
                    self.loading = false;
                    self.pages = 1;
                    self.page = 0;
                    self.notice = "登录后查看云端收藏夹".into();
                    cx.request_redraw(RefreshHint::Ui);
                    return;
                }
                let source = self.shelf_source;
                let generation = self.generation;
                let poster = cx.page_poster();
                let spawner = cx.spawner();
                cx.spawn_local_droppable(move || {
                    let cache = ("folders", repo.api.username());
                    let value: Option<serde_json::Value> = repo.store.read_cache("cloud", &cache);
                    if let Some(folders) = value.as_ref().and_then(|v| {
                        v[if source == 2 {
                            "favoredWenku"
                        } else {
                            "favoredWeb"
                        }]
                        .as_array()
                    }) {
                        poster.post(FoldersResult(generation, Ok(folders.clone())));
                        if repo.offline {
                            return;
                        }
                    } else if repo.offline {
                        poster.post(FoldersResult(generation, Err("没有收藏夹缓存".into())));
                        return;
                    }
                    let work = move || {
                        FoldersResult(
                            generation,
                            (|| {
                                let cache = ("folders", repo.api.username());
                                let value: serde_json::Value = if repo.offline {
                                    repo.store
                                        .read_cache("cloud", &cache)
                                        .ok_or("没有收藏夹缓存")?
                                } else {
                                    let v = repo.api.get("user/favored")?;
                                    repo.store.write_cache("cloud", &cache, &v)?;
                                    v
                                };
                                value[if source == 2 {
                                    "favoredWenku"
                                } else {
                                    "favoredWeb"
                                }]
                                .as_array()
                                .cloned()
                                .ok_or_else(|| "收藏夹格式无效".into())
                            })(),
                        )
                    };
                    if let Some(spawner) = spawner {
                        spawner.spawn_on(kn_ui::Lane::Net, true, work);
                    }
                });
            }
        }
        cx.request_redraw(RefreshHint::Ui);
    }
}
impl Page<Kindlia> for LibraryPage {
    fn ready(&self) -> bool {
        !self.loading || !self.books.is_empty() || !self.folders.is_empty() || !self.shelf.is_empty()
    }
    fn id(&self) -> PageId {
        PageId(if self.discover {
            if self.searching || self.return_on_close {
                111
            } else {
                11
            }
        } else if self.history {
            15
        } else if self.local {
            16
        } else {
            10
        })
    }
    fn enter(&mut self, cx: &mut Cx<Kindlia>, _returning: bool) {
        if !_returning && !self.local && !self.discover && !self.history {
            self.shelf_source = 1;
        }
        if let Some(q) = cx.app.discover_query.take() {
            self.query = q;
            self.page = 0;
        }
        let m = cx.app.metrics;
        self.rows = ((cx.height.saturating_sub(
            m.header_h()
                + m.touch
                + m.tab_bar_h()
                + m.pager_h()
                + if !self.discover && !self.history {
                    (m.touch as f32 * 2.45) as u32 + m.margin
                } else {
                    0
                },
        )) / m.row_h_cover())
        .max(1) as usize;
        self.load(cx, false);
    }
    fn wants_backdrop(&self) -> kn_ui::Backdrop {
        self.picker
            .as_ref()
            .map(|(_, p)| p)
            .map(|p| p.wants_backdrop())
            .unwrap_or(kn_ui::Backdrop::None)
    }
    fn set_backdrop(&mut self, bg: Option<Bitmap>) {
        if let Some(p) = self.picker.as_mut().map(|(_, p)| p) {
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
        let msg = match msg.downcast::<ArtResult>() {
            Ok(result) => {
                let ArtResult(key, meta, result, failed) = *result;
                if self.art.contains_key(&key) {
                    let value = result.ok().map(|image| (meta, image));
                    self.art.finish(key, value, failed);
                }
                cx.request_redraw(RefreshHint::Ui);
                return;
            }
            Err(msg) => msg,
        };
        let msg = match msg.downcast::<ClearResult>() {
            Ok(r) => {
                self.loading = false;
                match r.0 {
                    Ok(()) => self.load(cx, false),
                    Err(e) => self.notice = e,
                };
                cx.request_redraw(RefreshHint::Ui);
                return;
            }
            Err(m) => m,
        };
        let msg = match msg.downcast::<FolderMutation>() {
            Ok(r) => {
                self.loading = false;
                match r.0 {
                    Ok(()) => {
                        cx.toast("操作已完成");
                        self.load(cx, true);
                    }
                    Err(e) => {
                        self.notice = e;
                        cx.request_redraw(RefreshHint::Ui);
                    }
                }
                return;
            }
            Err(m) => m,
        };
        let msg = match msg.downcast::<ListResult>() {
            Ok(result) => {
                if result.0 != self.generation {
                    return;
                }
                self.loading = false;
                match result.1 {
                    Ok(loaded) => {
                        self.books = loaded
                            .value
                            .items
                            .into_iter()
                            .filter(|b| {
                                (!cx.app.config.ignore_japanese
                                    || b.youdao + b.baidu + b.gpt + b.sakura > 0)
                                    && (!cx.app.config.ignore_ai
                                        || b.youdao + b.baidu > 0
                                        || b.gpt + b.sakura == 0)
                            })
                            .collect();
                        self.pages = loaded.value.page_number.max(1);
                        self.notice = loaded.notice.unwrap_or_else(|| {
                            if self.books.is_empty() {
                                "没有找到小说".into()
                            } else {
                                self.query.query.clone()
                            }
                        });
                    }
                    Err(e) => {
                        self.books.clear();
                        self.notice = e;
                    }
                }
                cx.request_redraw(RefreshHint::Ui);
                return;
            }
            Err(msg) => msg,
        };
        let msg = match msg.downcast::<FoldersResult>() {
            Ok(r) => {
                if r.0 != self.generation {
                    return;
                }
                self.loading = false;
                match r.1 {
                    Ok(folders) => {
                        self.pages = folders.len().div_ceil(self.rows).max(1);
                        self.page = self.page.min(self.pages - 1);
                        self.folders = folders;
                        self.notice = if self.folders.is_empty() {
                            "暂无收藏夹".into()
                        } else {
                            String::new()
                        };
                    }
                    Err(e) => self.notice = e,
                }
                cx.request_redraw(RefreshHint::Ui);
                return;
            }
            Err(m) => m,
        };
        if let Ok(result) = msg.downcast::<ShelfResult>() {
            if self.local || self.history {
                self.loading = false;
            }
            match result.0 {
                Ok(entries) => {
                    self.shelf = if self.history {
                        entries
                            .into_iter()
                            .filter(|b| b.progress.is_some())
                            .collect()
                    } else if self.local {
                        entries
                            .into_iter()
                            .filter(|b| matches!(b.key.provider_id.as_str(), "epub" | "local"))
                            .collect()
                    } else {
                        entries
                    };
                    if self.local || self.history {
                        self.pages = self.shelf.len().div_ceil(self.rows).max(1);
                        self.page = self.page.min(self.pages - 1);
                        self.notice = if self.shelf.is_empty() {
                            "书架为空，请到发现页选书".into()
                        } else {
                            String::new()
                        };
                    }
                }
                Err(e) => self.notice = e,
            }
            cx.request_redraw(RefreshHint::Ui);
        }
    }
}
