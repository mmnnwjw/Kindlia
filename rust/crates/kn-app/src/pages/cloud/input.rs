use super::*;

impl CloudPage {
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
        if let Some((kind, picker)) = &mut self.choice {
            match picker.input(cx, event) {
                choice::Event::Selected(n) => {
                    let kind = *kind;
                    self.choice = None;
                    match kind {
                        9 => {
                            if n == 1 {
                                self.mutate(cx, "DELETE", "user/read-history".into(), Value::Null);
                            }
                        }
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
                        20 => {
                            match n {
                                0 => {
                                    if let Some(CloudConfirm::FolderMenu { wenku, id, title }) =
                                        self.confirm.take()
                                    {
                                        self.rename_target = Some(format!(
                                            "user/favored-{}/{}",
                                            if wenku { "wenku" } else { "web" },
                                            encode(&id)
                                        ));
                                        self.editor(&title, 2);
                                    }
                                }
                                1 => {
                                    if let Some(CloudConfirm::FolderMenu { wenku, id, .. }) =
                                        self.confirm.take()
                                    {
                                        self.confirm = Some(CloudConfirm::DeletePath(format!(
                                            "user/favored-{}/{}",
                                            if wenku { "wenku" } else { "web" },
                                            encode(&id)
                                        )));
                                        self.choice = Some((
                                            21,
                                            choice::Choice::new(
                                                "删除收藏夹？",
                                                ["取消", "删除"],
                                                Some(0),
                                            ),
                                        ));
                                    }
                                }
                                _ => self.confirm = None,
                            }
                        }
                        21 => {
                            let pending = self.confirm.take();
                            if n == 1 {
                                if let Some(CloudConfirm::DeletePath(path)) = pending {
                                    self.mutate(cx, "DELETE", path, Value::Null);
                                }
                            }
                        }
                        _ => {}
                    }
                    cx.request_redraw(RefreshHint::Clean);
                }
                choice::Event::Cancel => {
                    if *kind == 20 || *kind == 21 {
                        self.confirm = None;
                    }
                    self.choice = None;
                    cx.request_redraw(RefreshHint::Clean);
                }
                _ => {}
            }
            return Transition::None;
        }
        if hit(cx, event) == Some(1) && self.editing && !self.searching {
            self.editing = false;
            self.rename_target = None;
            cx.request_redraw(RefreshHint::Clean);
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
        let h = page_hit(cx, event);
        match h {
            Some(98) => self.load_with_cache(cx, false),
            Some(60_000) => return Transition::Root(Box::new(library::LibraryPage::discover())),
            Some(60_001) => return Transition::None,
            Some(60_002) => return Transition::Root(Box::new(ranks::RankCategories)),
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
                if matches!(self.mode, Mode::WenkuBook(_) | Mode::Comments(..))
                    && !cx.app.repo.api.signed_in()
                {
                    return Transition::Push(Box::new(login::LoginPage::new()));
                }
                match self.mode.clone() {
                    Mode::Wenku(q, _) => {
                        self.searching = true;
                        self.editor(&q, 0);
                    }
                    Mode::WenkuBook(id) => {
                        if let Some(folder) = self.items.first().and_then(|i| {
                            if let Action::Info(v) = &i.action {
                                v["favored"].as_str()
                            } else {
                                None
                            }
                        }) {
                            self.confirm = Some(CloudConfirm::DeletePath(format!(
                                "user/favored-wenku/{}/{}",
                                encode(folder),
                                encode(&id)
                            )));
                            self.choice = Some((
                                21,
                                choice::Choice::new("从云收藏移出？", ["取消", "移出"], Some(0)),
                            ));
                            cx.request_redraw(RefreshHint::Ui);
                            return Transition::None;
                        }
                        return Transition::Push(Box::new(Self::new(Mode::Folders(
                            true,
                            Some(NovelKey {
                                provider_id: "wenku".into(),
                                novel_id: id,
                            }),
                        ))));
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
                    Mode::Comments(..) => self.editor("", 0),
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
                    self.choice = Some((
                        9,
                        choice::Choice::new("清除云端及本地阅读历史？", ["取消", "清除"], Some(0)),
                    ));
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
                _ => return Transition::Back,
            },
            Some(12) => {
                self.load(cx);
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
                        Action::Info(_) => {
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
        // 长按收藏夹打开操作弹窗；长按收藏或历史条目弹出移除确认。
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
                    let offset = if matches!(
                        self.mode,
                        Mode::Folders(..) | Mode::WenkuBook(_) | Mode::Rank(..)
                    ) {
                        self.page * self.rows
                    } else {
                        0
                    };
                    if let Some(item) = self.items.get(offset + (id.0 - 200) as usize) {
                        match (&self.mode, &item.action) {
                            (Mode::Folders(w, _), Action::Folder(id)) => {
                                self.confirm = Some(CloudConfirm::FolderMenu {
                                    wenku: *w,
                                    id: id.clone(),
                                    title: item.title.clone(),
                                });
                                self.choice = Some((
                                    20,
                                    choice::Choice::new(
                                        &item.title,
                                        ["重命名收藏夹", "删除收藏夹", "取消"],
                                        None,
                                    ),
                                ));
                            }
                            (Mode::Favorites(_, f), a) => {
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
                                    self.confirm = Some(CloudConfirm::DeletePath(path));
                                    self.choice = Some((
                                        21,
                                        choice::Choice::new(
                                            "从云收藏移出？",
                                            ["取消", "移出"],
                                            Some(0),
                                        ),
                                    ));
                                }
                            }
                            (Mode::History, Action::Web(k)) => {
                                self.confirm = Some(CloudConfirm::DeletePath(format!(
                                    "user/read-history/{}/{}",
                                    encode(&k.provider_id),
                                    encode(&k.novel_id)
                                )));
                                self.choice = Some((
                                    21,
                                    choice::Choice::new(
                                        "从历史记录移出？",
                                        ["取消", "移出"],
                                        Some(0),
                                    ),
                                ));
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
}
