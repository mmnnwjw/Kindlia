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

struct ListResult(u64, Result<Loaded<ApiPage<NovelOutline>>, String>);
struct FoldersResult(u64, Result<Vec<serde_json::Value>, String>);
struct ShelfResult(Result<Vec<ShelfEntry>, String>);
struct ClearResult(Result<(), String>);
struct ArtResult(String, crate::covers::Metadata, Result<Bitmap, String>);
fn art(
    art: &mut std::collections::HashMap<String, Option<(crate::covers::Metadata, Bitmap)>>,
    cx: &mut Cx<Kindlia>,
    key: &crate::model::NovelKey,
    size: (u32, u32),
) -> String {
    let tag = format!("{}:{}x{}", key.cache_id(), size.0, size.1);
    if !art.contains_key(&tag) {
        art.insert(tag.clone(), None);
        let repo = cx.app.repo.clone();
        let key = key.clone();
        let id = tag.clone();
        cx.spawn_droppable(move || {
            let meta = crate::covers::metadata(&repo, &key, true);
            let image = crate::covers::image(&repo, &meta.cover, size);
            ArtResult(id, meta, image)
        });
    }
    tag
}
pub struct LibraryPage {
    discover: bool,
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
    art: std::collections::HashMap<String, Option<(crate::covers::Metadata, Bitmap)>>,
}
impl LibraryPage {
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
            history: false,
            shelf_source: 0,
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
            art: Default::default(),
        }
    }
    fn load(&mut self, cx: &mut Cx<Kindlia>, refresh: bool) {
        if self.discover {
            let m=cx.app.metrics;
            let controls=if self.searching {m.touch+m.touch/4}else{m.touch};
            self.rows=(cx.height.saturating_sub(m.header_h()+controls+m.tab_bar_h()+m.pager_h())/m.row_h_cover()).max(1)as usize;
        }
        self.loading = true;
        if self.art.len() > 32 {
            self.art.clear();
        }
        self.notice = "加载中…".into();
        self.generation += 1;
        let repo = cx.app.repo.clone();
        if self.discover {
            self.query.page = self.page;
            self.query.page_size = self.rows;
            let query = self.query.clone();
            let generation = self.generation;
            crate::repository::read_on_workers(cx, repo, refresh, move |r| r.list(&query, refresh), move |r| ListResult(generation, r));
        } else {
            let store = repo.store.clone();
            cx.spawn_local(move || ShelfResult(store.shelf()));
            if self.shelf_source > 0 && !self.history {
                let source = self.shelf_source;
                let generation = self.generation;
                let poster = cx.page_poster();
                let spawner = cx.spawner();
                cx.spawn_local_droppable(move || {
                    let cache = ("folders", repo.api.username());
                    let value: Option<serde_json::Value> = repo.store.read_cache("cloud", &cache);
                    if let Some(folders) = value.as_ref().and_then(|v| v[if source == 2 { "favoredWenku" } else { "favoredWeb" }].as_array()) {
                        poster.post(FoldersResult(generation, Ok(folders.clone())));
                        if repo.offline { return; }
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
    fn id(&self) -> PageId {
        PageId(if self.discover { 11 } else { 10 })
    }
    fn enter(&mut self, cx: &mut Cx<Kindlia>, _returning: bool) {
        if !_returning && !self.discover && !self.history && cx.app.repo.api.signed_in() {
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
        self.picker.as_ref().map(|(_,p)| p).map(|p| p.wants_backdrop()).unwrap_or(kn_ui::Backdrop::None)
    }
    fn set_backdrop(&mut self, bg: Option<Bitmap>) {
        if let Some(p) = self.picker.as_mut().map(|(_,p)| p) { p.set_backdrop(bg); }
    }
    fn render(&mut self, cx: &mut Cx<Kindlia>, frame: &mut Bitmap) {
        if let Some((_, picker)) = &mut self.picker {
            picker.render(cx, frame);
            return;
        }
        if self.discover {
            search_header(
                cx,
                frame,
                if self.searching {
                    "搜索网络小说"
                } else {
                    "发现"
                },
                self.searching,
                20,
            );
        } else {
            header(
                cx,
                frame,
                if self.history {
                    "历史"
                } else {
                    "我的收藏"
                },
                false,
            );
        }
        let m = cx.app.metrics;
        let width = cx.width;
        let mut y = m.header_h();
        if !self.discover && !self.history {
            let rect = Rect::new(
                m.margin as i32,
                (y + m.margin / 2) as i32,
                width - 2 * m.margin,
                (m.touch as f32 * 2.45) as u32,
            );
            widgets::card(frame, &cx.theme, &m, rect);
            let pad = m.margin as i32 / 2 + 8;
            let h = (rect.h as i32 - 2 * pad).max(1);
            let cover = Rect::new(
                rect.x + pad,
                rect.y + pad,
                (h as f32 * 0.72) as u32,
                h as u32,
            );
            let current_key = self
                .shelf
                .iter()
                .find(|b| b.progress.is_some())
                .map(|b| b.key.clone());
            if let Some(key) = current_key {
                let id = art(&mut self.art, cx, &key, (cover.w, cover.h));
                if let Some(Some((_, image))) = self.art.get(&id) {
                    frame.blit(
                        image,
                        image.bounds(),
                        cover.x + (cover.w - image.width()) as i32 / 2,
                        cover.y + (cover.h - image.height()) as i32 / 2,
                    );
                }
            }
            let x = cover.right() + m.margin as i32 / 2;
            let mut ink = Ink {
                fonts: cx.fonts,
                glyphs: cx.glyphs,
                chain: &cx.app.ui_fonts,
            };
            ink.text(frame, x, cover.y, "继续阅读", m.small, cx.theme.muted);
            let title = self
                .shelf
                .iter()
                .find(|b| b.progress.is_some())
                .map(|b| b.title.as_str())
                .unwrap_or("还没有阅读记录");
            let title = ink.fit(title, m.body, (rect.right() - pad - x).max(1) as f32);
            ink.text(
                frame,
                x,
                cover.y + (m.small * 1.8) as i32,
                &title,
                m.body,
                cx.theme.foreground,
            );
            ink.text(
                frame,
                x,
                cover.y + (m.small * 3.5) as i32,
                "点按回到上次位置",
                m.small,
                cx.theme.muted,
            );
            cx.hits.add(kn_ui::HitId(40), rect).rounded(m.radius);
            y = rect.bottom() as u32 + m.margin / 2;
        }
        if self.discover && !self.searching {
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
                Rect::new(m.margin as i32, y as i32, width - 2 * m.margin, m.touch),
                &["网络小说", "文库小说", "排行"],
                0,
                50,
            );
            y += m.touch;
        }
        if self.searching {
            for field in 0..5 {
                let (name, options, selected) = filters::options(field, &self.query);
                option_button(
                    cx,
                    frame,
                    Rect::new(
                        (field as u32 * width / 5) as i32,
                        y as i32,
                        width / 5,
                        m.touch + m.touch/4,
                    ),
                    &name,
                    &options[selected],
                    71 + field as u32,
                );
            }
            y += m.touch + m.touch/4;
        }
        let cell = width / 3;
        if !self.discover && !self.history {
            selected_button(
                cx,
                frame,
                Rect::new(0, y as i32, cell, m.touch),
                "网络收藏",
                60,
                self.shelf_source == 1,
            );
            selected_button(
                cx,
                frame,
                Rect::new(cell as i32, y as i32, cell, m.touch),
                "文库收藏",
                61,
                self.shelf_source == 2,
            );
            button(
                cx,
                frame,
                Rect::new((cell * 2) as i32, y as i32, cell, m.touch),
                "管理收藏",
                62,
                !self.loading,
            );
            y += m.touch;
        } else if self.history {
            button(
                cx,
                frame,
                Rect::new(m.margin as i32, y as i32, width - 2 * m.margin, m.touch),
                "清除历史",
                63,
                !self.loading,
            );
            y += m.touch;
        }
        let top = y;
        // 常态不保留空白提示行；错误/查询信息在列表底部显示。
        if !self.notice.is_empty()
            && self.notice != "我的收藏"
            && !self.loading
            && (self.folders.is_empty() || self.discover || self.history)
        {
            label(
                cx,
                frame,
                Rect::new(
                    0,
                    cx.height
                        .saturating_sub(m.tab_bar_h() + m.pager_h() + m.touch / 2)
                        as i32,
                    width,
                    m.touch / 2,
                ),
                &self.notice,
                true,
            );
        }
        if !self.editing {
            if self.discover {
                for (i, book) in self.books.iter().take(self.rows).enumerate() {
                    let rect = Rect::new(
                        0,
                        (top + m.row_h_cover() * i as u32) as i32,
                        width,
                        m.row_h_cover(),
                    );
                    let size = widgets::cover_slot(&m, rect);
                    let id = art(&mut self.art, cx, &book.key(), (size.w, size.h));
                    let info = self.art.get(&id).and_then(|a| a.as_ref());
                    let subtitle = info
                        .map(|(m, _)| m.author.clone())
                        .filter(|s| !s.is_empty())
                        .unwrap_or_else(|| format!("{} · {} 章", book.provider_id, book.total));
                    let mut ink = Ink {
                        fonts: cx.fonts,
                        glyphs: cx.glyphs,
                        chain: &cx.app.ui_fonts,
                    };
                    widgets::list_row_cover(
                        &mut ink,
                        frame,
                        &cx.theme,
                        &m,
                        rect,
                        info.map(|(_, i)| i),
                        book.title(),
                        &subtitle,
                        &format!(
                            "有道 {} · GPT {} · Sakura {}",
                            book.youdao, book.gpt, book.sakura
                        ),
                    );
                    cx.hits.add(HitId(200 + i as u32), rect).long_press();
                }
            } else if self.shelf_source > 0 && !self.history {
                for (i, folder) in self
                    .folders
                    .iter()
                    .skip(self.page * self.rows)
                    .take(self.rows)
                    .enumerate()
                {
                    row(
                        cx,
                        frame,
                        Rect::new(
                            0,
                            (top + m.row_h_cover() * i as u32) as i32,
                            width,
                            m.row_h_cover(),
                        ),
                        folder["title"].as_str().unwrap_or("收藏夹"),
                        "收藏夹 · 长按管理",
                        "",
                        Some(200 + i as u32),
                    );
                }
            } else {
                for (i, book) in self
                    .shelf
                    .iter()
                    .skip(self.page * self.rows)
                    .take(self.rows)
                    .enumerate()
                {
                    let rect = Rect::new(
                        0,
                        (top + m.row_h_cover() * i as u32) as i32,
                        width,
                        m.row_h_cover(),
                    );
                    let size = widgets::cover_slot(&m, rect);
                    let id = art(&mut self.art, cx, &book.key, (size.w, size.h));
                    let info = self.art.get(&id).and_then(|a| a.as_ref());
                    let subtitle = info
                        .map(|(m, _)| m.author.as_str())
                        .filter(|s| !s.is_empty())
                        .unwrap_or("点按继续阅读");
                    let mut ink = Ink {
                        fonts: cx.fonts,
                        glyphs: cx.glyphs,
                        chain: &cx.app.ui_fonts,
                    };
                    widgets::list_row_cover(
                        &mut ink,
                        frame,
                        &cx.theme,
                        &m,
                        rect,
                        info.map(|(_, i)| i),
                        &book.title,
                        subtitle,
                        if book.favorite {
                            "收藏"
                        } else {
                            "最近阅读"
                        },
                    );
                    cx.hits.add(HitId(200 + i as u32), rect).long_press();
                }
            }
            pager(cx, frame, self.page, self.pages, m.tab_bar_h());
            draw_tabs(
                cx,
                frame,
                if self.discover {
                    2
                } else if self.history {
                    1
                } else {
                    0
                },
            );
        } else {
            let h = TextEditor::height(&m);
            let mut ink = Ink {
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
                Rect::new(0, cx.height.saturating_sub(h) as i32, width, h),
                "小说关键词",
                "搜索",
            );
        }
    }
    fn on_input(&mut self, cx: &mut Cx<Kindlia>, event: &InputEvent) -> Transition<Kindlia> {
        // 按下反馈由运行时负责；等完整手势再修改页面，避免按下/抬起各整页刷新一次。
        if matches!(event, InputEvent::Gesture(g) if g.kind == kn_platform::GestureKind::Down) {
            return Transition::None;
        }
        if let Some((field, picker)) = &mut self.picker {
            match picker.input(cx, event) {
                choice::Event::Selected(n) => {
                    let field = *field;
                    self.picker = None;
                    if field == 0 && n == 7 {
                        return Transition::Push(Box::new(filters::FilterPage::new(
                            self.query.clone(),
                        )));
                    }
                    filters::apply(field, n, &mut self.query);
                    self.page = 0;
                    if !self.editing {
                        self.load(cx, true);
                    }
                    cx.request_redraw(RefreshHint::Clean);
                }
                choice::Event::Cancel => {
                    self.picker = None;
                    cx.request_redraw(RefreshHint::Clean);
                }
                _ => {}
            }
            return Transition::None;
        }
        if !self.editing && !self.discover {
            if let InputEvent::Gesture(g) = event {
                if g.kind == GestureKind::Long {
                    if let Some(id) = cx
                        .hits
                        .at(kn_render::Point {
                            x: g.end.0,
                            y: g.end.1,
                        })
                        .filter(|id| id.0 >= 200 && id.0 < 1000)
                    {
                        if self.shelf_source > 0 && !self.history {
                            if let Some(folder) = self
                                .folders
                                .get(self.page * self.rows + (id.0 - 200) as usize)
                            {
                                return Transition::Push(Box::new(manage::ManagePage::folder(
                                    self.shelf_source == 2,
                                    folder["id"].as_str().unwrap_or("").into(),
                                    folder["title"].as_str().unwrap_or("").into(),
                                )));
                            }
                            return Transition::None;
                        }
                        if let Some(book) = self
                            .shelf
                            .get(self.page * self.rows + (id.0 - 200) as usize)
                        {
                            return Transition::Push(Box::new(BookPage::new(book.key.clone())));
                        }
                    }
                }
            }
        }
        if self.editing {
            if let InputEvent::Gesture(g) = event {
                if let Some(id) = cx
                    .hits
                    .at(kn_render::Point {
                        x: g.end.0,
                        y: g.end.1,
                    })
                    .filter(|id| self.editor.owns(*id))
                {
                    match self.editor.on_gesture(id, g.kind) {
                        EditorEvent::Submit => {
                            self.query.query = self.editor.text();
                            self.editing = false;
                            self.page = 0;
                            self.load(cx, true);
                        }
                        EditorEvent::Edited => cx.request_redraw(RefreshHint::Ui),
                        _ => {}
                    }
                    return Transition::None;
                }
            }
        }
        if let Some(t) = tab_input(
            cx,
            event,
            if self.discover {
                2
            } else if self.history {
                1
            } else {
                0
            },
        ) {
            return t;
        }
        match hit(cx, event) {
            Some(40) => {
                if let Some(book) = self.shelf.iter().find(|b| b.progress.is_some()) {
                    return Transition::Push(Box::new(reader::ReaderPage::new(
                        book.key.clone(),
                        book.progress.as_ref().unwrap().chapter_id.clone(),
                    )));
                }
            }
            Some(50) => {}
            Some(51) => {
                return Transition::Push(Box::new(cloud::CloudPage::new(cloud::Mode::Wenku(
                    String::new(),
                    0,
                ))))
            }
            Some(52) => return Transition::Push(Box::new(ranks::RankCategories)),
            Some(63) => {
                if self.notice == "再次点击清除历史以确认" {
                    let store = cx.app.repo.store.clone();
                    self.loading = true;
                    cx.spawn_local(move || ClearResult(store.clear_history()));
                } else {
                    self.notice = "再次点击清除历史以确认".into();
                }
                cx.request_redraw(RefreshHint::Ui);
            }
            Some(60) if !cx.app.repo.api.signed_in() => {
                return Transition::Push(Box::new(login::LoginPage::new()))
            }
            Some(61) if !cx.app.repo.api.signed_in() => {
                return Transition::Push(Box::new(login::LoginPage::new()))
            }
            Some(60) => {
                if self.history {
                    return Transition::Push(Box::new(cloud::CloudPage::new(cloud::Mode::History)));
                }
                self.shelf_source = 1;
                self.page = 0;
                self.load(cx, true);
            }
            Some(61) => {
                if self.history {
                    return Transition::Push(Box::new(cloud::CloudPage::new(cloud::Mode::History)));
                }
                self.shelf_source = 2;
                self.page = 0;
                self.load(cx, true);
            }
            Some(62) if !cx.app.repo.api.signed_in() => {
                return Transition::Push(Box::new(login::LoginPage::new()))
            }
            Some(62) => {
                return Transition::Push(Box::new(cloud::CloudPage::new(cloud::Mode::Folders(
                    self.shelf_source == 2,
                    None,
                ))))
            }
            Some(100) => return Transition::Root(Box::new(Self::shelf())),
            Some(101) => return Transition::Root(Box::new(Self::discover())),
            Some(102) => return Transition::Root(Box::new(SettingsPage::new(cx.app))),
            Some(1) if self.searching => {
                if self.return_on_close {
                    return Transition::Back;
                }
                self.searching = false;
                self.editing = false;
                self.query.query.clear();
                self.page = 0;
                self.load(cx, true);
            }
            Some(id) if (71..76).contains(&id) => {
                let field = (id - 71) as usize;
                let (name, options, selected) = filters::options(field, &self.query);
                self.picker = Some((field, choice::Choice::new(&name, options, Some(selected))));
                cx.request_redraw(RefreshHint::Ui);
            }
            Some(20) => {
                self.searching = true;
                self.editing = true;
                self.editor.set_text(&self.query.query);
                cx.request_redraw(RefreshHint::Ui);
            }
            Some(21) => {
                return Transition::Push(Box::new(filters::FilterPage::new(self.query.clone())))
            }
            Some(22) => {
                let (name, options, selected) = filters::options(4, &self.query);
                self.picker = Some((4, choice::Choice::new(&name, options, Some(selected))));
            }
            Some(23) => {
                self.shelf_source = 0;
                self.page = 0;
                self.load(cx, true);
            }
            Some(24) => return Transition::Exit,
            Some(25) => {
                self.editing = false;
                cx.request_redraw(RefreshHint::Ui);
            }
            Some(2) if self.page > 0 && !self.loading => {
                self.page -= 1;
                self.load(cx, false);
            }
            Some(3) if self.page + 1 < self.pages && !self.loading => {
                self.page += 1;
                self.load(cx, false);
            }
            Some(id) if id >= 200 && id < 1000 => {
                let i = (id - 200) as usize;
                if self.discover {
                    if let Some(book) = self.books.get(i) {
                        return Transition::Push(Box::new(BookPage::new(book.key())));
                    }
                } else if self.shelf_source > 0 && !self.history {
                    if let Some(folder) = self.folders.get(self.page * self.rows + i) {
                        return Transition::Push(Box::new(cloud::CloudPage::new(
                            cloud::Mode::Favorites(
                                self.shelf_source == 2,
                                folder["id"].as_str().unwrap_or("").into(),
                            ),
                        )));
                    }
                } else if let Some(book) = self.shelf.get(self.page * self.rows + i) {
                    if let Some(progress) = &book.progress {
                        return Transition::Push(Box::new(super::reader::ReaderPage::new(
                            book.key.clone(),
                            progress.chapter_id.clone(),
                        )));
                    }
                    return Transition::Push(Box::new(BookPage::new(book.key.clone())));
                }
            }
            _ => {}
        }
        if !self.editing && !self.loading {
            if let InputEvent::Gesture(g) = event {
                let next = matches!(g.kind, GestureKind::SwipeUp | GestureKind::SwipeLeft);
                let prev = matches!(g.kind, GestureKind::SwipeDown | GestureKind::SwipeRight);
                if next && self.page + 1 < self.pages {
                    self.page += 1;
                    self.load(cx, false);
                }
                if prev && self.page > 0 {
                    self.page -= 1;
                    self.load(cx, false);
                }
            }
        }
        Transition::None
    }
    fn on_message(&mut self, cx: &mut Cx<Kindlia>, msg: Box<dyn Any + Send>) {
        let msg = match msg.downcast::<ArtResult>() {
            Ok(result) => {
                if let Ok(image) = result.2 {
                    if !self.art.contains_key(&result.0) {
                        return;
                    }
                    self.art.insert(result.0, Some((result.1, image)));
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
                        self.notice.clear();
                    }
                    Err(e) => self.notice = e,
                }
                cx.request_redraw(RefreshHint::Ui);
                return;
            }
            Err(m) => m,
        };
        if let Ok(result) = msg.downcast::<ShelfResult>() {
            if self.shelf_source == 0 || self.history {
                self.loading = false;
            }
            match result.0 {
                Ok(entries) => {
                    self.shelf = if self.history {
                        entries
                            .into_iter()
                            .filter(|b| b.progress.is_some())
                            .collect()
                    } else {
                        entries
                    };
                    if self.shelf_source == 0 || self.history {
                        self.pages = self.shelf.len().div_ceil(self.rows).max(1);
                    }
                    self.page = self.page.min(self.pages - 1);
                    if self.shelf_source == 0 || self.history {
                        self.notice = if self.shelf.is_empty() {
                            "书架为空，请到发现页选书".into()
                        } else {
                            "本地收藏与最近阅读".into()
                        };
                    }
                }
                Err(e) => self.notice = e,
            }
            cx.request_redraw(RefreshHint::Ui);
        }
    }
}
