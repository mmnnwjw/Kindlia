use super::*;

impl CloudPage {
    pub(super) fn render_view(&mut self, cx: &mut Cx<Kindlia>, frame: &mut Bitmap) {
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
                        crate::images::spawn_fit(cx, repo, url, (size.w, size.h), move |r| {
                            let failed = r.is_err();
                            CoverResult(
                                tag,
                                r.or_else(|_| crate::covers::fallback((size.w, size.h))),
                                failed,
                            )
                        });
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
                            (
                                if v["favored"].as_str().is_some() {
                                    "移出收藏"
                                } else {
                                    "加入书架"
                                },
                                10,
                                !self.loading,
                            ),
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
                self.searching || !self.root,
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
                !self.root,
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
        if self.items.is_empty() {
            state_message(
                cx,
                frame,
                Rect::new(
                    0,
                    list_top as i32,
                    w,
                    cx.height.saturating_sub(
                        list_top
                            + m.pager_h()
                            + if self.tab().is_some() {
                                m.tab_bar_h()
                            } else {
                                0
                            },
                    ),
                ),
                if self.notice.is_empty() {
                    "暂无内容"
                } else {
                    &self.notice
                },
                if self.loading {
                    None
                } else {
                    Some(("重试", 98))
                },
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
                        crate::images::spawn_fit(cx, repo, url, (size.w, size.h), move |r| {
                            let failed = r.is_err();
                            CoverResult(
                                tag,
                                r.or_else(|_| crate::covers::fallback((size.w, size.h))),
                                failed,
                            )
                        });
                    }
                    let mut ink = kn_ui::widgets::Ink {
                        fonts: cx.fonts,
                        glyphs: cx.glyphs,
                        chain: &cx.app.ui_fonts,
                    };
                    kn_ui::widgets::list_row_cover_badged(
                        &mut ink,
                        frame,
                        &cx.theme,
                        &m,
                        rect,
                        self.covers.get(&cover_key).and_then(|b| b.as_ref()),
                        &item.badge,
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
}
