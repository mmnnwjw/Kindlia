//! Opt-in offline sampling with real fonts and reader code. No Kindle display/input access.
use kindlia_app::{
    epub,
    pages::reader::ReaderPage,
    store::{Config, Store},
    Kindlia,
};
use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
fn ms(at: Instant) -> f64 {
    at.elapsed().as_secs_f64() * 1000.0
}
pub(super) fn run(path: &str, root: PathBuf, mut config: Config) -> Result<(), String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    config.offline = true;
    config.landscape = false;
    let store = Store::new(root.clone(), &config.api_base_url);
    let at = Instant::now();
    let key = epub::import(
        &store,
        &("profile", kindlia_app::model::digest(&bytes)),
        &bytes,
        Path::new(path)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("profile.epub"),
    )?;
    let import_ms = ms(at);
    let book = epub::index(&store, &key)?;
    let mut navigation_checks = vec![];
    for n in book
        .navigation
        .iter()
        .filter(|n| n.chapter_id.contains('#'))
    {
        let at = Instant::now();
        let doc = epub::document(&store, &key, &n.chapter_id)?;
        let target = doc
            .blocks
            .iter()
            .find(|b| b.path.starts_with("/epub-anchor/"));
        navigation_checks.push(serde_json::json!({"id":n.chapter_id,"title":n.title,"target":target.map(|b|b.text.chars().take(80).collect::<String>()),"parse_ms":ms(at)}));
    }
    let mut selected = (0, 0);
    let mut parses = vec![];
    for i in 0..book.chapters.len() {
        let at = Instant::now();
        let doc = epub::document(&store, &key, &i.to_string())?;
        let parse_ms = ms(at);
        let chars = doc
            .blocks
            .iter()
            .map(|b| b.text.chars().count())
            .sum::<usize>();
        if chars > selected.1 {
            selected = (i, chars);
        }
        parses.push(serde_json::json!({"title":book.chapters[i].title,"chapter":i,"parse_ms":parse_ms,"blocks":doc.blocks.len(),"chars":chars}));
    }
    let mut fonts = kn_render::FontStore::new();
    let chain = super::load_fonts(&mut fonts, &config, &root)?;
    let at = Instant::now();
    let doc = epub::document(&store, &key, &selected.0.to_string())?;
    let reopen_parse_ms = ms(at);
    let params = kn_text::LayoutParams {
        width: 1236,
        height: (1648 - kn_ui::widgets::Metrics::for_screen(1236, 1648).touch) as i64,
        font_size: config.font_size,
        margin: config.margin as i64,
        line_spacing: config.line_spacing,
        first_line_indent: config.first_line_indent,
        paragraph_spacing: config.paragraph_spacing,
        rich: true,
        real_image_size: true,
        strict_kinsoku: true,
        ..Default::default()
    };
    let mut paginator = kn_text::Paginator::new(doc.blocks, params);
    let mut widths = kn_text::WidthCache::default();
    let mut measure = kn_text::FontMeasure::new(&mut fonts, &chain, &mut widths);
    let at = Instant::now();
    let mut pages = usize::from(paginator.next_page(&mut measure).is_some());
    let first_page_ms = ms(at);
    while paginator.next_page(&mut measure).is_some() {
        pages += 1;
    }
    let paginate_ms = ms(at);
    drop(measure);
    drop(paginator);
    let app = Kindlia::new(
        root,
        config,
        chain,
        1236,
        1648,
        300,
        kn_platform::Frontlight::detect(None),
    )?;
    let mut ui = kn_ui::Headless::new(app, fonts, 1236, 1648).map_err(|e| e.to_string())?;
    ui.push(Box::new(ReaderPage::new(
        key.clone(),
        selected.0.to_string(),
    )));
    ui.settle(Duration::from_secs(10));
    let _ = ui.frame();
    let at = Instant::now();
    for _ in 0..50 {
        std::hint::black_box(ui.frame());
    }
    let cached_frame_ms = ms(at) / 50.0;
    // Changing only reference gray invalidates the bitmap without changing pagination.
    let gray = ui.app().config.original_gray;
    let at = Instant::now();
    for i in 0..50 {
        ui.app().config.original_gray = if i % 2 == 0 {
            gray.saturating_add(1)
        } else {
            gray
        };
        std::hint::black_box(ui.frame());
    }
    let repaint_frame_ms = ms(at) / 50.0;
    println!("{}", serde_json::to_string_pretty(&serde_json::json!({
        "build": if cfg!(debug_assertions) { "dev opt-level=1" } else { "release" },"key":key,"target":std::env::consts::ARCH,"viewport":[1236,1648],"bytes":bytes.len(),"chapters":book.chapters.len(),
        "navigation":book.navigation,"navigation_checks":navigation_checks,"import_ms":import_ms,"chapter_parse":parses,"selected_chapter":selected.0,"selected_chars":selected.1,
        "reopen_parse_ms":reopen_parse_ms,"first_page_ms":first_page_ms,"paginate_ms":paginate_ms,"pages":pages,
        "cached_frame_ms":cached_frame_ms,"repaint_frame_ms":repaint_frame_ms,
        "note":"host sampling; frame includes allocation/header, excludes e-ink refresh; repaint invalidates bitmap only"
    })).map_err(|e| e.to_string())?);
    Ok(())
}
