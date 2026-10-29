use super::*;
use std::io::Write;
fn temporary() -> std::path::PathBuf {
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let p = std::env::temp_dir().join(format!(
        "kindlia-v2-{}-{}",
        std::process::id(),
        N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&p).unwrap();
    p
}
fn synthetic_epub() -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, text) in [
        (
            "META-INF/container.xml",
            r#"<container><rootfiles><rootfile full-path="OEBPS/book.opf"/></rootfiles></container>"#,
        ),
        (
            "OEBPS/book.opf",
            r#"<package><metadata><title>合成测试书</title></metadata><manifest><item id="b" href="text/second.xhtml"/><item id="a" href="text/first.xhtml"/><item id="nav" properties="nav" href="nav.xhtml"/></manifest><spine><itemref idref="a"/><itemref idref="b"/></spine></package>"#,
        ),
        (
            "OEBPS/nav.xhtml",
            r#"<html><body><nav><a href="text/first.xhtml">第一章</a><a href="text/second.xhtml">第二章</a></nav></body></html>"#,
        ),
        (
            "OEBPS/text/first.xhtml",
            r#"<html><body><p>译文甲</p><p style="opacity:0.4;">原文あ</p><p>未翻译的段落</p><img src="../images/插图.png"/></body></html>"#,
        ),
        (
            "OEBPS/text/second.xhtml",
            "<html><body><p>第二章</p></body></html>",
        ),
        ("OEBPS/images/插图.png", "synthetic-image"),
    ] {
        zip.start_file(name, zip::write::FileOptions::default())
            .unwrap();
        zip.write_all(text.as_bytes()).unwrap();
    }
    zip.finish().unwrap().into_inner()
}
#[test]
fn epub_spine_labels_and_relative_images() {
    let root = temporary();
    let store = store::Store::new(root.clone(), "http://localhost");
    let key = epub::import(&store, &"synthetic", &synthetic_epub(), "test.epub").unwrap();
    let book = epub::index(&store, &key).unwrap();
    assert_eq!(book.chapters[0].title, "第一章");
    assert_eq!(book.chapters[1].path, "OEBPS/text/second.xhtml");
    let doc = epub::document(&store, &key, "0").unwrap();
    let image = doc
        .blocks
        .iter()
        .find(|b| b.kind == kn_text::BlockKind::Image)
        .unwrap();
    assert_eq!(
        epub::image(&store, &image.source_url).unwrap(),
        b"synthetic-image"
    );
    assert_eq!(
        epub::chapter(&store, &key, "0").unwrap().next_id.as_deref(),
        Some("1")
    );
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn epub_original_only_keeps_untranslated_paragraph() {
    let root = temporary();
    let store = store::Store::new(root.clone(), "http://localhost");
    let key = epub::import(&store, &"synthetic", &synthetic_epub(), "test.epub").unwrap();
    store
        .write_cache(
            "epub-source",
            &key,
            &serde_json::json!({"human":false,"original":true,"translated":false}),
        )
        .unwrap();
    let doc = epub::document(&store, &key, "0").unwrap();
    let text = doc
        .blocks
        .iter()
        .map(|b| b.text.as_str())
        .collect::<String>();
    assert!(text.contains("原文あ"));
    assert!(text.contains("未翻译的段落"));
    assert!(!text.contains("译文甲"));
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn translation_preference_and_original_are_independent() {
    let c:model::Chapter=serde_json::from_value(serde_json::json!({"titleJp":"title","paragraphs":["原文あ","<图片>https://example.com/i.png",""],"youdaoParagraphs":["有道","ignored",""],"gptParagraphs":["GPT","ignored",""]})).unwrap();
    let text = |p: &str, original, translated| {
        paragraphs::build_preferences(&c, p, original, translated)
            .blocks
            .into_iter()
            .map(|b| b.text)
            .collect::<Vec<_>>()
    };
    assert_eq!(text("youdao", false, true)[0], "有道");
    assert_eq!(&text("gpt", true, true)[..2], &["GPT", "原文あ"]);
    assert_eq!(text("gpt", true, false)[0], "原文あ");
    let b = paragraphs::build_preferences(&c, "youdao", true, true);
    let original = b.blocks.iter().find(|b| b.path == "/jp/0").unwrap();
    assert!(original.spans.iter().all(|s| s.style.muted));
    assert_eq!(b.blocks[0].path, "/p/0");
    assert_eq!(
        b.blocks
            .iter()
            .filter(|b| b.kind == kn_text::BlockKind::Image)
            .count(),
        1
    );
}
#[test]
fn old_config_migrates_bilingual_without_losing_preferences() {
    let root = temporary();
    std::fs::write(
        root.join("config.toml"),
        "reading_mode = 'bilingual'\nfont_size = 52\n",
    )
    .unwrap();
    let c = store::Config::load(&root).unwrap();
    assert!(c.show_original && c.show_translation);
    assert_eq!(c.font_size, 52);
    c.save(&root).unwrap();
    assert!(store::Config::load(&root).unwrap().show_original);
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn invalid_epub_does_not_publish_an_index() {
    let root = temporary();
    let store = store::Store::new(root.clone(), "http://localhost");
    assert!(epub::import(&store, &"bad", b"<html>gateway</html>", "bad.epub").is_err());
    let key = model::NovelKey {
        provider_id: "epub".into(),
        novel_id: model::digest(&"bad"),
    };
    assert!(epub::index(&store, &key).is_err());
    assert_eq!(epub::unescape("%あ"), "%あ");
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn epub_css_notes_encoding_and_root_images() {
    use std::io::Read;
    let root = temporary();
    let store = store::Store::new(root.clone(), "http://localhost");
    let mut original = zip::ZipArchive::new(std::io::Cursor::new(synthetic_epub())).unwrap();
    let mut output = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for i in 0..original.len() {
        let mut entry = original.by_index(i).unwrap();
        let mut data = Vec::new();
        entry.read_to_end(&mut data).unwrap();
        let replacement = match entry.name() {
            "OEBPS/nav.xhtml" => Some(
                r#"<!DOCTYPE html><html xmlns:epub="http://www.idpf.org/2007/ops"><nav epub:type="landmarks"><a href="text/first.xhtml">错误标题</a></nav><nav epub:type="toc"><a href="text/first.xhtml">正确目录</a></nav></html>"#,
            ),
            "OEBPS/text/first.xhtml" => Some(
                r##"<!DOCTYPE html><html xmlns:epub="http://www.idpf.org/2007/ops"><head><link rel="stylesheet" href="../theme.css"/></head><body><p class="center" style="font-weight:normal">正文<ruby>字<rt>じ</rt></ruby><a epub:type="noteref" href="../notes.xhtml#n1">注</a></p><p class="gone">隐藏文字</p><img src="/OEBPS/images/%E6%8F%92%E5%9B%BE%25%23.png"/></body></html>"##,
            ),
            _ => None,
        };
        output
            .start_file(entry.name(), zip::write::FileOptions::default())
            .unwrap();
        output
            .write_all(replacement.map(str::as_bytes).unwrap_or(&data))
            .unwrap();
    }
    for (name, data) in [
        ("OEBPS/theme.css", b"p {text-align:right;} .center {text-align:center;} p.center {font-weight:bold !important;} .gone {display:none;}".as_slice()),
        ("OEBPS/images/插图%#.png", b"image-body".as_slice()),
    ] {
        output.start_file(name, zip::write::FileOptions::default()).unwrap();
        output.write_all(data).unwrap();
    }
    output
        .start_file("OEBPS/notes.xhtml", zip::write::FileOptions::default())
        .unwrap();
    output.write_all(&[0xff, 0xfe]).unwrap();
    for unit in "<?xml version=\"1.0\" encoding=\"UTF-16\"?><html><body><aside id=\"n1\">跨文件注释</aside></body></html>".encode_utf16() {
        output.write_all(&unit.to_le_bytes()).unwrap();
    }
    let key = epub::import(
        &store,
        &"compatibility",
        &output.finish().unwrap().into_inner(),
        "compat.epub",
    )
    .unwrap();
    assert_eq!(
        epub::index(&store, &key).unwrap().chapters[0].title,
        "正确目录"
    );
    let doc = epub::document(&store, &key, "0").unwrap();
    let body = doc.blocks.iter().find(|b| b.text.contains("正文")).unwrap();
    assert_eq!(body.style.align, kn_text::style::Align::Center);
    assert!(body.spans.iter().any(|s| s.style.bold));
    assert!(body.spans.iter().any(|s| s.style.ruby));
    assert_eq!(doc.notes, vec!["跨文件注释"]);
    assert!(!doc.blocks.iter().any(|b| b.text.contains("隐藏文字")));
    let image = doc
        .blocks
        .iter()
        .find(|b| b.kind == kn_text::BlockKind::Image)
        .unwrap();
    assert_eq!(
        epub::image(&store, &image.source_url).unwrap(),
        b"image-body"
    );
    assert_eq!(epub::document(&store, &key, "0").unwrap().notes, doc.notes);
    // Pooled ZIP metadata must not survive replacement of the same logical volume.
    let replaced =
        epub::import(&store, &"compatibility", &synthetic_epub(), "compat.epub").unwrap();
    assert_eq!(replaced, key);
    assert!(epub::document(&store, &key, "0").unwrap().notes.is_empty());
    assert!(epub::import(&store, &"compatibility", b"broken", "compat.epub").is_err());
    assert!(epub::document(&store, &key, "0")
        .unwrap()
        .blocks
        .iter()
        .any(|b| b.text.contains("译文甲")));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn gateway_routes_every_service_and_encodes_nested_image_urls() {
    let g = gateway::Gateway::new("https://gateway.example/", "fixture-key-123456").unwrap();
    assert_eq!(g.route("https://n.novelia.cc/api/wenku/id/file/vol.epub?translations=gpt&translations=youdao","main").unwrap(),"https://gateway.example/main/api/wenku/id/file/vol.epub?translations=gpt&translations=youdao");
    assert_eq!(
        g.route("https://auth.novelia.cc/api/v1/auth/login", "auth")
            .unwrap(),
        "https://gateway.example/auth/api/v1/auth/login"
    );
    assert_eq!(
        g.route(
            "https://forum.novelia.cc/api/v1/external/comment/novel/id",
            "forum"
        )
        .unwrap(),
        "https://gateway.example/forum/api/v1/external/comment/novel/id"
    );
    let image = g
        .route("https://i.pximg.net/a.png?one=1&two=2", "image")
        .unwrap();
    let u = url::Url::parse(&image).unwrap();
    assert_eq!(
        u.query_pairs().collect::<Vec<_>>(),
        vec![("url".into(), "https://i.pximg.net/a.png?one=1&two=2".into())]
    );
    assert!(gateway::Gateway::new("http://public.example", "fixture-key-123456").is_err());
    assert!(gateway::Gateway::new("https://gateway.example/path", "fixture-key-123456").is_err());
    assert!(gateway::Gateway::new("https://gateway.example", "bad\r\nheader").is_err());
}

#[test]
fn epub_links_resolve_against_document_paths_even_in_tolerant_html() {
    let doc = kn_text::extract_document("<html><body><p>not well formed<img src='../images/a%23b.png'><img src='/OPS/root.png'></body>", "https://epub.invalid/OPS/text/c.xhtml", true);
    let urls: Vec<_> = doc
        .blocks
        .iter()
        .filter(|b| b.kind == kn_text::BlockKind::Image)
        .map(|b| b.source_url.as_str())
        .collect();
    assert_eq!(
        urls,
        vec![
            "https://epub.invalid/OPS/images/a%23b.png",
            "https://epub.invalid/OPS/root.png"
        ]
    );
}

#[test]
fn unified_directory_migration_preserves_active_settings_and_conflicts() {
    let temp = temporary();
    let old = temp.join("old");
    let app = temp.join("kindlia");
    std::fs::create_dir_all(old.join("data/account")).unwrap();
    std::fs::create_dir_all(old.join("fonts")).unwrap();
    std::fs::create_dir_all(app.join("fonts")).unwrap();
    std::fs::write(old.join("config.toml"), b"reader_font='same.ttf'").unwrap();
    std::fs::write(app.join("config.toml"), b"default").unwrap();
    std::fs::write(
        old.join("data/account/session.json"),
        b"session-not-for-output",
    )
    .unwrap();
    std::fs::write(old.join("fonts/same.ttf"), b"old-font").unwrap();
    std::fs::write(app.join("fonts/same.ttf"), b"new-font").unwrap();
    assert!(migration::migrate(&old, &app).unwrap() > 0);
    assert!(!old.exists());
    assert_eq!(
        std::fs::read(app.join("config.toml")).unwrap(),
        b"reader_font='same.ttf'"
    );
    assert_eq!(
        std::fs::read(app.join("data/account/session.json")).unwrap(),
        b"session-not-for-output"
    );
    assert_eq!(
        std::fs::read(app.join("fonts/same.ttf")).unwrap(),
        b"new-font"
    );
    let record = std::fs::read_dir(app.join("backups"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    assert_eq!(
        std::fs::read(record.join("conflicts/previous-source/fonts/same.ttf")).unwrap(),
        b"old-font"
    );
    assert_eq!(
        std::fs::read(record.join("conflicts/previous-target/config.toml")).unwrap(),
        b"default"
    );
    assert_eq!(migration::migrate(&old, &app).unwrap(), 0);
    assert!(migration::migrate(&app, &app).is_err());
    std::fs::remove_dir_all(temp).unwrap();
}

#[test]
#[cfg(windows)]
fn failed_migration_rolls_back_already_moved_files() {
    use std::os::windows::fs::OpenOptionsExt;
    let temp = temporary();
    let old = temp.join("old");
    let app = temp.join("app");
    std::fs::create_dir_all(&old).unwrap();
    std::fs::write(old.join("a-first"), b"keep-first").unwrap();
    std::fs::write(old.join("z-busy"), b"keep-busy").unwrap();
    let busy = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(old.join("z-busy"))
        .unwrap();
    assert!(migration::migrate(&old, &app).is_err());
    assert_eq!(std::fs::read(old.join("a-first")).unwrap(), b"keep-first");
    assert!(!app.join("a-first").exists());
    drop(busy);
    assert!(migration::migrate(&old, &app).is_ok());
    std::fs::remove_dir_all(temp).unwrap();
}

#[test]
fn history_outbox_is_per_account_and_keeps_latest_chapter() {
    let temp = temporary();
    let store = store::Store::new(temp.clone(), "https://example.com");
    let key = model::NovelKey {
        provider_id: "syosetu".into(),
        novel_id: "book".into(),
    };
    history::enqueue(&store, "alice", &key, "chapter-1", 1).unwrap();
    history::enqueue(&store, "alice", &key, "chapter-2", 2).unwrap();
    history::enqueue(&store, "bob", &key, "chapter-9", 9).unwrap();
    for (user, chapter) in [("alice", "chapter-2"), ("bob", "chapter-9")] {
        let p = store.data_path(&format!("history-outbox-{}.json", model::digest(&user)));
        let v: serde_json::Value = serde_json::from_slice(&std::fs::read(p).unwrap()).unwrap();
        assert_eq!(v.as_object().unwrap().len(), 1);
        assert_eq!(
            v.as_object().unwrap().values().next().unwrap()["chapter"],
            chapter
        );
    }
    std::fs::remove_dir_all(temp).unwrap();
}

#[test]
fn conversion_preserves_note_objects_and_original_reference() {
    let mut doc = kn_text::extract_document(
        r##"<p><b>乾隆年間的頭髮</b><a epub:type="noteref" href="#note">注</a></p><aside id="note">說明</aside>"##,
        "https://example.com",
        true,
    );
    for b in &mut doc.blocks {
        conversion::apply(b, "t2s");
    }
    let body = doc.blocks.iter().find(|b| b.text.contains("头发")).unwrap();
    assert!(body.text.contains("乾隆年间"));
    assert!(body
        .spans
        .iter()
        .all(|s| s.end <= body.text.chars().count()));
    for (at, object) in &body.objects {
        if matches!(object, kn_text::Inline::Note(_)) {
            assert_eq!(body.text.chars().nth(*at), Some(kn_text::OBJECT_CHAR));
        }
    }
    let mut original = kn_text::Block {
        text: "頭髮".into(),
        path: "/jp/1".into(),
        ..Default::default()
    };
    conversion::apply(&mut original, "t2s");
    assert_eq!(original.text, "頭髮");
    let image = covers::fallback((100, 150)).unwrap();
    assert!(image.width() > 0 && image.width() <= 100 && image.height() <= 150);
    let pin = pages::lock::hash_pin("012345");
    let config = store::Config {
        lock_pin: pin,
        ..Default::default()
    };
    assert!(pages::lock::pin_matches(&config, "012345"));
    assert!(!pages::lock::pin_matches(&config, "123456"));
}

#[test]
fn queued_history_request_keeps_its_submitting_account() {
    let api=api::ApiClient::new("https://example.com",10,true).unwrap();
    {let mut s=api.session.lock().unwrap();s.username="alice".into();s.token="synthetic-alice".into();}
    let snapshot=api.account_snapshot();
    {let mut s=api.session.lock().unwrap();s.username="bob".into();s.token="synthetic-bob".into();}
    assert_eq!(snapshot.username(),"alice");assert_eq!(api.username(),"bob");
    assert!(snapshot.session_file.is_none());
}
#[test]
#[cfg(windows)]
fn inline_controls_render_as_spaces_and_fit_on_utf8_boundaries() {
    use kn_render::{Bitmap, FontStore, GlyphCache, TextStyle};
    let mut fonts = FontStore::new();
    let font = fonts.load_file(std::path::Path::new("C:/Windows/Fonts/arial.ttf")).unwrap();
    let chain = [font];
    let style = TextStyle { fonts: &chain, size_px: 24.0 };
    let mut glyphs = GlyphCache::new(1024 * 1024);
    let mut a = Bitmap::new(400, 50, 255);
    let mut b = a.clone();
    glyphs.draw_text(&fonts, &mut a, 4.0, 32.0, "ab\ncd\tEF\u{2028}gh", &style, 0);
    glyphs.draw_text(&fonts, &mut b, 4.0, 32.0, "ab cd EF gh", &style, 0);
    assert_eq!(a.data(), b.data());
    assert_eq!(fonts.measure("ab\ncd\tEF\u{2028}gh", &style), fonts.measure("ab cd EF gh", &style));
    for width in 0..200 { let _ = fonts.fit("ab\u{2028}中文ef", &style, width as f32); }
}

#[test]
#[cfg(windows)]
fn reader_font_is_reused_and_switch_drops_old_glyphs() {
    let root = temporary();
    std::fs::create_dir_all(root.join("fonts")).unwrap();
    for name in ["a.ttf", "b.ttf"] {
        std::fs::copy("C:/Windows/Fonts/arial.ttf", root.join("fonts").join(name)).unwrap();
    }
    let mut store = kn_render::FontStore::new();
    let mut glyphs = kn_render::GlyphCache::new(1024 * 1024);
    let mut cached = None;
    let (a, _) = fonts::select(&mut store, &root, "a.ttf", &[], &mut glyphs, &mut cached);
    let (b, _) = fonts::select(&mut store, &root, "a.ttf", &[], &mut glyphs, &mut cached);
    assert_eq!(a, b);
    let (c, _) = fonts::select(&mut store, &root, "b.ttf", &[], &mut glyphs, &mut cached);
    assert_ne!(a, c);
    assert_eq!(cached.unwrap().0, "b.ttf");
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn epub_kobo_self_closing_script_does_not_escape_body_or_hide_cover() {
    let original=synthetic_epub();
    let mut input=zip::ZipArchive::new(std::io::Cursor::new(original)).unwrap();
    let mut output=zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for i in 0..input.len() {
        let mut entry=input.by_index(i).unwrap();
        output.start_file(entry.name(),zip::write::FileOptions::default()).unwrap();
        if entry.name()=="OEBPS/text/first.xhtml" {
            output.write_all(br#"<?xml version="1.0"?><html xmlns="http://www.w3.org/1999/xhtml"><head><title>INVISIBLE TITLE</title><script src="kobo.js"/><style>p {font-weight:bold}</style></head><body><script src="inline.js"/><p>VISIBLE BODY</p><svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink"><image width="1443" height="2048" xlink:href="../images/cover.png"/></svg></body></html>"#).unwrap();
        } else {std::io::copy(&mut entry,&mut output).unwrap();}
    }
    let root=temporary();let store=store::Store::new(root.clone(),"https://example.com");
    let key=epub::import(&store,&"kobo-test",&output.finish().unwrap().into_inner(),"kobo.epub").unwrap();
    let doc=epub::document(&store,&key,"0").unwrap();
    let text=doc.blocks.iter().map(|b|b.text.as_str()).collect::<String>();
    assert_eq!(text,"VISIBLE BODY");
    assert!(doc.blocks.iter().any(|b|b.kind==kn_text::BlockKind::Image && b.source_url.starts_with("epub://")));
    let image=doc.blocks.iter().find(|b|b.kind==kn_text::BlockKind::Image).unwrap();
    assert_eq!((image.width,image.height),(1443,2048));
    assert!(doc.blocks.iter().flat_map(|b|&b.spans).any(|s|s.style.bold));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
#[cfg(windows)]
fn reader_loads_custom_font_through_local_worker() {
    let root=temporary();std::fs::create_dir_all(root.join("fonts")).unwrap();
    std::fs::copy("C:/Windows/Fonts/arial.ttf",root.join("fonts/reader.ttf")).unwrap();
    let store=store::Store::new(root.clone(),"https://example.com");
    let key=epub::import(&store,&"async-font",&synthetic_epub(),"sample.epub").unwrap();
    let mut fonts=kn_render::FontStore::new();
    let ui=fonts.load_file(std::path::Path::new("C:/Windows/Fonts/arial.ttf")).unwrap();
    let config=store::Config{api_base_url:"https://example.com".into(),offline:true,operation_tips:false,welcome_skipped:true,reader_font:"reader.ttf".into(),..Default::default()};
    let app=Kindlia::new(root.clone(),config,vec![ui],600,800,167,kn_platform::Frontlight::detect(None)).unwrap();
    let mut ui=kn_ui::Headless::new(app,fonts,600,800).unwrap();
    ui.push(Box::new(pages::reader::ReaderPage::new(key,"0".into())));
    ui.settle(std::time::Duration::from_secs(3));
    let _=ui.frame();
    assert_eq!(ui.app().reader_font_cache.as_ref().map(|p|p.0.as_str()),Some("reader.ttf"));
    let id=ui.app().reader_font_cache.as_ref().unwrap().1;
    ui.push(Box::new(pages::reader::ReaderPage::new(model::NovelKey{provider_id:"epub".into(),novel_id:"missing".into()},"0".into())));
    ui.settle(std::time::Duration::from_secs(1));
    assert_eq!(ui.app().reader_font_cache.as_ref().unwrap().1,id);
    drop(ui);std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn epub_navigation_preserves_fragments_and_old_spine_ids() {
    let original=synthetic_epub();
    let mut input=zip::ZipArchive::new(std::io::Cursor::new(original)).unwrap();
    let mut output=zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for i in 0..input.len() {
        let mut entry=input.by_index(i).unwrap();
        output.start_file(entry.name(),zip::write::FileOptions::default()).unwrap();
        if entry.name()=="OEBPS/nav.xhtml" {
            output.write_all(br##"<html><body><nav xmlns:epub="http://www.idpf.org/2007/ops" epub:type="toc"><ol><li><a href="text/first.xhtml">First</a><ol><li><a href="text/first.xhtml#part-2">Second section</a></li></ol></li><li><a href="text/second.xhtml">Last</a></li></ol></nav></body></html>"##).unwrap();
        } else if entry.name()=="OEBPS/text/first.xhtml" {
            output.write_all(br#"<html><body><p>BEFORE</p><p id="part-2">TARGET</p><p>AFTER</p></body></html>"#).unwrap();
        } else {std::io::copy(&mut entry,&mut output).unwrap();}
    }
    let root=temporary();let store=store::Store::new(root.clone(),"https://example.com");
    let key=epub::import(&store,&"fragments",&output.finish().unwrap().into_inner(),"test.epub").unwrap();
    let mut book=epub::index(&store,&key).unwrap();
    assert_eq!(book.chapters.len(),2);assert_eq!(book.navigation.len(),3);
    assert_eq!(book.navigation[1].chapter_id,"0#part-2");assert_eq!(book.navigation[1].depth,1);
    let doc=epub::document(&store,&key,"0#part-2").unwrap();
    assert_eq!(doc.blocks.iter().find(|b|b.path=="/epub-anchor/part-2").unwrap().text,"TARGET");
    assert_eq!(doc.blocks[0].text,"BEFORE");
    assert_eq!(epub::chapter(&store,&key,"0#part-2").unwrap().title_jp,"Second section");
    // An index from a previous release upgrades without changing chapter IDs or progress paths.
    book.index_version=0;book.navigation.clear();store.write_cache("epub-index",&key,&book).unwrap();
    let migrated=epub::index(&store,&key).unwrap();assert_eq!(migrated.navigation.len(),3);
    assert_eq!(migrated.chapters[1].path,book.chapters[1].path);
    assert_eq!(epub::document(&store,&key,"0").unwrap().blocks[1].path,"/epub/1");
    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(windows)]
fn modal_test_ui() -> (std::path::PathBuf,kn_ui::Headless<Kindlia>) {
    let root=temporary();let mut fonts=kn_render::FontStore::new();
    let font=fonts.load_file(std::path::Path::new("C:/Windows/Fonts/arial.ttf")).unwrap();
    let config=store::Config{api_base_url:"https://example.com".into(),offline:true,operation_tips:false,welcome_skipped:true,..Default::default()};
    let app=Kindlia::new(root.clone(),config,vec![font],600,800,167,kn_platform::Frontlight::detect(None)).unwrap();
    let mut ui=kn_ui::Headless::new(app,fonts,600,800).unwrap();ui.settle(std::time::Duration::from_secs(1));
    (root,ui)
}
#[cfg(windows)]
fn tap_ui(ui:&mut kn_ui::Headless<Kindlia>,x:i32,y:i32) {
    ui.input(&kn_platform::InputEvent::Gesture(kn_platform::Gesture{kind:kn_platform::GestureKind::Tap,start:(x,y),end:(x,y),duration_ms:60}));
}
#[test]
#[cfg(windows)]
fn comment_first_and_middle_lines_open_modal_over_thread() {
    let (root,mut ui)=modal_test_ui();
    ui.push(Box::new(pages::comments::Thread::new("test".into(),serde_json::json!({"id":1,"authorUsername":"someone","content":"line one\nline two\nline three\nline four","status":0}))));
    ui.settle(std::time::Duration::from_secs(1));
    let m=ui.app().metrics;let top=m.header_h()+m.margin;
    let before=ui.frame().0;
    for row in [0,1,3] {
        tap_ui(&mut ui,80,(top+(m.body*1.45*(row as f32+0.5)) as u32) as i32);
        let popup=ui.frame().0;
        assert_ne!(before.data(),popup.data(),"comment row {row} did not open actions");
        // The underlying header and the outside card region are preserved.
        assert_eq!(&before.data()[..600*m.header_h() as usize],&popup.data()[..600*m.header_h() as usize]);
        tap_ui(&mut ui,2,2);
        assert_eq!(before.data(),ui.frame().0.data());
    }
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
#[cfg(windows)]
fn navigation_does_not_wait_for_history_response() {
    use std::{io::{Read,Write},time::{Duration,Instant}};
    let listener=std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let base=format!("http://{}",listener.local_addr().unwrap());
    let (arrived_tx,arrived_rx)=std::sync::mpsc::channel();
    let (release_tx,release_rx)=std::sync::mpsc::channel();
    let server=std::thread::spawn(move || {
        let (mut stream,_)=listener.accept().unwrap();let mut buf=[0;4096];let _=stream.read(&mut buf);
        arrived_tx.send(()).unwrap();let _=release_rx.recv_timeout(Duration::from_secs(5));
        let body=r#"{"items":[],"total":0}"#;
        let _=write!(stream,"HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",body.len(),body);
    });
    let (root,mut ui)=modal_test_ui();
    ui.app().repo.api=api::ApiClient::new(&base,5,false).unwrap();
    {let mut s=ui.app().repo.api.session.lock().unwrap();s.username="test".into();s.token="synthetic-test".into();}
    ui.app().repo.offline=false;
    ui.push(Box::new(pages::cloud::CloudPage::new(pages::cloud::Mode::History)));
    arrived_rx.recv_timeout(Duration::from_secs(3)).unwrap();
    let waiting=ui.frame().0;
    let at=Instant::now();tap_ui(&mut ui,550,775);let next=ui.frame().0;let elapsed=at.elapsed();
    let _=release_tx.send(());server.join().unwrap();
    assert_ne!(waiting.data(),next.data(),"History swallowed intentional navigation");
    assert!(elapsed<Duration::from_secs(1),"navigation waited for network: {elapsed:?}");
    eprintln!("history blocked response: navigation {elapsed:?}");
    ui.settle(Duration::from_millis(200));drop(ui);std::fs::remove_dir_all(root).unwrap();
}

#[test]
#[cfg(windows)]
fn press_feedback_does_not_request_full_page_render() {
    let (root,mut ui)=modal_test_ui();
    let pages:Vec<Box<dyn kn_ui::Page<Kindlia>>>=vec![
        Box::new(pages::me::MePage::default()),
        Box::new(pages::settings::SettingsPage::new(ui.app())),
        Box::new(pages::library::LibraryPage::discover()),
        Box::new(pages::text::TextPage::new("Info","Body")),
    ];
    for page in pages {
        let id=page.id();ui.push(page);ui.settle(std::time::Duration::from_millis(50));let _=ui.frame();
        ui.input(&kn_platform::InputEvent::Gesture(kn_platform::Gesture{kind:kn_platform::GestureKind::Down,start:(250,400),end:(250,400),duration_ms:0}));
        assert!(ui.frame().1.is_none(),"{id:?} rendered the full page on press, before the actual tap");
    }
    drop(ui);std::fs::remove_dir_all(root).unwrap();
}
#[test]
#[cfg(windows)]
fn cached_web_reader_bypasses_busy_network_workers() {
    use std::sync::{Arc,Mutex,Condvar};
    struct BlockNet(Arc<(Mutex<bool>,Condvar)>,std::sync::mpsc::Sender<()>);
    impl kn_ui::Page<Kindlia> for BlockNet {
        fn id(&self)->kn_ui::PageId {kn_ui::PageId(998)}
        fn enter(&mut self,cx:&mut kn_ui::Cx<Kindlia>,_:bool) {
            for _ in 0..3 {let gate=self.0.clone();let tx=self.1.clone();cx.spawn(move||{tx.send(()).unwrap();let (lock,cv)=&*gate;let ready=lock.lock().unwrap();let _guard=cv.wait_while(ready,|done| !*done).unwrap();});}
        }
        fn render(&mut self,_:&mut kn_ui::Cx<Kindlia>,_:&mut kn_render::Bitmap) {}
        fn on_input(&mut self,_:&mut kn_ui::Cx<Kindlia>,_:&kn_platform::InputEvent)->kn_ui::Transition<Kindlia> {kn_ui::Transition::None}
    }
    let (root,mut ui)=modal_test_ui();std::fs::create_dir_all(root.join("fonts")).unwrap();std::fs::copy("C:/Windows/Fonts/arial.ttf",root.join("fonts/reader.ttf")).unwrap();ui.app().config.reader_font="reader.ttf".into();
    let key=model::NovelKey{provider_id:"syosetu".into(),novel_id:"perf-fixture".into()};
    let chapter:model::Chapter=serde_json::from_value(serde_json::json!({"titleJp":"Cached fixture","paragraphs":["Readable without network"]})).unwrap();
    ui.app().repo.store.write_cache("chapters",&(&key,"1"),&chapter).unwrap();
    let gate=Arc::new((Mutex::new(false),Condvar::new()));let(tx,rx)=std::sync::mpsc::channel();ui.push(Box::new(BlockNet(gate.clone(),tx)));
    for _ in 0..3 {rx.recv_timeout(std::time::Duration::from_secs(1)).unwrap();}
    ui.push(Box::new(pages::reader::ReaderPage::new(key,"1".into())));ui.settle(std::time::Duration::from_millis(300));
    let ready=ui.app().reader_font_cache.is_some();
    {let(lock,cv)=&*gate;*lock.lock().unwrap()=true;cv.notify_all();}
    ui.settle(std::time::Duration::from_secs(1));drop(ui);std::fs::remove_dir_all(root).unwrap();
    assert!(ready,"cached web chapter was queued behind unrelated stalled network requests");
}

#[test]
fn account_status_does_not_wait_for_login_network() {
    use std::{io::{Read, Write}, sync::mpsc, time::Duration};
    let listener=std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let mut api=api::ApiClient::new("https://example.com",3,false).unwrap();
    api.auth_base=format!("http://{}",listener.local_addr().unwrap());
    let (arrived_tx,arrived_rx)=mpsc::channel();
    let (release_tx,release_rx)=mpsc::channel();
    let server=std::thread::spawn(move || {
        let (mut stream,_)=listener.accept().unwrap();
        let mut bytes=[0;4096];let _=stream.read(&mut bytes);
        arrived_tx.send(()).unwrap();let _=release_rx.recv_timeout(Duration::from_secs(3));
        let _=write!(stream,"HTTP/1.1 200 OK\r\nContent-Length: 9\r\nConnection: close\r\n\r\nsynthetic");
    });
    let login_api=api.clone();
    let logout_api=api.clone();
    let login=std::thread::spawn(move || login_api.login("fixture", "fixture"));
    arrived_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    let (read_tx,read_rx)=mpsc::channel();
    let status=std::thread::spawn(move || {read_tx.send((api.signed_in(),api.username())).unwrap();});
    let response=read_rx.recv_timeout(Duration::from_millis(200));
    // 新的退出必须立即完成，且随后返回的旧登录响应不能让账号复活。
    if response.is_ok() { logout_api.logout().unwrap(); }
    let _=release_tx.send(());server.join().unwrap();let login_result=login.join().unwrap();status.join().unwrap();
    assert!(response.is_ok(),"UI account status waited for an unrelated login HTTP response");
    assert!(login_result.is_err(),"old login overwrote a newer logout");
    assert!(!logout_api.signed_in());
}
