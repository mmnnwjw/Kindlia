use super::*;
use crate::{
    model::{preferred, Novel, NovelKey},
    repository::Loaded,
    Kindlia,
};
use kn_platform::InputEvent;
use kn_render::{Bitmap, Rect};
use kn_ui::{Cx, Page, PageId, RefreshHint, Transition};
use std::any::Any;

struct NovelResult(Result<Loaded<Novel>, String>);
struct FavoriteResult(Result<(), String>);
struct Mutation(Result<(), String>);
struct CoverResult(Result<Bitmap, String>);
pub struct BookPage {
    key: NovelKey,
    novel: Option<Novel>,
    notice: String,
    loading: bool,
    favorite: bool,
    catalog: bool,
    from_reader: bool,
    page: usize,
    rows: usize,
    pending_back: bool,
    cover: Option<Bitmap>,
    tags: Option<choice::Choice>,
    confirm_remove: Option<(String, choice::Choice)>,
    refresh_favorite: bool,
    current_chapter: Option<String>,
    located: bool,
}
impl BookPage {
    pub fn new(key: NovelKey) -> Self {
        Self {
            key,
            novel: None,
            notice: String::new(),
            loading: false,
            favorite: false,
            catalog: false,
            from_reader: false,
            page: 0,
            rows: 6,
            pending_back: false,
            cover: None,
            tags: None,
            confirm_remove: None,
            refresh_favorite: false,
            current_chapter: None,
            located: false,
        }
    }
    pub fn catalog(key: NovelKey, novel: Option<Novel>) -> Self {
        Self {
            novel,
            catalog: true,
            from_reader: true,
            ..Self::new(key)
        }
    }
    fn load(&mut self, cx: &mut Cx<Kindlia>, refresh: bool) {
        self.loading = true;
        self.notice = "加载中…".into();
        let repo = cx.app.repo.clone();
        let key = self.key.clone();
        crate::repository::read_on_workers(
            cx,
            repo,
            refresh,
            move |r| r.novel(&key, refresh),
            NovelResult,
        );
        cx.request_redraw(RefreshHint::Ui);
    }
    fn open(&self, id: String, cx: &mut Cx<Kindlia>) -> Transition<Kindlia> {
        if self.from_reader {
            cx.app.reader_jump = Some(id);
            Transition::Back
        } else {
            Transition::Push(Box::new(reader::ReaderPage::new(self.key.clone(), id)))
        }
    }
}
impl Page<Kindlia> for BookPage {
    fn id(&self) -> PageId {
        PageId(20)
    }
    fn enter(&mut self, cx: &mut Cx<Kindlia>, _returning: bool) {
        self.current_chapter = cx.app.repo.store.progress(&self.key).map(|p| p.chapter_id);
        if self.from_reader && cx.app.reader_jump.is_some() {
            self.pending_back = true;
            return;
        }
        self.favorite = cx
            .app
            .repo
            .store
            .shelf()
            .unwrap_or_default()
            .iter()
            .any(|e| e.key == self.key && e.favorite);
        if self.novel.is_none() || self.refresh_favorite {
            let refresh = self.refresh_favorite;
            self.refresh_favorite = false;
            self.load(cx, refresh);
        }
    }
    fn wants_backdrop(&self) -> kn_ui::Backdrop {
        self.tags
            .as_ref()
            .map(|p| p.wants_backdrop())
            .or_else(|| self.confirm_remove.as_ref().map(|(_, p)| p.wants_backdrop()))
            .unwrap_or(kn_ui::Backdrop::None)
    }
    fn set_backdrop(&mut self, bg: Option<Bitmap>) {
        if let Some(p) = self.tags.as_mut() {
            p.set_backdrop(bg);
        } else if let Some((_, p)) = self.confirm_remove.as_mut() {
            p.set_backdrop(bg);
        }
    }
    fn render(&mut self, cx: &mut Cx<Kindlia>, frame: &mut Bitmap) {
        if let Some(tags) = &mut self.tags {
            tags.render(cx, frame);
            return;
        }
        if let Some((_, c)) = &mut self.confirm_remove {
            c.render(cx, frame);
            return;
        }
        header(
            cx,
            frame,
            if self.catalog {
                "目录"
            } else {
                "小说详情"
            },
            true,
        );
        let m = cx.app.metrics;
        let w = cx.width;
        let top = m.header_h();
        if !self.catalog {
            if let Some(novel) = &self.novel {
                let author = novel
                    .authors
                    .iter()
                    .map(|a| a.name.as_str())
                    .collect::<Vec<_>>()
                    .join(" / ");
                let total = novel.toc.iter().filter(|c| c.chapter_id.is_some()).count();
                let meta = vec![
                    format!(
                        "{} · {} 章",
                        novel.novel_type.as_deref().unwrap_or("Web小说"),
                        total
                    ),
                    format!(
                        "{} · {}",
                        crate::formatting::characters(novel.total_characters),
                        crate::formatting::updated(
                            novel.toc.iter().filter_map(|c| c.create_at).max()
                        )
                    ),
                    format!("标签：{}", novel.keywords.join("、")),
                ];
                let resume = cx.app.repo.store.progress(&self.key).is_some();
                let cloud = cx.app.repo.api.signed_in();
                detail::render(
                    cx,
                    frame,
                    detail::Detail {
                        title: novel.title(),
                        author: &author,
                        meta: &meta,
                        intro: preferred(novel.introduction_zh.as_deref(), &novel.introduction_jp),
                        notice: &self.notice,
                    },
                    self.cover.as_ref(),
                    &[
                        (
                            if resume {
                                "继续阅读"
                            } else {
                                "开始阅读"
                            },
                            10,
                            total > 0,
                        ),
                        ("目录", 11, total > 0),
                        (
                            if cloud && novel.favored.is_some() {
                                "移出收藏"
                            } else if self.favorite && !cloud {
                                "移出书架"
                            } else {
                                "加入书架"
                            },
                            if cloud { 14 } else { 12 },
                            true,
                        ),
                        ("评论", 15, true),
                    ],
                    16,
                );
            } else {
                let area = Rect::new(0, top as i32, w, cx.height.saturating_sub(top));
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
                        Some(("重试", HitId(13)))
                    },
                );
            }
            return;
        }
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
            Rect::new(m.margin as i32, top as i32, w - 2 * m.margin, m.touch),
            &["目录", "书签"],
            0,
            70,
        );
        let start = top + m.touch;
        if let Some(novel) = &self.novel {
            if self.catalog {
                self.rows =
                    (cx.height.saturating_sub(start + m.pager_h()) / m.row_h()).max(1) as usize;
                let pages = novel.toc.len().div_ceil(self.rows).max(1);
                if !self.located {
                    self.located = true;
                    if let Some(i) = novel.toc.iter().position(|c| {
                        c.chapter_id.is_some() && c.chapter_id == self.current_chapter
                    }) {
                        self.page = i / self.rows;
                    }
                }
                self.page = self.page.min(pages - 1);
                for (i, item) in novel
                    .toc
                    .iter()
                    .skip(self.page * self.rows)
                    .take(self.rows)
                    .enumerate()
                {
                    row(
                        cx,
                        frame,
                        Rect::new(0, (start + m.row_h() * i as u32) as i32, w, m.row_h()),
                        item.title(),
                        &if item.chapter_id.is_some() {
                            crate::formatting::date(item.create_at)
                        } else {
                            "分卷".into()
                        },
                        if item.chapter_id.is_some() && item.chapter_id == self.current_chapter {
                            "当前"
                        } else {
                            ""
                        },
                        item.chapter_id.as_ref().map(|_| 200 + i as u32),
                    );
                }
                pager(cx, frame, self.page, pages, 0);
            }
        }
        tips::show(
            cx,
            frame,
            &tips::BOOKMARKS,
            self.catalog && self.from_reader && self.tags.is_none() && self.confirm_remove.is_none(),
        );
    }
    fn on_input(&mut self, cx: &mut Cx<Kindlia>, event: &InputEvent) -> Transition<Kindlia> {
        if self.catalog && self.from_reader && tips::input(cx, event, &tips::BOOKMARKS) {
            return Transition::None;
        }
        // 按下反馈由运行时负责；等完整手势再修改页面，避免按下/抬起各整页刷新一次。
        if matches!(event, InputEvent::Gesture(g) if g.kind == kn_platform::GestureKind::Down) {
            return Transition::None;
        }
        if let Some(tags) = &mut self.tags {
            match tags.input(cx, event) {
                choice::Event::Selected(n) => {
                    let tag = self
                        .novel
                        .as_ref()
                        .and_then(|book| book.keywords.get(n))
                        .cloned();
                    self.tags = None;
                    if let Some(tag) = tag {
                        return Transition::Push(Box::new(library::LibraryPage::search(format!(
                            "{tag}$"
                        ))));
                    }
                }
                choice::Event::Cancel => self.tags = None,
                _ => {}
            }
            cx.request_redraw(RefreshHint::Clean);
            return Transition::None;
        }
        if let Some((path, confirm)) = &mut self.confirm_remove {
            match confirm.input(cx, event) {
                choice::Event::Selected(1) => {
                    let path = path.clone();
                    self.confirm_remove = None;
                    if cx.app.repo.offline {
                        self.notice = "离线模式不可修改云端数据".into();
                    } else {
                        self.loading = true;
                        let api = cx.app.repo.api.account_snapshot();
                        cx.spawn(move || Mutation(api.mutate("DELETE", &path, &serde_json::Value::Null)));
                    }
                }
                choice::Event::Selected(_) | choice::Event::Cancel => self.confirm_remove = None,
                _ => {}
            }
            cx.request_redraw(RefreshHint::Clean);
            return Transition::None;
        }
        match if self.catalog {
            page_hit(cx, event)
        } else {
            hit(cx, event)
        } {
            Some(1) => {
                if self.catalog && !self.from_reader {
                    self.catalog = false;
                    self.page = 0;
                    cx.request_redraw(RefreshHint::Ui);
                } else {
                    return Transition::Back;
                }
            }
            Some(10) => {
                if let Some(novel) = &self.novel {
                    let id = cx
                        .app
                        .repo
                        .store
                        .progress(&self.key)
                        .map(|p| p.chapter_id)
                        .filter(|id| novel.toc.iter().any(|i| i.chapter_id.as_ref() == Some(id)))
                        .or_else(|| novel.toc.iter().find_map(|i| i.chapter_id.clone()));
                    if let Some(id) = id {
                        return self.open(id, cx);
                    }
                }
            }
            Some(11) => {
                self.catalog = !self.catalog;
                self.located = false;
                self.page = 0;
                cx.request_redraw(RefreshHint::Ui);
            }
            Some(12) => {
                if let Some(novel) = &self.novel {
                    let store = cx.app.repo.store.clone();
                    let key = self.key.clone();
                    let title = novel.title().to_owned();
                    let favorite = !self.favorite;
                    cx.spawn_local(move || {
                        FavoriteResult(store.update_entry(&key, &title, |e| e.favorite = favorite))
                    });
                }
            }
            Some(71) | Some(17) => {
                return Transition::Push(Box::new(bookmarks::BookmarksPage::catalog(
                    self.key.clone(),
                )))
            }
            Some(13) => self.load(cx, true),
            Some(19) => {
                if let Some(a) = self.novel.as_ref().and_then(|n| n.authors.first()) {
                    let q = if a.name.contains([' ', '　']) {
                        format!("\"{}\"", a.name)
                    } else {
                        a.name.clone()
                    };
                    return Transition::Push(Box::new(library::LibraryPage::search(q)));
                }
            }
            Some(18) => {
                if let Some(book) = &self.novel {
                    self.tags = Some(choice::Choice::new(
                        "选择标签搜索",
                        book.keywords.clone(),
                        None,
                    ));
                    cx.request_redraw(RefreshHint::Ui);
                }
            }
            Some(14) => {
                if !cx.app.repo.api.signed_in() {
                    return Transition::Push(Box::new(login::LoginPage::new()));
                }
                if let Some(folder) = self.novel.as_ref().and_then(|n| n.favored.as_deref()) {
                    let mut path = format!(
                        "user/favored-web/{}/{}/{}",
                        crate::api::encode(folder),
                        crate::api::encode(&self.key.provider_id),
                        crate::api::encode(&self.key.novel_id)
                    );
                    if self.key.provider_id == "epub" {
                        if let Some(v) = cx
                            .app
                            .repo
                            .store
                            .read_cache::<serde_json::Value>("epub-source", &self.key)
                        {
                            let wenku_id = v["novel"].as_str().unwrap_or("");
                            path = format!(
                                "user/favored-wenku/{}/{}",
                                crate::api::encode(folder),
                                crate::api::encode(wenku_id)
                            );
                        }
                    }
                    self.confirm_remove = Some((
                        path,
                        choice::Choice::new("从云收藏移出？", ["取消", "移出"], Some(0)),
                    ));
                    cx.request_redraw(RefreshHint::Ui);
                    return Transition::None;
                }
                self.refresh_favorite = true;
                let mut key = self.key.clone();
                if key.provider_id == "epub" {
                    if let Some(v) = cx
                        .app
                        .repo
                        .store
                        .read_cache::<serde_json::Value>("epub-source", &key)
                    {
                        key.provider_id = "wenku".into();
                        key.novel_id = v["novel"].as_str().unwrap_or("").into();
                    }
                }
                return Transition::Push(Box::new(cloud::CloudPage::new(cloud::Mode::Folders(
                    key.provider_id == "wenku",
                    Some(key),
                ))));
            }
            Some(15) => {
                let subject = if self.key.provider_id == "epub" {
                    cx.app
                        .repo
                        .store
                        .read_cache::<serde_json::Value>("epub-source", &self.key)
                        .and_then(|v| v["novel"].as_str().map(|s| format!("wenku-{s}")))
                        .unwrap_or_default()
                } else {
                    format!("web-{}-{}", self.key.provider_id, self.key.novel_id)
                };
                return Transition::Push(Box::new(cloud::CloudPage::new(cloud::Mode::Comments(
                    subject, None,
                ))));
            }
            Some(16) => {
                if let Some(n) = &self.novel {
                    return Transition::Push(Box::new(text::TextPage::new(
                        n.title(),
                        preferred(n.introduction_zh.as_deref(), &n.introduction_jp),
                    )));
                }
            }
            Some(2) => {
                self.page = self.page.saturating_sub(1);
                cx.request_redraw(RefreshHint::Ui);
            }
            Some(3)
                if self.page + 1
                    < self
                        .novel
                        .as_ref()
                        .map_or(0, |n| n.toc.len().div_ceil(self.rows)) =>
            {
                self.page += 1;
                cx.request_redraw(RefreshHint::Ui);
            }
            Some(id) if id >= 200 => {
                if let Some(novel) = &self.novel {
                    if let Some(id) = novel
                        .toc
                        .get(self.page * self.rows + (id - 200) as usize)
                        .and_then(|i| i.chapter_id.clone())
                    {
                        return self.open(id, cx);
                    }
                }
            }
            _ => {}
        }
        Transition::None
    }
    fn take_transition(&mut self) -> Transition<Kindlia> {
        if self.pending_back {
            self.pending_back = false;
            Transition::Back
        } else {
            Transition::None
        }
    }
    fn on_message(&mut self, cx: &mut Cx<Kindlia>, msg: Box<dyn Any + Send>) {
        let msg = match msg.downcast::<NovelResult>() {
            Ok(result) => {
                self.loading = false;
                match result.0 {
                    Ok(loaded) => {
                        let key = self.key.clone();
                        let r = detail::cover_rect(cx);
                        crate::covers::spawn(cx, key, (r.w, r.h), move |_, image| {
                            CoverResult(image.or_else(|_| crate::covers::fallback((r.w, r.h))))
                        });
                        self.novel = Some(loaded.value);
                        self.notice = loaded.notice.unwrap_or_default();
                    }
                    Err(e) => self.notice = e,
                }
                cx.request_redraw(RefreshHint::Ui);
                return;
            }
            Err(msg) => msg,
        };
        let msg = match msg.downcast::<CoverResult>() {
            Ok(result) => {
                self.cover = result.0.ok();
                cx.request_redraw(RefreshHint::Ui);
                return;
            }
            Err(msg) => msg,
        };
        let msg = match msg.downcast::<FavoriteResult>() {
            Ok(result) => {
                match result.0 {
                    Ok(()) => {
                        self.favorite = !self.favorite;
                        self.notice.clear();
                        cx.toast("书架已保存");
                    }
                    Err(e) => self.notice = e,
                }
                cx.request_redraw(RefreshHint::Ui);
                return;
            }
            Err(msg) => msg,
        };
        if let Ok(result) = msg.downcast::<Mutation>() {
            self.loading = false;
            match result.0 {
                Ok(()) => {
                    self.refresh_favorite = false;
                    cx.toast("已移出收藏");
                    self.load(cx, true);
                }
                Err(e) => {
                    self.notice = e;
                    cx.request_redraw(RefreshHint::Ui);
                }
            }
        }
    }
}
