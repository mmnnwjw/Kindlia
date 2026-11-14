use super::*;

impl LibraryPage {
    pub(super) fn render_view(&mut self, cx: &mut Cx<Kindlia>, frame: &mut Bitmap) {
        if let Some((_, picker)) = &mut self.picker {
            picker.render(cx, frame);
            return;
        }
        if self.discover {
            search_header(
                cx,
                frame,
                if self.searching {
                    "搜索Web小说"
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
                if self.local {
                    "本地书籍"
                } else if self.history {
                    "历史"
                } else {
                    "我的收藏"
                },
                self.local,
            );
        }
        let m = cx.app.metrics;
        let width = cx.width;
        let mut y = m.header_h();
        if !self.discover && !self.history && self.shelf.iter().any(|b| b.progress.is_some()) {
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
            let current = self.shelf.iter().find_map(|b| b.progress.as_ref());
            let age = current
                .map(|p| crate::formatting::relative_time(p.updated_at))
                .unwrap_or_default();
            ink.text(
                frame,
                x,
                cover.y,
                &format!("继续阅读 · {age}"),
                m.small,
                cx.theme.muted,
            );
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
            let subtitle = current
                .map(|p| {
                    let title = if p.chapter_title.is_empty() {
                        "已保存章节"
                    } else {
                        &p.chapter_title
                    };
                    if p.pages > 0 {
                        format!("{title} · {} / {}", p.page, p.pages)
                    } else {
                        title.to_owned()
                    }
                })
                .unwrap_or_default();
            if let Some(p) = current.filter(|p| p.pages > 0) {
                let width = (rect.right() - pad - x).max(1) as u32;
                let bar = Rect::new(x, rect.bottom() - pad - 6, width, 5);
                frame.rounded_rect(bar, 2, Some(cx.theme.muted), None, 0);
                frame.rounded_rect(
                    Rect::new(
                        bar.x,
                        bar.y,
                        (bar.w as f64 * (p.page as f64 / p.pages as f64).clamp(0., 1.)) as u32,
                        bar.h,
                    ),
                    2,
                    Some(cx.theme.foreground),
                    None,
                    0,
                );
            }
            let subtitle = ink.fit(&subtitle, m.small, (rect.right() - pad - x).max(1) as f32);
            ink.text(
                frame,
                x,
                cover.y + (m.small * 3.5) as i32,
                &subtitle,
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
                &["Web小说", "文库小说", "排行"],
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
                        m.touch + m.touch / 4,
                    ),
                    &name,
                    &options[selected],
                    71 + field as u32,
                );
            }
            y += m.touch + m.touch / 4;
        }
        let cell = width / 3;
        if self.local {
            button(
                cx,
                frame,
                Rect::new(m.margin as i32, y as i32, width - 2 * m.margin, m.touch),
                "导入本地小说",
                30,
                true,
            );
            y += m.touch;
        } else if !self.discover && !self.history {
            selected_button(
                cx,
                frame,
                Rect::new(0, y as i32, cell, m.touch),
                "Web收藏",
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
        let folder_h = if self.shelf_source > 0 && !self.history {
            m.row_h()
        } else {
            m.row_h_cover()
        };
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
        if !self.discover {
            self.rows = (cx.height.saturating_sub(
                top + m.pager_h()
                    + if self.local || self.return_on_close {
                        0
                    } else {
                        m.tab_bar_h()
                    },
            ) / folder_h)
                .max(1) as usize;
            self.pages = if self.shelf_source > 0 && !self.history {
                self.folders.len()
            } else {
                self.shelf.len()
            }
            .div_ceil(self.rows)
            .max(1);
            self.page = self.page.min(self.pages - 1);
        }
        if !self.editing {
            let empty = if self.discover {
                self.books.is_empty()
            } else if self.shelf_source > 0 && !self.history {
                self.folders.is_empty()
            } else {
                self.shelf.is_empty()
            };
            if empty {
                state_message(
                    cx,
                    frame,
                    Rect::new(
                        0,
                        top as i32,
                        width,
                        cx.height.saturating_sub(
                            top + m.pager_h()
                                + if self.local || self.return_on_close {
                                    0
                                } else {
                                    m.tab_bar_h()
                                },
                        ),
                    ),
                    if self.loading {
                        "加载中…"
                    } else if self.notice.is_empty() {
                        "暂无书籍"
                    } else {
                        &self.notice
                    },
                    if self.loading {
                        None
                    } else if self.local {
                        Some(("导入本地小说", 30))
                    } else {
                        Some(("重试", 98))
                    },
                );
            }
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
                    let subtitle = (!book.authors.is_empty())
                        .then(|| book.authors.join(" / "))
                        .or_else(|| info.map(|(m, _)| m.author.clone()))
                        .filter(|s| !s.is_empty())
                        .unwrap_or_else(|| format!("{} · {} 章", book.provider_id, book.total));
                    let mut ink = Ink {
                        fonts: cx.fonts,
                        glyphs: cx.glyphs,
                        chain: &cx.app.ui_fonts,
                    };
                    widgets::list_row_cover_badged(
                        &mut ink,
                        frame,
                        &cx.theme,
                        &m,
                        rect,
                        info.map(|(_, i)| i),
                        &crate::formatting::web_badge(book.novel_type.as_deref(), &book.attentions),
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
                    folder_row(
                        cx,
                        frame,
                        Rect::new(0, (top + folder_h * i as u32) as i32, width, folder_h),
                        folder["title"].as_str().unwrap_or("收藏夹"),
                        "长按管理",
                        200 + i as u32,
                    );
                    cx.hits
                        .add(
                            HitId(200 + i as u32),
                            Rect::new(0, (top + folder_h * i as u32) as i32, width, folder_h),
                        )
                        .long_press();
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
            pager(
                cx,
                frame,
                self.page,
                self.pages,
                if self.local || self.return_on_close {
                    0
                } else {
                    m.tab_bar_h()
                },
            );
            if !self.local && !self.return_on_close {
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
            }
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
}
