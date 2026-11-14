use super::*;

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
            eprintln!(
                "[wenku] incomplete ZIP ({} bytes), attempt {}: {error}",
                bytes.len(),
                attempt + 1
            );
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
pub(super) fn fetch(
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
                    badge: String::new(),
                    title: string(f, "title"),
                    subtitle: "收藏夹 · 长按管理".into(),
                    action: Action::Folder(string(f, "id")),
                    cover: String::new(),
                });
            }
        }
        Mode::WenkuBook(id) => {
            items.push(Item {
                badge: String::new(),
                title: "简介与书籍信息".into(),
                subtitle: string(&v, "introduction"),
                action: Action::Info(v.clone()),
                cover: string(&v, "cover"),
            });
            for vol in v["volumeZh"].as_array().into_iter().flatten() {
                if let Some(name) = vol.as_str() {
                    items.push(Item {
                        badge: String::new(),
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
                    badge: String::new(),
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
                    badge: String::new(),
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
                let mut authors = crate::formatting::authors(b);
                if authors.is_empty() {
                    match &action {
                        Action::Web(key) => {
                            if let Some(n) =
                                repo.store.read_cache::<crate::model::Novel>("novels", key)
                            {
                                authors = n.authors.into_iter().map(|a| a.name).collect();
                            }
                        }
                        Action::Wenku(id) => {
                            let meta = crate::covers::metadata(
                                repo,
                                &NovelKey {
                                    provider_id: "wenku".into(),
                                    novel_id: id.clone(),
                                },
                                false,
                            );
                            if !meta.author.is_empty() {
                                authors.push(meta.author);
                            }
                        }
                        _ => {}
                    }
                }
                items.push(Item {
                    badge: crate::formatting::book_badge(b),
                    title: title(b),
                    subtitle: if authors.is_empty() {
                        string(b, "providerId")
                    } else {
                        authors.join(" / ")
                    },
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
pub(super) fn rank_options(
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
