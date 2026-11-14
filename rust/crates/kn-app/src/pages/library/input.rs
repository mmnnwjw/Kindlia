use super::*;

impl LibraryPage {
    pub(super) fn handle_input(
        &mut self,
        cx: &mut Cx<Kindlia>,
        event: &InputEvent,
    ) -> Transition<Kindlia> {
        if let Some((tip, _)) = self.active_tip() {
            if super::tips::input(cx, event, tip) {
                return Transition::None;
            }
        }
        // 按下反馈由运行时负责；等完整手势再修改页面，避免按下/抬起各整页刷新一次。
        if matches!(event, InputEvent::Gesture(g) if g.kind == kn_platform::GestureKind::Down) {
            return Transition::None;
        }
        if let Some((field, picker)) = &mut self.picker {
            match picker.input(cx, event) {
                choice::Event::Selected(n) => {
                    let field = *field;
                    self.picker = None;
                    if field == usize::MAX {
                        if n == 1 {
                            let store = cx.app.repo.store.clone();
                            self.loading = true;
                            cx.spawn_local(move || ClearResult(store.clear_history()));
                        }
                        cx.request_redraw(RefreshHint::Ui);
                        return Transition::None;
                    }
                    if field == usize::MAX - 1 {
                        if n == 0 {
                            if let Some((_, title)) = &self.managing_folder {
                                self.renaming_folder = true;
                                self.editing = true;
                                self.editor.set_max_chars(256);
                                self.editor.set_text(title);
                            }
                        } else if n == 1 {
                            self.picker = Some((
                                usize::MAX - 2,
                                choice::Choice::new("删除收藏夹？", ["取消", "删除"], Some(0)),
                            ));
                        } else {
                            self.managing_folder = None;
                        }
                        cx.request_redraw(RefreshHint::Clean);
                        return Transition::None;
                    }
                    if field == usize::MAX - 2 {
                        let folder = self.managing_folder.take();
                        if n == 1 {
                            if let Some((path, _)) = folder {
                                self.generation += 1;
                                self.loading = false;
                                if cx.app.repo.offline {
                                    self.notice = "离线模式不可修改云端数据".into();
                                } else {
                                    self.loading = true;
                                    let api = cx.app.repo.api.account_snapshot();
                                    cx.spawn(move || {
                                        FolderMutation(api.mutate(
                                            "DELETE",
                                            &path,
                                            &serde_json::Value::Null,
                                        ))
                                    });
                                }
                            }
                        }
                        cx.request_redraw(RefreshHint::Clean);
                        return Transition::None;
                    }
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
                    if *field == usize::MAX - 1 || *field == usize::MAX - 2 {
                        self.managing_folder = None;
                    }
                    self.picker = None;
                    cx.request_redraw(RefreshHint::Clean);
                }
                _ => {}
            }
            return Transition::None;
        }
        if !self.editing && !self.discover && !self.loading {
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
                        if !self.history && !self.local {
                            if let Some(folder) = self
                                .folders
                                .get(self.page * self.rows + (id.0 - 200) as usize)
                            {
                                let folder_id = folder["id"].as_str().unwrap_or("").to_owned();
                                let title = folder["title"].as_str().unwrap_or("收藏夹").to_owned();
                                let section = if self.shelf_source == 2 {
                                    "wenku"
                                } else {
                                    "web"
                                };
                                let path = format!(
                                    "user/favored-{section}/{}",
                                    crate::api::encode(&folder_id)
                                );
                                self.managing_folder = Some((path, title.clone()));
                                self.picker = Some((
                                    usize::MAX - 1,
                                    choice::Choice::new(
                                        &title,
                                        ["重命名收藏夹", "删除收藏夹", "取消"],
                                        None,
                                    ),
                                ));
                                cx.request_redraw(RefreshHint::Ui);
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
            if self.renaming_folder && matches!(hit(cx, event), Some(1) | Some(25)) {
                self.editing = false;
                self.renaming_folder = false;
                self.managing_folder = None;
                self.editor.set_text("");
                cx.request_redraw(RefreshHint::Clean);
                return Transition::None;
            }
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
                            let text = self.editor.text();
                            self.editing = false;
                            if self.renaming_folder {
                                self.renaming_folder = false;
                                let folder = self.managing_folder.take();
                                if !text.trim().is_empty() {
                                    if let Some((path, _)) = folder {
                                        self.generation += 1;
                                        self.loading = false;
                                        if cx.app.repo.offline {
                                            self.notice = "离线模式不可修改云端数据".into();
                                        } else {
                                            self.loading = true;
                                            let api = cx.app.repo.api.account_snapshot();
                                            cx.spawn(move || {
                                                FolderMutation(api.mutate(
                                                    "PUT",
                                                    &path,
                                                    &serde_json::json!({ "title": text }),
                                                ))
                                            });
                                        }
                                    }
                                }
                                cx.request_redraw(RefreshHint::Ui);
                            } else {
                                self.query.query = text;
                                self.page = 0;
                                self.load(cx, true);
                            }
                        }
                        EditorEvent::Edited => cx.request_redraw(RefreshHint::Ui),
                        _ => {}
                    }
                    return Transition::None;
                }
            }
            if self.renaming_folder {
                // Editing a folder is modal: unrelated list/navigation hits cannot alter its target.
                return Transition::None;
            }
        }
        if !self.local && !self.return_on_close {
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
        }
        match page_hit(cx, event) {
            Some(98) => self.load(cx, true),
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
                return Transition::Root(Box::new(cloud::CloudPage::root(cloud::Mode::Wenku(
                    String::new(),
                    0,
                ))))
            }
            Some(52) => return Transition::Root(Box::new(ranks::RankCategories)),
            Some(63) => {
                self.picker = Some((
                    usize::MAX,
                    choice::Choice::new("清除本地阅读历史？", ["取消", "清除"], Some(0)),
                ))
            }
            Some(30) => return Transition::Push(Box::new(import::ImportPage::default())),
            Some(1) if self.local => return Transition::Back,
            Some(60) if !cx.app.repo.api.signed_in() => {
                return Transition::Push(Box::new(login::LoginPage::new()))
            }
            Some(61) if !cx.app.repo.api.signed_in() => {
                return Transition::Push(Box::new(login::LoginPage::new()))
            }
            Some(64) => return Transition::Push(Box::new(login::LoginPage::new())),
            Some(60) if !self.history && self.shelf_source != 1 => {
                self.shelf_source = 1;
                self.page = 0;
                self.load(cx, true);
            }
            Some(61) if !self.history && self.shelf_source != 2 => {
                self.shelf_source = 2;
                self.page = 0;
                self.load(cx, true);
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
            Some(24) => return Transition::Exit,
            Some(25) => {
                self.editing = false;
                self.renaming_folder = false;
                self.managing_folder = None;
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
                } else if !self.history && !self.local {
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
        Transition::None
    }
}
