//! 复用 kn-text 渐进分页与 kn-ui 生命周期，业务标识全部使用 AutoNovel 字符串键。
use super::*;
use crate::{
    model::{Chapter, Novel, NovelKey},
    paragraphs,
    repository::Loaded,
    store::Progress,
    Kindlia,
};
use kn_platform::{GestureKind, InputEvent, KeyCode, LightProp};
use kn_render::{Bitmap, FontId, Rect, Synth, TextStyle};
use kn_text::{Block, FontMeasure, LayoutParams, PageItem, Paginator, WidthCache};
use kn_text::{Inline, RichLine};
use kn_ui::{
    widgets::{self, ButtonStyle, Ink},
    HitId,
};
use kn_ui::{Cx, Orientation, Page, PageId, RefreshHint, Transition};
use std::{
    any::Any,
    collections::{HashMap, HashSet},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

struct ChapterResult(
    u64,
    Result<(Loaded<Chapter>, Option<kn_text::Document>), String>,
);
struct VolumeResult(u64, Result<NovelKey, String>);
struct NovelResult(Result<Loaded<Novel>, String>);
struct ImageResult(String, Result<Bitmap, String>);
struct Prefetched;
struct FontResult(String, Result<Vec<u8>, String>, f64);
struct CachedChapter {
    id: String,
    signature: String,
    chapter: Chapter,
    document: Option<kn_text::Document>,
    blocks: std::sync::Arc<[Block]>,
    pages: Vec<kn_text::Page>,
    layout: LayoutParams,
    index: usize,
    paginator: Option<Paginator>,
    done: bool,
}
fn text_signature(c: &crate::store::Config) -> String {
    format!(
        "{}:{}:{}:{}:{}:{}",
        c.translation,
        c.show_original,
        c.show_translation,
        c.original_size,
        c.original_gray,
        c.convert
    )
}
fn page_signature(cx: &Cx<Kindlia>) -> String {
    let c=&cx.app.config;
    crate::model::digest(&(text_signature(c),cx.width,cx.height,c.font_size,c.line_spacing,c.margin,
        &c.reader_font,c.paragraph_spacing,c.first_line_indent,c.landscape,&c.landscape_side))
}
enum ImageSlot {
    Loading,
    Ready(Bitmap),
    Failed,
}

pub struct ReaderPage {
    key: NovelKey,
    chain: Vec<FontId>,
    font_name: String,
    font_loading: Option<String>,
    document: Option<kn_text::Document>,
    chrome_rects: Vec<Rect>,
    track: Rect,
    config_signature: String,
    text_signature: String,
    chapter_cache: Vec<CachedChapter>,
    prefetched: HashSet<String>,
    chapter_id: String,
    chapter: Option<Chapter>,
    novel: Option<Novel>,
    blocks: std::sync::Arc<[Block]>,
    pages: Vec<kn_text::Page>,
    paginator: Option<Paginator>,
    widths: WidthCache,
    index: usize,
    done: bool,
    menu: bool,
    notice: String,
    generation: u64,
    layout: Option<LayoutParams>,
    target: Option<(String, usize)>,
    images: HashMap<String, ImageSlot>,
    loading: bool,
    load_started: Option<std::time::Instant>,
    progress_store: Option<crate::store::Store>,
    bookmarks: Vec<(String, String, usize)>,
    saved_anchor: Option<(NovelKey, String, String, usize)>,
    page_bitmaps: Vec<(usize, Bitmap)>,
    target_seen: bool,
    bitmap_style: String,
    rich: bool,
    tracker: crate::stats::Tracker,
    stats_key: Option<String>,
}
impl ReaderPage {
    fn stats_book(&mut self, cx: &Cx<Kindlia>) -> (String, String) {
        let id = self.stats_key.get_or_insert_with(|| {
            let source = (self.key.provider_id == "epub")
                .then(|| cx.app.repo.store.read_cache::<serde_json::Value>("epub-source", &self.key))
                .flatten();
            let key = source.as_ref().and_then(|v| v["novel"].as_str())
                .map(|id| NovelKey { provider_id: "wenku".into(), novel_id: id.into() })
                .unwrap_or_else(|| self.key.clone());
            crate::model::digest(&key)
        }).clone();
        let title = self.novel.as_ref().map(|n| n.title().to_owned())
            .or_else(|| self.chapter.as_ref().and_then(|c| {
                c.novel_title_zh.as_ref().filter(|s| !s.is_empty()).or(c.novel_title_jp.as_ref()).cloned()
            })).unwrap_or_else(|| self.key.novel_id.clone());
        (id, title)
    }
    fn stop_tracking(&mut self, cx: &Cx<Kindlia>) {
        if self.tracker.is_running() {
            let (id, title) = self.stats_book(cx);
            self.tracker.stop(&cx.app.repo.store, &id, &title, false);
        }
    }
    fn start_tracking(&mut self) {
        if self.chapter.is_some() && !self.pages.is_empty() && self.target.is_none()
            && !self.loading && self.font_loading.is_none() {
            self.tracker.start();
        }
    }
    fn page_label(&self) -> String {
        format!(
            "{} / {}{}",
            self.index + 1,
            self.pages.len().max(1),
            if self.done { "" } else { "+" }
        )
    }
    fn bookmark_on_page(&self) -> Option<usize> {
        let (path, offset) = self.anchor()?;
        self.bookmarks
            .iter()
            .position(|(c, p, o)| c == &self.chapter_id && p == &path && *o == offset)
    }
    fn bookmark(&mut self, cx: &mut Cx<Kindlia>) {
        if let Some((path, offset)) = self.anchor() {
            let store = &cx.app.repo.store;
            let mut bookmarks: Vec<(String, String, usize)> = store.bookmarks(&self.key);
            let entry = (self.chapter_id.clone(), path, offset);
            if let Some(i) = bookmarks.iter().position(|e| e == &entry) {
                bookmarks.remove(i);
            } else {
                bookmarks.push(entry);
            }
            match store.save_bookmarks(&self.key, &bookmarks) {
                Ok(()) => self.bookmarks = bookmarks,
                Err(e) => self.notice = e,
            }
        }
    }
    fn sync_history(&self, cx: &mut Cx<Kindlia>) {
        crate::history::start(cx);
    }
    fn draw_rich_line(
        &self,
        cx: &mut Cx<Kindlia>,
        bmp: &mut Bitmap,
        text: &str,
        x: f32,
        y: f32,
        line: &RichLine,
    ) {
        let theme = cx.theme;
        let metrics_chain = [cx.app.ui_fonts[0]];
        let ascent = |cx: &Cx<Kindlia>, size: u32| {
            cx.fonts
                .line_metrics(&TextStyle {
                    fonts: &metrics_chain,
                    size_px: size as f32,
                })
                .ascent
        };
        let baseline = (y + ascent(cx, line.max_size)).round();
        // 最近的注音基字 (x, 宽, 字号)
        let mut base: Option<(f32, f32, u32)> = None;
        for r in &line.runs {
            let s = r.style;
            if s.hidden {
                continue;
            }
            let color = if s.muted {
                let gray = cx.app.config.original_gray as i32;
                (theme.foreground as i32
                    + (theme.background as i32 - theme.foreground as i32) * gray / 255)
                    .clamp(0, 255) as u8
            } else {
                theme.foreground
            };
            let size = r.size as f32;
            let rx = x + r.x;
            let bl = baseline
                + match s.shift {
                    1 => -(line.max_size as f32 * 0.33).round(),
                    -1 => (line.max_size as f32 * 0.15).round(),
                    _ => 0.0,
                };
            match &r.object {
                Some(Inline::Note(_)) => {
                    // 注释图标: 圆角框里一个「注」(用系统字体: 章节字体的字形是打乱的)
                    let h = (size * 0.95).round();
                    let w = (r.width - size * 0.1).max(h * 0.8).round();
                    let rect = Rect::new(
                        (rx + size * 0.05).round() as i32,
                        (bl - size * 0.85).round() as i32,
                        w as u32,
                        h as u32,
                    );
                    let lw = ((size / 24.0).round() as u32).max(1);
                    bmp.rounded_rect(rect, (h * 0.2) as u32, None, Some(color), lw);
                    let chain = cx.app.ui_fonts.clone();
                    let mut ink = Ink {
                        fonts: &*cx.fonts,
                        glyphs: &mut *cx.glyphs,
                        chain: &chain,
                    };
                    ink.text_centered(bmp, rect, "注", size * 0.62, color);
                }
                Some(_) => {}
                None if s.ruby => {
                    let Some((bx, bw, bsize)) = base else {
                        continue;
                    };
                    let style = TextStyle {
                        fonts: &self.chain,
                        size_px: size,
                    };
                    let rt = &text[r.start..r.end];
                    let w = cx.fonts.measure(rt, &style);
                    let rbl = (baseline - ascent(cx, bsize) - size * 0.15).round();
                    cx.glyphs.draw_text_synth(
                        &*cx.fonts,
                        bmp,
                        (bx + (bw - w) / 2.0).round(),
                        rbl,
                        rt,
                        &style,
                        color,
                        Synth {
                            bold: s.bold,
                            italic: s.italic,
                        },
                    );
                }
                None => {
                    let style = TextStyle {
                        fonts: &self.chain,
                        size_px: size,
                    };
                    let run = &text[r.start..r.end];
                    cx.glyphs.draw_text_synth(
                        &*cx.fonts,
                        bmp,
                        rx,
                        bl,
                        run,
                        &style,
                        color,
                        Synth {
                            bold: s.bold,
                            italic: s.italic,
                        },
                    );
                    let thick = ((size / 20.0).round() as u32).max(1);
                    if s.underline {
                        bmp.fill_rect(
                            Rect::new(
                                rx.round() as i32,
                                (bl + size * 0.12).round() as i32,
                                r.width.round() as u32,
                                thick,
                            ),
                            color,
                        );
                    }
                    if s.strike {
                        bmp.fill_rect(
                            Rect::new(
                                rx.round() as i32,
                                (bl - size * 0.32).round() as i32,
                                r.width.round() as u32,
                                thick,
                            ),
                            color,
                        );
                    }
                    if s.dots {
                        // 着重号: 每个字下方居中一个点
                        let mut pen = rx;
                        let d = ((size * 0.14).round() as u32).max(2);
                        for ch in run.chars() {
                            let font = cx.fonts.pick_font(&self.chain, ch);
                            let adv = cx.fonts.advance(font, ch, size);
                            if !ch.is_whitespace() {
                                let dx = (pen + adv / 2.0 - d as f32 / 2.0).round() as i32;
                                bmp.rounded_rect(
                                    Rect::new(dx, (bl + size * 0.2).round() as i32, d, d),
                                    d / 2,
                                    Some(color),
                                    None,
                                    0,
                                );
                            }
                            pen += adv;
                        }
                    }
                    base = Some((rx, r.width, r.size));
                }
            }
        }
    }

    fn draw_header(&self, cx: &mut Cx<Kindlia>, frame: &mut Bitmap, top: i32) {
        let theme = cx.theme;
        let m = cx.app.metrics;
        let margin = self.layout.as_ref().map(|p| p.margin).unwrap_or(34) as i32;
        frame.fill_rect(Rect::new(0, 0, cx.width, top as u32), theme.background);
        let chain = cx.app.ui_fonts.clone();
        let mut ink = Ink {
            fonts: &*cx.fonts,
            glyphs: &mut *cx.glyphs,
            chain: &chain,
        };
        let style_top = (top - (m.tiny * 1.2) as i32) / 2 + 4;
        // 右侧: 时间 · 电量 (与其它页面顶栏一致, 每分钟随 on_minute 重绘, 只刷顶栏这一小块)
        let status = super::status_text();
        let status_w = ink.width(&status, m.tiny).ceil() as i32;
        let status_x = cx.width as i32 - margin - status_w;
        ink.text(frame, status_x, style_top, &status, m.tiny, theme.muted);
        // 左侧: 章节名, 后面隔一点接章内页码
        let label = self.page_label();
        let label_w = ink.width(&label, m.tiny).ceil() as i32;
        let gap = (m.tiny * 0.8) as i32;
        let title_w = (status_x - margin - margin / 2 - gap - label_w).max(0) as f32;
        let title = ink.fit(
            self.chapter
                .as_ref()
                .map(|c| c.title())
                .unwrap_or("加载章节"),
            m.tiny,
            title_w,
        );
        let title_end = margin
            + ink
                .text(frame, margin, style_top, &title, m.tiny, theme.muted)
                .ceil() as i32;
        let label_x = if title.is_empty() {
            margin
        } else {
            title_end + gap
        };
        ink.text(frame, label_x, style_top, &label, m.tiny, theme.muted);
        // 书签角标: 右侧页边距里, 从屏幕顶边垂下
        if self.bookmark_on_page().is_some() {
            let rw = (margin as f32 * 0.42).max(10.0) as u32;
            let rx = cx.width as i32 - margin / 2 - rw as i32 / 2;
            ribbon(
                frame,
                Rect::new(rx, 0, rw, (top as f32 * 0.95) as u32),
                true,
                2.0,
                theme.foreground,
            );
        }
        // 底部进度线
        if !self.pages.is_empty() {
            let y = cx.height as i32 - (margin / 2).max(10);
            let w = cx.width as i32 - 2 * margin;
            let total = if self.done {
                self.pages.len()
            } else {
                self.pages.len() + 1
            };
            let filled = (w as f32 * (self.index + 1) as f32 / total.max(1) as f32) as i32;
            frame.fill_rect(Rect::new(margin, y, w as u32, 2), theme.mid);
            frame.fill_rect(
                Rect::new(margin, y - 1, filled.max(0) as u32, 4),
                theme.foreground,
            );
        }
    }

    fn draw_chrome(&mut self, cx: &mut Cx<Kindlia>, frame: &mut Bitmap) {
        let theme = cx.theme;
        let m = cx.app.metrics;
        let (w, h) = (cx.width, cx.height);
        let chain = cx.app.ui_fonts.clone();
        let mut ink = Ink {
            fonts: &*cx.fonts,
            glyphs: &mut *cx.glyphs,
            chain: &chain,
        };

        // 顶栏: 返回 / 书名 / 书签 / 主页 (书名给书签按钮让出位置)
        let title = self
            .novel
            .as_ref()
            .map(|n| n.title().to_owned())
            .unwrap_or_else(|| "阅读".into());
        let bar_h = m.header_h() as i32;
        let title = ink.fit(
            &title,
            m.title,
            (w as i32 - 4 * bar_h - 2 * m.margin as i32).max(0) as f32,
        );
        let bar = widgets::header(&mut ink, frame, &theme, &m, &title, "", true, true);
        frame.fill_rect(Rect::new(0, bar.bottom(), w, 2), theme.foreground);
        cx.hits.add(HitId(1), Rect::new(0, 0, bar.h, bar.h));
        cx.hits.add(
            HitId(18),
            Rect::new(w as i32 - bar.h as i32, 0, bar.h, bar.h),
        );
        if self.chapter.is_some() {
            let mark = Rect::new(w as i32 - 2 * bar.h as i32, 0, bar.h, bar.h);
            let icon_h = (bar.h as f32 * 0.46) as u32;
            let icon_w = (icon_h as f32 * 0.68) as u32;
            let icon = Rect::new(
                mark.x + (mark.w - icon_w) as i32 / 2,
                (bar.h - icon_h) as i32 / 2,
                icon_w,
                icon_h,
            );
            ribbon(
                frame,
                icon,
                self.bookmark_on_page().is_some(),
                (4.0 * m.scale).max(3.0),
                theme.foreground,
            );
            cx.hits.add(HitId(26), mark).long_press();
        }
        let light_bottom = super::light::draw(
            &mut ink,
            frame,
            cx.hits,
            &theme,
            &m,
            &cx.app.light,
            w,
            bar.bottom() + 2,
        );
        self.chrome_rects = vec![Rect::new(0, 0, w, light_bottom as u32)];

        // 底部面板
        let pad = m.margin as i32 / 2;
        let gap = (m.margin as f32 * 0.5) as i32;
        let btn_h = m.touch as i32;
        let info_h = (m.small * 1.6) as i32;
        let track_h = (10.0 * m.scale).max(8.0) as i32;
        let panel_h = pad + info_h + gap / 2 + track_h + gap + btn_h + gap + btn_h + pad;
        let panel = Rect::new(0, h as i32 - panel_h, w, panel_h as u32);
        self.chrome_rects.push(panel);
        frame.fill_rect(panel, theme.background);
        frame.fill_rect(Rect::new(0, panel.y, w, 2), theme.foreground);
        cx.hits.add(HitId(28), panel).no_feedback();
        let side = m.margin as i32;
        let inner_w = w as i32 - 2 * side;

        let mut y = panel.y + pad;
        let pages = self.page_label() + " 页";
        let pages_w = ink.width(&pages, m.small).ceil() as i32;
        ink.text(
            frame,
            w as i32 - side - pages_w,
            y + (info_h - (m.small * 1.25) as i32) / 2,
            &pages,
            m.small,
            theme.muted,
        );
        let chapter_label = self
            .chapter
            .as_ref()
            .map(|c| c.title().to_owned())
            .unwrap_or_default();
        let chapter_label = ink.fit(
            &chapter_label,
            m.small,
            (inner_w - pages_w - side / 2) as f32,
        );
        ink.text(
            frame,
            side,
            y + (info_h - (m.small * 1.25) as i32) / 2,
            &chapter_label,
            m.small,
            theme.foreground,
        );
        y += info_h + gap / 2;

        // 章内进度条 (点按/滑动跳页; 触摸区域上下放宽到一个按钮高)
        let track = Rect::new(side, y, inner_w as u32, track_h as u32);
        self.track = track;
        let touch = Rect::new(
            side - gap,
            y + track_h / 2 - btn_h / 2,
            (inner_w + 2 * gap) as u32,
            btn_h as u32,
        );
        cx.hits.add(HitId(27), touch).no_feedback();
        let total = if self.done {
            self.pages.len()
        } else {
            self.pages.len() + 1
        };
        let filled = (inner_w as f32 * (self.index + 1) as f32 / total.max(1) as f32) as u32;
        frame.rounded_rect(
            track,
            track_h as u32 / 2,
            Some(theme.background),
            Some(theme.mid),
            2,
        );
        if filled > 0 {
            frame.rounded_rect(
                Rect::new(side, y, filled.max(track_h as u32), track_h as u32),
                track_h as u32 / 2,
                Some(theme.foreground),
                None,
                0,
            );
        }
        y += track_h + gap;

        // 上一章 / 目录 / 设置 / 下一章
        let cols = 4;
        let col_w = (inner_w - gap * (cols - 1)) / cols;
        let row = [
            (HitId(10), "上一章"),
            (HitId(11), "目录"),
            (HitId(17), "设置"),
            (HitId(12), "下一章"),
        ];
        for (i, (id, label)) in row.iter().enumerate() {
            let r = Rect::new(
                side + i as i32 * (col_w + gap),
                y,
                col_w as u32,
                btn_h as u32,
            );
            widgets::button(
                &mut ink,
                frame,
                &theme,
                &m,
                r,
                label,
                ButtonStyle::Secondary,
            );
            cx.hits.add(*id, r).rounded(m.radius);
        }
        y += btn_h + gap;

        // 字号 − 当前 + / 横竖屏 / 日夜
        let (min_font, max_font) = (24, 96);
        let size = cx.app.config.font_size.clamp(min_font, max_font);
        let cols = 5;
        let col_w = (inner_w - gap * (cols - 1)) / cols;
        let cell = |i: i32| Rect::new(side + i * (col_w + gap), y, col_w as u32, btn_h as u32);
        let down = cell(0);
        widgets::button(
            &mut ink,
            frame,
            &theme,
            &m,
            down,
            "A－",
            if size > min_font {
                ButtonStyle::Secondary
            } else {
                ButtonStyle::Disabled
            },
        );
        cx.hits
            .add(HitId(13), down)
            .rounded(m.radius)
            .enabled(size > min_font);
        ink.text_centered(
            frame,
            cell(1),
            &format!("字号 {size}"),
            m.small,
            theme.foreground,
        );
        let up = cell(2);
        widgets::button(
            &mut ink,
            frame,
            &theme,
            &m,
            up,
            "A＋",
            if size < max_font {
                ButtonStyle::Secondary
            } else {
                ButtonStyle::Disabled
            },
        );
        cx.hits
            .add(HitId(15), up)
            .rounded(m.radius)
            .enabled(size < max_font);
        let rotate = cell(3);
        widgets::button(
            &mut ink,
            frame,
            &theme,
            &m,
            rotate,
            if cx.app.config.landscape {
                "竖屏"
            } else {
                "横屏"
            },
            ButtonStyle::Secondary,
        );
        cx.hits.add(HitId(16), rotate).rounded(m.radius);
        let night = cell(4);
        widgets::button(
            &mut ink,
            frame,
            &theme,
            &m,
            night,
            if cx.app.config.night {
                "日间"
            } else {
                "夜间"
            },
            ButtonStyle::Secondary,
        );
        cx.hits.add(HitId(19), night).rounded(m.radius);

        // 提示条: 面板上方居中的反色圆角条
        if !self.notice.is_empty() {
            let text_w = ink.width(&self.notice, m.small).ceil() as u32;
            let nw = (text_w + 2 * m.margin).min(w - 2 * m.margin);
            let nh = (m.small * 2.2) as u32;
            let r = Rect::new((w - nw) as i32 / 2, panel.y - nh as i32 - gap, nw, nh);
            self.chrome_rects.push(r);
            frame.rounded_rect(r, nh / 2, Some(theme.foreground), None, 0);
            ink.text_centered(
                frame,
                r.inflate(-(m.margin as i32) / 2),
                &self.notice,
                m.small,
                theme.background,
            );
        }
    }

    pub fn new(key: NovelKey, chapter_id: String) -> Self {
        Self {
            key,
            chain: vec![],
            font_name: String::new(),
            font_loading: None,
            document: None,
            chrome_rects: vec![],
            track: Rect::default(),
            config_signature: String::new(),
            text_signature: String::new(),
            chapter_cache: vec![],
            prefetched: HashSet::new(),
            chapter_id,
            chapter: None,
            novel: None,
            blocks: std::sync::Arc::from([]),
            pages: vec![],
            paginator: None,
            widths: WidthCache::default(),
            index: 0,
            done: false,
            menu: false,
            notice: String::new(),
            generation: 0,
            layout: None,
            target: None,
            images: HashMap::new(),
            loading: false,
            load_started: None,
            progress_store: None,
            bookmarks: vec![],
            saved_anchor: None,
            page_bitmaps: vec![],
            target_seen: false,
            bitmap_style: String::new(),
            rich: false,
            tracker: crate::stats::Tracker::default(),
            stats_key: None,
        }
    }
    fn load(&mut self, cx: &mut Cx<Kindlia>, refresh: bool) {
        self.stop_tracking(cx);
        self.load_started = Some(std::time::Instant::now());
        self.generation += 1;
        self.loading = true;
        self.notice = "正在打开章节…".into();
        self.chapter = None;
        self.pages.clear();
        self.blocks = std::sync::Arc::from([]);
        self.paginator = None;
        self.layout = None;
        self.images.clear();
        self.page_bitmaps.clear();
        self.index = 0;
        self.done = false;
        let repo = cx.app.repo.clone();
        let key = self.key.clone();
        let id = self.chapter_id.clone();
        let generation = self.generation;
        let config = cx.app.config.clone();
        let local_repo = repo.clone();
        let local_key = key.clone();
        let local_id = id.clone();
        let poster = cx.page_poster();
        let spawner = cx.spawner();
        let work = move || {
            ChapterResult(
                generation,
                if key.provider_id == "epub" {
                    crate::epub::load_chapter(&repo.store, &key, &id)
                        .or_else(|error| {
                            if repo.offline {
                                return Err(format!("文库卷未缓存：{error}"));
                            }
                            let source = repo
                                .store
                                .read_cache::<serde_json::Value>("epub-source", &key)
                                .ok_or(error)?;
                            let mut original = config.clone();
                            original.translation =
                                source["translation"].as_str().unwrap_or("auto").into();
                            original.show_original = source["original"].as_bool().unwrap_or(false);
                            original.show_translation =
                                source["translated"].as_bool().unwrap_or(true);
                            let restored = super::cloud::open_volume(
                                &repo,
                                &original,
                                source["novel"].as_str().ok_or("缺少文库来源")?,
                                source["volume"].as_str().ok_or("缺少卷文件名")?,
                                source["human"].as_bool().unwrap_or(false),
                            )?;
                            if restored != key {
                                return Err("文库来源发生变化，请从详情页重新选卷".into());
                            }
                            crate::epub::load_chapter(&repo.store, &restored, &id)
                        })
                        .map(|(chapter, document)| {
                            (
                                Loaded {
                                    value: chapter,
                                    notice: None,
                                },
                                Some(document),
                            )
                        })
                } else {
                    repo.chapter(&key, &id, refresh)
                        .map(|chapter| (chapter, None))
                },
            )
        };
        // 缓存读取与 EPUB 解析先走本地队列，不能等待列表/预加载的网络请求。
        // 损坏或尚未下载的 EPUB 交回网络队列恢复，本地线程始终不做 HTTP。
        cx.spawn_local_droppable(move || {
            if local_key.provider_id == "epub" {
                match crate::epub::load_chapter(&local_repo.store, &local_key, &local_id) {
                    Ok((chapter, document)) => {
                        poster.post(ChapterResult(generation, Ok((Loaded {
                            value: chapter,
                            notice: None,
                        }, Some(document)))));
                        return;
                    }
                    Err(error) if local_repo.offline => {
                        poster.post(ChapterResult(generation, Err(format!("文库卷未缓存：{error}"))));
                        return;
                    }
                    Err(_) => {}
                }
            } else if local_repo.offline || !refresh {
                if let Some(chapter) = local_repo.store.read_cache("chapters", &(&local_key, &local_id)) {
                    poster.post(ChapterResult(generation, Ok((Loaded {
                        value: chapter,
                        notice: local_repo.offline.then(|| "离线缓存".into()),
                    }, None))));
                    return;
                }
                if local_repo.offline {
                    poster.post(ChapterResult(generation, Err("离线模式：此内容尚未缓存".into())));
                    return;
                }
            }
            if let Some(spawner) = spawner {
                spawner.spawn_on(kn_ui::Lane::Net, true, work);
            }
        });

        cx.request_redraw(RefreshHint::Ui);
    }
    fn anchor(&self) -> Option<(String, usize)> {
        if self.pages.is_empty() {
            None
        } else {
            Some(kn_text::first_anchor_on_page(
                &self.pages,
                self.index as i64,
            ))
        }
    }
    fn rebuild(&mut self, cx: &mut Cx<Kindlia>) {
        self.stop_tracking(cx);
        self.save(cx);
        if self.target.is_none() {
            self.target = self.anchor();
        }
        if let Some(doc) = &self.document {
            self.blocks = doc.blocks.clone().into();
        } else if let Some(chapter) = &self.chapter {
            let built = paragraphs::build_preferences(
                chapter,
                &cx.app.config.translation,
                cx.app.config.show_original,
                cx.app.config.show_translation,
            );
            self.blocks = built.blocks.into();
            if let Some(notice) = built.notice {
                self.notice = notice;
            }
        }
        self.rich = self.document.is_some() || self.blocks.iter().any(|b| !b.spans.is_empty());
        for b in std::sync::Arc::make_mut(&mut self.blocks) {
            crate::conversion::apply(b, &cx.app.config.convert);
            let reference = b.path.starts_with("/jp/")
                || (self.document.is_some()
                    && cx.app.config.show_original
                    && cx.app.config.show_translation
                    && !b.spans.is_empty()
                    && b.spans.iter().all(|s| s.style.muted));
            if reference {
                b.style.scale = ((b.style.scale as u32
                    * cx.app.config.original_size.clamp(60, 120) as u32)
                    / 100) as u16;
            }
        }
        self.text_signature = text_signature(&cx.app.config);
        self.layout = None;
    }
    fn prefetch(&mut self, cx: &mut Cx<Kindlia>) {
        if !cx.app.config.prefetch_chapters || cx.app.repo.offline || self.key.provider_id == "epub"
        {
            return;
        }
        for id in [self.adjacent(false), self.adjacent(true)]
            .into_iter()
            .flatten()
        {
            let tag = format!("{}:{id}", self.key.cache_id());
            if !self.prefetched.insert(tag) {
                continue;
            }
            let repo = cx.app.repo.clone();
            let key = self.key.clone();
            cx.spawn_droppable(move || {
                if let Err(e) = repo.chapter(&key, &id, false) {
                    eprintln!("[prefetch] {e}");
                }
                Prefetched
            });
        }
    }
    fn ensure_layout(&mut self, cx: &mut Cx<Kindlia>) {
        let style = format!(
            "{}:{}:{}:{}:{}",
            cx.theme.background,
            cx.width,
            cx.height,
            cx.app.config.original_gray,
            text_signature(&cx.app.config)
        );
        if self.bitmap_style != style {
            self.bitmap_style = style;
            self.page_bitmaps.clear();
        }
        if self.chapter.is_none() {
            return;
        }
        if self.chain.is_empty() || self.font_name != cx.app.config.reader_font {
            self.save(cx);
            if self.target.is_none() {
                self.target = self.anchor();
            }
            let name = cx.app.config.reader_font.clone();
            let cached = name.is_empty() || cx.app.reader_font_cache.as_ref().is_some_and(|(n, _)| *n == name);
            if !cached {
                if self.font_loading.as_ref() != Some(&name) {
                    self.stop_tracking(cx);
                    self.font_loading = Some(name.clone());
                    let root = cx.app.repo.store.root.clone();
                    cx.spawn_local_droppable(move || {
                        let at = std::time::Instant::now();
                        let bytes = crate::fonts::read(&root, &name);
                        FontResult(name, bytes, at.elapsed().as_secs_f64() * 1000.0)
                    });
                }
                self.notice = "正在加载字体…".into();
                return;
            }
            self.font_loading = None;
            let (chain, notice) = crate::fonts::select(
                cx.fonts,
                &cx.app.repo.store.root,
                &cx.app.config.reader_font,
                &cx.app.ui_fonts,
                cx.glyphs,
                &mut cx.app.reader_font_cache,
            );
            self.chain = chain;
            self.font_name = cx.app.config.reader_font.clone();
            self.layout = None;
            if let Some(n) = notice {
                self.notice = n;
            }
        }
        let scale = cx.app.dpi as f64 / 300.0;
        let params = LayoutParams {
            width: cx.width as i64,
            height: cx.height.saturating_sub(cx.app.metrics.touch) as i64,
            font_size: (cx.app.config.font_size as f64 * scale).round().max(12.0) as u32,
            margin: (cx.app.config.margin as f64 * scale).round() as i64,
            line_spacing: cx.app.config.line_spacing,
            first_line_indent: cx.app.config.first_line_indent,
            paragraph_spacing: cx.app.config.paragraph_spacing,
            rich: self.rich,
            strict_kinsoku: true,
            real_image_size: true,
            ..Default::default()
        };
        if self.layout.as_ref() == Some(&params) {
            return;
        }
        self.save(cx);
        if self.target.is_none() {
            self.target = self.anchor();
        }
        eprintln!(
            "[reader] paginate chapter={} viewport={}x{}",
            self.chapter_id, cx.width, cx.height
        );
        self.layout = Some(params.clone());
        self.target_seen = false;
        self.index = 0;
        self.pages.clear();
        self.done = false;
        self.images.clear();
        self.page_bitmaps.clear();
        self.widths.clear();
        self.paginator = Some(Paginator::new(self.blocks.clone(), params));
    }
    fn save(&mut self, cx: &mut Cx<Kindlia>) {
        if let Some(error) = cx.app.progress_writer.take_error() {
            self.notice = format!("进度保存失败，将重试：{error}");
            self.saved_anchor = None;
        }
        if self.target.is_some() || self.chapter.is_none() {
            return;
        }
        if let Some((path, offset)) = self.anchor() {
            let anchor = (
                self.key.clone(),
                self.chapter_id.clone(),
                path.clone(),
                offset,
            );
            if self.saved_anchor.as_ref() == Some(&anchor) {
                return;
            }
            let chapter = self.chapter.as_ref().unwrap();
            let title = self
                .novel
                .as_ref()
                .map(|n| n.title())
                .or(chapter.novel_title_zh.as_deref())
                .or(chapter.novel_title_jp.as_deref())
                .unwrap_or(&self.key.novel_id);
            let progress = Progress {
                chapter_id: self.chapter_id.clone(),
                path,
                offset,
                updated_at: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
            };
            let store = self.progress_store.as_ref().unwrap_or(&cx.app.repo.store);
            let save_started = std::time::Instant::now();
            let user = if cx.app.repo.api.signed_in() { cx.app.repo.api.username() } else { String::new() };
            if let Err(e) = cx.app.progress_writer.save(store.clone(), self.key.clone(), title.into(), progress, user) {
                self.notice = format!("进度保存失败: {e}");
            } else {
                self.saved_anchor = Some(anchor);
            }
            if std::env::var_os("KN_DEBUG").is_some() {
                eprintln!("[reader-save] staged progress + history {:.1} ms", save_started.elapsed().as_secs_f64()*1000.0);
            }
        }
    }
    fn adjacent(&self, forward: bool) -> Option<String> {
        self.chapter
            .as_ref()
            .and_then(|c| {
                if forward {
                    c.next_id.clone()
                } else {
                    c.prev_id.clone()
                }
            })
            .or_else(|| {
                self.novel
                    .as_ref()
                    .and_then(|n| n.adjacent(&self.chapter_id, forward))
            })
    }
    fn change_chapter(&mut self, cx: &mut Cx<Kindlia>, id: String) {
        if id == self.chapter_id && self.chapter.is_some() {
            return;
        }
        self.stop_tracking(cx);
        self.save(cx);
        self.sync_history(cx);
        let signature = page_signature(cx);
        if self.target.is_none() && self.pages.len() <= 2000 {
            if let (Some(chapter), Some(layout)) = (self.chapter.take(), self.layout.take()) {
                self.chapter_cache.retain(|c| c.id != self.chapter_id);
                self.chapter_cache.push(CachedChapter {
                    id: self.chapter_id.clone(),
                    signature: signature.clone(),
                    chapter,
                    document: self.document.take(),
                    blocks: std::mem::take(&mut self.blocks),
                    pages: std::mem::take(&mut self.pages),
                    layout,
                    index: self.index,
                    paginator: self.paginator.take(),
                    done: self.done,
                });
                while self.chapter_cache.len() > 3
                    || self
                        .chapter_cache
                        .iter()
                        .map(|c| c.pages.len())
                        .sum::<usize>()
                        > 2000
                {
                    self.chapter_cache.remove(0);
                }
            }
        }
        self.chapter_id = id;
        self.target = None;
        self.menu = false;
        if let Some(i) = self
            .chapter_cache
            .iter()
            .position(|c| c.id == self.chapter_id && c.signature == signature)
        {
            let cache = self.chapter_cache.remove(i);
            self.generation += 1;
            self.chapter = Some(cache.chapter);
            self.document = cache.document;
            self.blocks = cache.blocks;
            self.rich = self.document.is_some() || self.blocks.iter().any(|b| !b.spans.is_empty());
            self.pages = cache.pages;
            self.layout = Some(cache.layout);
            self.index = cache.index;
            self.done = cache.done;
            self.paginator = cache.paginator;
            self.loading = false;
            self.images.clear();
            self.page_bitmaps.clear();
            self.notice.clear();
            self.prefetch(cx);
            cx.request_turn(None);
        } else {
            self.load(cx, false);
        }
    }
    fn turn(&mut self, cx: &mut Cx<Kindlia>, forward: bool) {
        if self.target.is_some() {
            return;
        }
        let previous = self.index;
        if forward && self.index + 1 < self.pages.len() {
            self.index += 1;
        } else if !forward && self.index > 0 {
            self.index -= 1;
        } else if self.done {
            if let Some(id) = self.adjacent(forward) {
                self.change_chapter(cx, id);
                return;
            }
        }
        if self.index != previous {
            let (id, title) = self.stats_book(cx);
            self.tracker.page(&cx.app.repo.store, &id, &title, false);
        }
        kn_platform::display::set_swipe_steps(cx.app.config.swipe_steps.clamp(6, 60));
        cx.request_turn(cx.app.config.page_turn_animation.then_some(if forward {
            kn_platform::SwipeDir::Left
        } else {
            kn_platform::SwipeDir::Right
        }));
        cx.after(Duration::from_secs(3), 1);
    }
    // Cache only content: clock, bookmark ribbon and menu stay live.
    fn paint_content(&self, cx: &mut Cx<Kindlia>, frame: &mut Bitmap, index: usize) {
        let m = cx.app.metrics;
        let w = cx.width;
        let content_top = m.touch / 2;
        if let Some(items) = self.pages.get(index) {
            for item in items {
                match item {
                    PageItem::Text {
                        text,
                        x,
                        y,
                        rich: Some(line),
                        ..
                    } => self.draw_rich_line(
                        cx,
                        frame,
                        text,
                        *x as f32,
                        *y as f32 + content_top as f32,
                        line,
                    ),
                    PageItem::Note { .. } => {}
                    PageItem::Rule {
                        x,
                        y,
                        width,
                        height,
                    } => frame.fill_rect(
                        Rect::new(
                            *x as i32,
                            *y as i32 + content_top as i32,
                            *width as u32,
                            *height as u32,
                        ),
                        cx.theme.mid,
                    ),
                    PageItem::Dot { x, y, size } => frame.rounded_rect(
                        Rect::new(
                            *x as i32,
                            *y as i32 + content_top as i32,
                            *size as u32,
                            *size as u32,
                        ),
                        *size as u32 / 2,
                        Some(cx.theme.foreground),
                        None,
                        0,
                    ),
                    PageItem::Text {
                        text, x, y, size, ..
                    } => {
                        let style = TextStyle {
                            fonts: &self.chain,
                            size_px: *size as f32,
                        };
                        let ascent = cx.fonts.line_metrics(&style).ascent;
                        cx.glyphs.draw_text(
                            cx.fonts,
                            frame,
                            *x as f32,
                            *y as f32 + content_top as f32 + ascent,
                            text,
                            &style,
                            cx.theme.foreground,
                        );
                    }
                    PageItem::Image {
                        url,
                        x,
                        y,
                        width,
                        height,
                        ..
                    } => {
                        let rect = Rect::new(
                            *x as i32,
                            *y as i32 + content_top as i32,
                            *width as u32,
                            *height as u32,
                        );
                        let key = format!("{url}|{width}x{height}");
                        if let Some(ImageSlot::Ready(image)) = self.images.get(&key) {
                            let actual = image_rect(image, rect);
                            frame.blit(
                                image,
                                image.bounds(),
                                actual.x,
                                actual.y,
                            );
                        } else {
                            label(
                                cx,
                                frame,
                                rect,
                                if matches!(self.images.get(&key), Some(ImageSlot::Failed)) {
                                    "插图无法显示（菜单可重取）"
                                } else {
                                    "插图加载中…"
                                },
                                true,
                            );
                        }
                    }
                }
            }
        } else {
            label(
                cx,
                frame,
                Rect::new(0, (cx.height / 3) as i32, w, m.touch),
                &self.notice,
                true,
            );
        }
    }
    fn cache_page(&mut self, cx: &mut Cx<Kindlia>, index: usize) {
        if index >= self.pages.len() || self.page_bitmaps.iter().any(|(i, _)| *i == index) {
            return;
        }
        let mut bitmap = Bitmap::new(cx.width, cx.height, cx.theme.background);
        self.paint_content(cx, &mut bitmap, index);
        self.page_bitmaps.push((index, bitmap));
        let current = self.index;
        self.page_bitmaps.retain(|(i, _)| i.abs_diff(current) <= 1);
    }
    fn images(&mut self, cx: &mut Cx<Kindlia>) {
        let mut todo = Vec::new();
        for page in self.pages.iter().skip(self.index.saturating_sub(1)).take(3) {
            for item in page {
                if let PageItem::Image {
                    url, width, height, ..
                } = item
                {
                    let key = format!("{url}|{width}x{height}");
                    if !self.images.contains_key(&key) {
                        self.images.insert(key.clone(), ImageSlot::Loading);
                        todo.push((key, url.clone(), (*width as u32, *height as u32)));
                    }
                }
            }
        }
        for (key, url, size) in todo {
            let repo = cx.app.repo.clone();
            let local = url.starts_with("epub://");
            let work = move || ImageResult(key, repo.image(&url, size));
            if local {
                cx.spawn_local_droppable(work);
            } else {
                cx.spawn_droppable(work);
            }
        }
        // 仅保留当前和相邻页图像，磁盘原图缓存不受影响。
        let keep: std::collections::HashSet<String> = self
            .pages
            .iter()
            .skip(self.index.saturating_sub(1))
            .take(4)
            .flatten()
            .filter_map(|item| {
                if let PageItem::Image {
                    url, width, height, ..
                } = item
                {
                    Some(format!("{url}|{width}x{height}"))
                } else {
                    None
                }
            })
            .collect();
        self.images.retain(|key, _| keep.contains(key));
    }
}
impl Page<Kindlia> for ReaderPage {
    fn id(&self) -> PageId {
        PageId(30)
    }
    fn orientation(&self, app: &Kindlia) -> Option<Orientation> {
        Some(if app.config.landscape {
            if app.config.landscape_side == "right" {
                Orientation::LandscapeRight
            } else {
                Orientation::LandscapeLeft
            }
        } else {
            Orientation::Portrait
        })
    }
    fn flash_on_enter(&self) -> bool {
        false
    }
    fn enter(&mut self, cx: &mut Cx<Kindlia>, returning: bool) {
        crate::sleep::prepare(cx, self.key.clone());
        if self.progress_store.is_none() {
            self.progress_store = Some(cx.app.repo.store.clone());
        }
        if let Some(id) = cx.app.reader_jump.take() {
            self.change_chapter(cx, id);
            self.target = cx.app.reader_anchor.take();
            self.target_seen = false;
            if self.done {
                if let Some((path, offset)) = self.target.take() {
                    self.index = kn_text::page_for_path(&self.pages, &path, Some(offset as i64), 0);
                }
            }
        } else if self.chapter.is_none() && !self.loading {
            self.target = cx
                .app
                .repo
                .store
                .progress(&self.key)
                .filter(|p| p.chapter_id == self.chapter_id)
                .map(|p| (p.path, p.offset));
            self.load(cx, false);
        } else if returning && self.text_signature != text_signature(&cx.app.config) {
            self.rebuild(cx);
        }
        self.bookmarks = cx.app.repo.store.bookmarks(&self.key);
        self.prefetch(cx);
        let signature = format!(
            "{}:{}:{}",
            cx.app.config.translation, cx.app.config.show_original, cx.app.config.show_translation
        );
        if self.key.provider_id == "epub"
            && returning
            && !self.config_signature.is_empty()
            && self.config_signature != signature
        {
            if let Some(source) = cx
                .app
                .repo
                .store
                .read_cache::<serde_json::Value>("epub-source", &self.key)
            {
                if !source["human"].as_bool().unwrap_or(false) {
                    let repo = cx.app.repo.clone();
                    let config = cx.app.config.clone();
                    let novel = source["novel"].as_str().unwrap_or("").to_owned();
                    let volume = source["volume"].as_str().unwrap_or("").to_owned();
                    self.generation += 1;
                    let generation = self.generation;
                    self.loading = true;
                    self.notice = "正在切换文库译文并重新下载…".into();
                    cx.spawn(move || {
                        VolumeResult(
                            generation,
                            super::cloud::open_volume(&repo, &config, &novel, &volume, false),
                        )
                    });
                }
            }
        }
        self.config_signature = signature;
        if self.novel.is_none() {
            let repo = cx.app.repo.clone();
            let key = self.key.clone();
            crate::repository::read_on_workers(cx, repo, false, move |r| r.novel(&key, false), NovelResult);
        }
    }
    fn leave(&mut self, cx: &mut Cx<Kindlia>) {
        self.stop_tracking(cx);
        self.save(cx);
        self.sync_history(cx);
    }
    fn on_suspend(&mut self, cx: &mut Cx<Kindlia>) {
        self.stop_tracking(cx);
        self.save(cx);
        self.sync_history(cx);
    }
    fn on_timer(&mut self, cx: &mut Cx<Kindlia>, _token: u32) {
        self.save(cx);
        self.sync_history(cx);
        cx.after(Duration::from_secs(30), 1);
    }
    fn render(&mut self, cx: &mut Cx<Kindlia>, frame: &mut Bitmap) {
        self.ensure_layout(cx);
        if self.font_loading.is_some() {
            self.draw_header(cx, frame, (cx.app.metrics.touch / 2) as i32);
            if self.menu { self.draw_chrome(cx, frame); }
            label(cx, frame, Rect::new(0, (cx.height / 3) as i32, cx.width, cx.app.metrics.touch), "正在加载字体…", true);
            return;
        }
        let m = cx.app.metrics;
        let w = cx.width;
        let content_top = m.touch / 2;
        self.cache_page(cx, self.index);
        if let Some((_, bitmap)) = self.page_bitmaps.iter().find(|(i, _)| *i == self.index) {
            frame.blit(bitmap, bitmap.bounds(), 0, 0);
        } else {
            self.paint_content(cx, frame, self.index);
        }
        if let Some(items) = self.pages.get(self.index) {
            for (position,item) in items.iter().enumerate() {
                if let PageItem::Image{url,x,y,width,height,..}=item {
                    let area = Rect::new(*x as i32, *y as i32 + content_top as i32, *width as u32, *height as u32);
                    if let Some(ImageSlot::Ready(image)) = self.images.get(&format!("{url}|{width}x{height}")) {
                        let visible = Rect::new(0, content_top as i32, cx.width, cx.height.saturating_sub(content_top));
                        if let Some(actual) = image_rect(image, area).intersect(&visible) {
                            cx.hits.add(HitId(800_000 + position as u32), actual).no_feedback();
                        }
                    }
                }
                if let PageItem::Note {
                    index,
                    x,
                    y,
                    width,
                    height,
                } = item
                {
                    cx.hits
                        .add(
                            HitId(2000 + *index as u32),
                            Rect::new(
                                *x as i32,
                                *y as i32 + content_top as i32,
                                *width as u32,
                                *height as u32,
                            ),
                        )
                        .no_feedback();
                }
            }
        }
        self.start_tracking();
        self.draw_header(cx, frame, content_top as i32);
        if self.menu || (self.chapter.is_none() && !self.loading) {
            self.draw_chrome(cx, frame);
            if self.chapter.is_none() {
                button(
                    cx,
                    frame,
                    Rect::new((w / 3) as i32, (cx.height / 2) as i32, w / 3, m.touch),
                    "重试",
                    20,
                    !self.loading,
                );
            }
        }
    }
    fn on_idle(&mut self, cx: &mut Cx<Kindlia>) -> bool {
        if self.font_loading.is_some() { return false; }
        self.ensure_layout(cx);
        if self.font_loading.is_some() { return false; }
        let first = self.pages.is_empty();
        if let Some(paginator) = &mut self.paginator {
            let page_started = std::time::Instant::now();
            let mut measure = FontMeasure::new(cx.fonts, &self.chain, &mut self.widths);
            let start = self.pages.len();
            let mut resolved = false;
            match paginator.next_page(&mut measure) {
                Some(page) => self.pages.push(page),
                None => self.done = true,
            }
            if first && !self.pages.is_empty() && std::env::var_os("KN_DEBUG").is_some() {
                eprintln!("[reader-load] first-page elapsed {:.1} ms, pagination {:.1} ms",
                    self.load_started.map(|t| t.elapsed().as_secs_f64()*1000.0).unwrap_or(0.0),
                    page_started.elapsed().as_secs_f64()*1000.0);
            }
            if let Some((path, offset)) = &self.target {
                let mut past = false;
                for item in self.pages[start..].iter().flatten() {
                    if item.path() == path {
                        self.target_seen = true;
                    } else if self.target_seen && !item.path().is_empty() {
                        past = true;
                    }
                }
                let found = self.target_seen;
                if self.done || (found && past) {
                    self.index = kn_text::page_for_path(&self.pages, path, Some(*offset as i64), 0);
                    self.target = None;
                    resolved = true;
                    cx.after(Duration::from_secs(3), 1);
                }
            }
            if self.done {
                self.paginator = None;
            }
            if first || resolved || self.done {
                cx.request_redraw(if first {
                    RefreshHint::Flash
                } else {
                    RefreshHint::Ui
                });
            }
            if first && self.target.is_none() {
                cx.after(Duration::from_secs(3), 1);
            }
        }
        self.images(cx);
        // Warm at most one neighbor per idle turn, as in KinNovel.
        for index in [
            Some(self.index),
            self.index.checked_add(1),
            self.index.checked_sub(1),
        ]
        .into_iter()
        .flatten()
        {
            if index < self.pages.len() && !self.page_bitmaps.iter().any(|(i, _)| *i == index) {
                self.cache_page(cx, index);
                return true;
            }
        }
        self.paginator.is_some()
    }
    fn on_input(&mut self, cx: &mut Cx<Kindlia>, event: &InputEvent) -> Transition<Kindlia> {
        if let InputEvent::Key { code, pressed: true } = event {
            let forward = match code {
                KeyCode::PageForward => Some(true),
                KeyCode::PageBack => Some(false),
                _ => None,
            };
            if let Some(forward) = forward {
                self.turn(cx, forward);
                return Transition::None;
            }
        }
        if self.menu || (self.chapter.is_none() && !self.loading) {
            if let InputEvent::Gesture(g) = event {
                if g.kind == GestureKind::Long
                    && cx.hits.at(kn_render::Point {
                        x: g.end.0,
                        y: g.end.1,
                    }) == Some(HitId(26))
                {
                    return Transition::Push(Box::new(super::bookmarks::BookmarksPage::new(
                        self.key.clone(),
                    )));
                }
                if g.kind == GestureKind::Tap {
                    if let Some(id) = cx.hits.at(kn_render::Point {
                        x: g.end.0,
                        y: g.end.1,
                    }) {
                        if super::light::on_tap(
                            cx,
                            id,
                            kn_render::Point {
                                x: g.end.0,
                                y: g.end.1,
                            },
                        ) {
                            return Transition::None;
                        }
                    }
                } else if matches!(
                    g.kind,
                    GestureKind::SwipeLeft
                        | GestureKind::SwipeRight
                        | GestureKind::SwipeUp
                        | GestureKind::SwipeDown
                ) {
                    if super::light::on_swipe(cx, g) {
                        return Transition::None;
                    }
                }
                if cx.hits.at(kn_render::Point {
                    x: g.start.0,
                    y: g.start.1,
                }) == Some(HitId(27))
                    && g.kind != GestureKind::Down
                {
                    self.target = None;
                    let ratio = (g.end.0 - self.track.x).clamp(0, self.track.w as i32) as f32
                        / self.track.w.max(1) as f32;
                    self.index =
                        ((self.pages.len().saturating_sub(1)) as f32 * ratio).round() as usize;
                    cx.after(Duration::from_secs(3), 1);
                    cx.request_redraw(RefreshHint::Ui);
                    return Transition::None;
                }
            }
            match hit(cx, event) {
                Some(1) => return Transition::Back,
                Some(10) => {
                    if let Some(id) = self.adjacent(false) {
                        self.change_chapter(cx, id);
                    }
                }
                Some(11) => {
                    self.save(cx);
                    return Transition::Push(Box::new(book::BookPage::catalog(
                        self.key.clone(),
                        self.novel.clone(),
                    )));
                }
                Some(12) => {
                    if let Some(id) = self.adjacent(true) {
                        self.change_chapter(cx, id);
                    }
                }
                Some(13) => {
                    self.save(cx);
                    cx.app.config.font_size = cx.app.config.font_size.saturating_sub(2).max(24);
                    self.target = self.anchor();
                    self.layout = None;
                }
                Some(26) => self.bookmark(cx),
                Some(28) => return Transition::None,
                Some(15) => {
                    self.save(cx);
                    cx.app.config.font_size = (cx.app.config.font_size + 2).min(96);
                    self.target = self.anchor();
                    self.layout = None;
                }
                Some(16) => {
                    cx.app.config.landscape = !cx.app.config.landscape;
                }
                Some(17) => return Transition::Push(Box::new(settings::SettingsPage::new(cx.app))),
                Some(18) => return Transition::Home,
                Some(19) => {
                    cx.app.config.night = !cx.app.config.night;
                    cx.request_redraw(RefreshHint::Flash);
                }
                Some(20) => {
                    self.save(cx);
                    self.target = self.anchor();
                    self.load(cx, true);
                }
                Some(21) => self.menu = false,
                Some(22) | Some(23) => {
                    if let Some(level) = cx.app.light.get(LightProp::Brightness) {
                        let delta = if hit(cx, event) == Some(22) { -1 } else { 1 };
                        cx.app.light.set(LightProp::Brightness, level.value + delta);
                    }
                }
                Some(24) | Some(25) => {
                    if let Some(level) = cx.app.light.get(LightProp::Warmth) {
                        let delta = if hit(cx, event) == Some(24) { -1 } else { 1 };
                        cx.app.light.set(LightProp::Warmth, level.value + delta);
                    }
                }
                _ => {
                    if matches!(event,InputEvent::Gesture(g)if g.kind==GestureKind::Tap) {
                        self.menu = false;
                        for r in self.chrome_rects.drain(..) {
                            cx.request_clean(r);
                        }
                        cx.request_redraw(RefreshHint::Ui);
                    }
                    return Transition::None;
                }
            }
            if let Err(e) = cx.app.save_config() {
                self.notice = e;
            }
            cx.request_redraw(RefreshHint::Ui);
            return Transition::None;
        }
        if let Some(id)=hit(cx,event).filter(|id|*id>=800_000) {
            // A tap must both begin and end on this rendered image; crossing
            // the letterbox boundary while releasing must not open a preview.
            if !matches!(event, InputEvent::Gesture(g) if cx.hits.at(kn_render::Point { x: g.start.0, y: g.start.1 }) == Some(HitId(id))) {
                return Transition::None;
            }
            if let Some(PageItem::Image{url,height,..})=self.pages.get(self.index).and_then(|p|p.get((id-800_000) as usize)) {
                return Transition::Push(Box::new(super::image::ImagePage::new(url.clone(),*height as u32)));
            }
        }
        if let Some(id) = hit(cx, event).filter(|id| *id >= 2000) {
            if let Some(note) = self
                .document
                .as_ref()
                .and_then(|d| id.checked_sub(2001).and_then(|n| d.notes.get(n as usize)))
            {
                return Transition::Push(Box::new(super::text::TextPage::new("注释", note)));
            }
        }
        if let InputEvent::Gesture(g) = event {
            match g.kind {
                GestureKind::SwipeLeft | GestureKind::SwipeUp => self.turn(cx, true),
                GestureKind::SwipeRight => self.turn(cx, false),
                GestureKind::SwipeDown => {
                    self.menu = true;
                    cx.app.light.refresh();
                    cx.request_redraw(RefreshHint::Ui);
                }
                GestureKind::Tap => {
                    if g.end.1 < (cx.app.metrics.touch / 2) as i32 {
                        return Transition::None;
                    }
                    if g.end.0 < (cx.width * 3 / 10) as i32 {
                        self.turn(cx, false);
                    } else if g.end.0 > (cx.width * 7 / 10) as i32 {
                        self.turn(cx, true);
                    } else {
                        self.menu = true;
                        cx.app.light.refresh();
                        cx.request_redraw(RefreshHint::Ui);
                    }
                }
                _ => {}
            }
        }
        Transition::None
    }
    fn on_message(&mut self, cx: &mut Cx<Kindlia>, msg: Box<dyn Any + Send>) {
        let msg = match msg.downcast::<FontResult>() {
            Ok(r) => {
                if self.font_loading.as_ref() != Some(&r.0) { return; }
                self.font_loading = None;
                if cx.app.config.reader_font != r.0 {
                    cx.request_redraw(RefreshHint::Ui);
                    return;
                }
                let at = std::time::Instant::now();
                let (chain, notice) = crate::fonts::install(cx.fonts, cx.glyphs, &r.0, r.1, &cx.app.ui_fonts, &mut cx.app.reader_font_cache);
                self.chain = chain;
                self.font_name = r.0;
                self.layout = None;
                if let Some(notice) = notice { self.notice = notice; }
                else if self.notice == "正在加载字体…" { self.notice.clear(); }
                if std::env::var_os("KN_DEBUG").is_some() {
                    eprintln!("[reader-font] worker io {:.1} ms, UI install {:.1} ms", r.2, at.elapsed().as_secs_f64()*1000.0);
                }
                cx.request_redraw(RefreshHint::Ui);
                return;
            }
            Err(msg) => msg,
        };
        let msg = match msg.downcast::<VolumeResult>() {
            Ok(r) => {
                if r.0 != self.generation {
                    return;
                }
                self.loading = false;
                match r.1 {
                    Ok(key) => {
                        self.stop_tracking(cx);
                        self.save(cx);
                        let anchor = self.anchor();
                        self.chapter_cache.clear();
                        self.prefetched.clear();
                        self.key = key;
                        self.stats_key = None;
                        self.bookmarks = cx.app.repo.store.bookmarks(&self.key);
                        self.novel = None;
                        self.document = None;
                        let key = self.key.clone();
                        let repo = cx.app.repo.clone();
                        let count = crate::epub::index(&repo.store, &key)
                            .map(|b| b.chapters.len())
                            .unwrap_or(1);
                        let index = self
                            .chapter_id
                            .parse::<usize>()
                            .unwrap_or(0)
                            .min(count.saturating_sub(1));
                        self.chapter_id = index.to_string();
                        crate::repository::read_on_workers(cx, repo, false, move |r| r.novel(&key, false), NovelResult);
                        self.load(cx, false);
                        self.target = anchor;
                    }
                    Err(e) => self.notice = e,
                }
                cx.request_redraw(RefreshHint::Ui);
                return;
            }
            Err(m) => m,
        };
        let msg = match msg.downcast::<ChapterResult>() {
            Ok(result) => {
                if result.0 != self.generation {
                    return;
                }
                self.loading = false;
                match result.1 {
                    Ok((loaded, document)) => {
                        if self.target.is_none() && self.key.provider_id=="epub" {
                            if let Some((_,fragment))=self.chapter_id.split_once('#') {
                                self.target=Some((format!("/epub-anchor/{}",crate::epub::unescape(fragment)),0));
                                self.target_seen=false;
                            }
                        }
                        self.document = document;
                        self.chapter = Some(loaded.value);
                        self.notice = loaded.notice.unwrap_or_default();
                        let rebuild_started = std::time::Instant::now();
                        self.rebuild(cx);
                        if std::env::var_os("KN_DEBUG").is_some() {
                            eprintln!("[reader-load] prepared elapsed {:.1} ms, UI rebuild {:.1} ms, blocks {}",
                                self.load_started.map(|t| t.elapsed().as_secs_f64()*1000.0).unwrap_or(0.0),
                                rebuild_started.elapsed().as_secs_f64()*1000.0, self.blocks.len());
                        }
                        self.prefetch(cx);
                    }
                    Err(e) => {
                        self.notice = e;
                        self.menu = true;
                    }
                }
                cx.request_redraw(RefreshHint::Ui);
                return;
            }
            Err(msg) => msg,
        };
        let msg = match msg.downcast::<NovelResult>() {
            Ok(result) => {
                if let Ok(loaded) = result.0 {
                    self.novel = Some(loaded.value);
                    self.prefetch(cx);
                }
                cx.request_redraw(RefreshHint::Ui);
                return;
            }
            Err(msg) => msg,
        };
        if let Ok(result) = msg.downcast::<ImageResult>() {
            // 尺寸和 URL 都匹配才接受，换章/重新排版后的旧任务不可覆盖当前页面。
            if self.images.contains_key(&result.0) {
                let affected = |index: usize| {
                    self.pages.get(index).is_some_and(|p| p.iter().any(|item| {
                    matches!(item, PageItem::Image { url, width, height, .. } if format!("{url}|{width}x{height}") == result.0)
                }))
                };
                let visible = affected(self.index);
                self.page_bitmaps.retain(|(i, _)| !affected(*i));
                self.images.insert(
                    result.0,
                    match result.1 {
                        Ok(bitmap) => ImageSlot::Ready(bitmap),
                        Err(e) => {
                            eprintln!("[image] {e}");
                            ImageSlot::Failed
                        }
                    },
                );
                if visible {
                    cx.request_redraw(RefreshHint::Ui);
                }
            }
        }
    }
}

// Use exactly the same centering and bitmap dimensions for painting and hits.
fn image_rect(image: &Bitmap, area: Rect) -> Rect {
    Rect::new(
        area.x + (area.w.saturating_sub(image.width()) / 2) as i32,
        area.y + (area.h.saturating_sub(image.height()) / 2) as i32,
        image.width(), image.height(),
    )
}

fn ribbon(frame: &mut Bitmap, r: Rect, filled: bool, line: f32, color: u8) {
    if r.w < 4 || r.h < 4 {
        return;
    }
    let notch = (r.w as f32 * 0.45).min(r.h as f32 * 0.4);
    let body = r.h as f32 - notch;
    let (x0, y0, w, h) = (r.x as f32, r.y as f32, r.w as f32, r.h as f32);
    if filled {
        frame.fill_rect(Rect::new(r.x, r.y, r.w, body as u32), color);
        // 缺口两侧的三角: 越往下, 中间的缺口越宽
        for dy in 0..notch.ceil() as i32 {
            let t = (dy as f32 + 0.5) / notch;
            let half = (w / 2.0 * t).round() as u32;
            let side = (r.w / 2).saturating_sub(half);
            let y = r.y + body as i32 + dy;
            frame.fill_rect(Rect::new(r.x, y, side, 1), color);
            frame.fill_rect(Rect::new(r.x + r.w as i32 - side as i32, y, side, 1), color);
        }
    } else {
        let mid = x0 + w / 2.0;
        let (l, rr) = (x0 + line / 2.0, x0 + w - line / 2.0);
        frame.line(l, y0 + line / 2.0, rr, y0 + line / 2.0, line, color);
        frame.line(l, y0, l, y0 + h, line, color);
        frame.line(rr, y0, rr, y0 + h, line, color);
        frame.line(l, y0 + h, mid, y0 + body, line, color);
        frame.line(mid, y0 + body, rr, y0 + h, line, color);
    }
}
