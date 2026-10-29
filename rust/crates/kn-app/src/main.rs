mod profile;
// Kindlia KUAL 入口与主机无设备预览入口。
use kindlia_app::{
    pages,
    store::{self, Config},
    Kindlia,
};
use kn_render::{FontId, FontStore};
use std::{path::Path, time::Duration};

fn load_fonts(fonts: &mut FontStore, config: &Config, root: &Path) -> Result<Vec<FontId>, String> {
    let mut paths: Vec<std::path::PathBuf> = config
        .font_paths
        .iter()
        .map(|p| {
            let p = Path::new(p);
            if p.is_absolute() {
                p.to_path_buf()
            } else {
                root.join(p)
            }
        })
        .collect();
    paths.extend(
        [
            "/usr/java/lib/fonts/STHeitiMedium.ttf",
            "/usr/java/lib/fonts/TBGothicMed_213.ttf",
            "/usr/java/lib/fonts/TBMinchoMedium_213.ttf",
            "/usr/java/lib/fonts/MTChineseSurrogates.ttf",
            "/usr/java/lib/fonts/code2000.ttf",
            "C:/Windows/Fonts/msyh.ttc",
            "C:/Windows/Fonts/YuGothR.ttc",
            "C:/Windows/Fonts/meiryo.ttc",
            "C:/Windows/Fonts/simhei.ttf",
        ]
        .iter()
        .map(std::path::PathBuf::from),
    );
    let mut chain = vec![];
    for path in paths {
        if !path.exists() {
            continue;
        }
        match fonts.load_file(&path) {
            Ok(font) => chain.push(font),
            Err(e) => eprintln!("[font] {}: {e}", path.display()),
        }
    }
    if chain.is_empty() {
        return Err("找不到字体，请在 config.toml 的 font_paths 中配置中日文字体".into());
    }
    // 回退链能否覆盖常用假名明确记录；不假设系统中文字体一定覆盖日文。
    for ch in ['あ', 'ア', '漢'] {
        let supported = chain.iter().any(|font| fonts.has_char(*font, ch));
        eprintln!("[font] {ch}: supported={supported}；完整日文覆盖仍需实机确认");
    }
    Ok(chain)
}

fn start_page(args: &[String]) -> Option<Box<dyn kn_ui::Page<Kindlia>>> {
    let key = |i: usize| kindlia_app::model::NovelKey {
        provider_id: args[i + 1].clone(),
        novel_id: args[i + 2].clone(),
    };
    if let Some(i) = args
        .iter()
        .position(|s| s == "--read")
        .filter(|i| args.len() > i + 3)
    {
        Some(Box::new(pages::reader::ReaderPage::new(
            key(i),
            args[i + 3].clone(),
        )))
    } else if let Some(i) = args
        .iter()
        .position(|s| s == "--book")
        .filter(|i| args.len() > i + 2)
    {
        Some(Box::new(pages::book::BookPage::new(key(i))))
    } else if let Some(i) = args
        .iter()
        .position(|s| s == "--wenku")
        .filter(|i| args.len() > i + 1)
    {
        Some(Box::new(pages::cloud::CloudPage::new(
            pages::cloud::Mode::WenkuBook(args[i + 1].clone()),
        )))
    } else if args.iter().any(|s| s == "--discover") {
        Some(Box::new(pages::library::LibraryPage::discover()))
    } else {
        None
    }
}

fn preview(
    args: &[String],
    root: std::path::PathBuf,
    config: Config,
    output: &str,
) -> Result<(), String> {
    let mut fonts = FontStore::new();
    let chain = load_fonts(&mut fonts, &config, &root)?;
    let app = Kindlia::new(
        root,
        config,
        chain,
        1236,
        1648,
        300,
        kn_platform::Frontlight::detect(None),
    )?;
    let settings = args
        .iter()
        .any(|s| s == "--settings")
        .then(|| pages::settings::SettingsPage::new(&app));
    let mut headless = kn_ui::Headless::new(app, fonts, 1236, 1648).map_err(|e| e.to_string())?;
    if let Some(page) = settings {
        headless.push(Box::new(page));
    }
    if let Some(page) = start_page(args) {
        headless.push(page);
    }
    headless.settle(Duration::from_secs(5));
    for i in 0..args.len() {
        let kind = match args[i].as_str() {
            "--tap" | "--tap-hit" => Some(kn_platform::GestureKind::Tap),
            "--swipe-left" => Some(kn_platform::GestureKind::SwipeLeft),
            "--swipe-right" => Some(kn_platform::GestureKind::SwipeRight),
            "--swipe-down" => Some(kn_platform::GestureKind::SwipeDown),
            _ => None,
        };
        if let Some(kind) = kind {
            let (x, y) = if args[i] == "--tap-hit" {
                let id = args
                    .get(i + 1)
                    .ok_or("--tap-hit 需要命中 ID")?
                    .parse::<u32>()
                    .map_err(|_| "无效命中 ID")?;
                let _ = headless.frame();
                let r = headless
                    .hits()
                    .rect_of(kn_ui::HitId(id))
                    .ok_or_else(|| format!("页面不存在命中 {id}"))?;
                (r.x + r.w as i32 / 2, r.y + r.h as i32 / 2)
            } else if kind == kn_platform::GestureKind::Tap {
                let value = args.get(i + 1).ok_or("--tap 需要 X,Y")?;
                let (x, y) = value.split_once(',').ok_or("--tap 需要 X,Y")?;
                (
                    x.parse().map_err(|_| "无效 X")?,
                    y.parse().map_err(|_| "无效 Y")?,
                )
            } else {
                (800, 800)
            };
            headless.input(&kn_platform::InputEvent::Gesture(kn_platform::Gesture {
                kind,
                start: (x, y),
                end: (x, y),
                duration_ms: 100,
            }));
            headless.settle(Duration::from_secs(3));
        }
    }
    // Rendering can enqueue cover requests; settle those before exporting the final image.
    let _ = headless.frame();
    headless.settle(Duration::from_secs(3));
    let (frame, _) = headless.frame();
    use std::io::Write;
    let mut file = std::fs::File::create(output).map_err(|e| e.to_string())?;
    write!(file, "P5\n{} {}\n255\n", frame.width(), frame.height()).map_err(|e| e.to_string())?;
    for y in 0..frame.height() {
        file.write_all(frame.row(y)).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn device(root: std::path::PathBuf, config: Config) -> Result<(), String> {
    use kn_platform::{Display, FbinkDisplay, GestureConfig, InputReader, PowerMonitor};
    let mut display = FbinkDisplay::open().map_err(|e| e.to_string())?;
    let info = display.info().clone();
    let upside_down = info.has_gsensor() && std::env::var("KN_ORIENTATION").as_deref() == Ok("D");
    if info.has_gsensor() {
        display.assume_fb_upside_down(upside_down);
        display.set_rotation(kn_platform::Rotation::new(if upside_down { 2 } else { 0 }));
    }
    let mut fonts = FontStore::new();
    let chain = load_fonts(&mut fonts, &config, &root)?;
    let app = Kindlia::new(
        root,
        config,
        chain,
        info.width,
        info.height,
        info.dpi,
        kn_platform::Frontlight::detect(info.caps),
    )?;
    let mut input =
        InputReader::open(&info, GestureConfig::default()).map_err(|e| e.to_string())?;
    if upside_down {
        input.set_rotation(kn_platform::Rotation::new(2));
    }
    kn_platform::display::set_swipe_steps(app.config.swipe_steps.clamp(6, 60));
    let policy = kn_ui::RefreshPolicy {
        full_refresh_every: app.config.full_refresh_every.clamp(2, 12) as f32,
        supports_reagl: info.supports_reagl,
        flash_every_turn: app.config.page_flash,
        ui_mode: if app.config.ui_refresh_mode == "classic" {
            kn_ui::UiRefreshMode::Classic
        } else {
            kn_ui::UiRefreshMode::Smooth
        },
        ui_waveform: if info.is_mtk && info.bytes_per_pixel == 1 {
            kn_platform::Waveform::Auto
        } else {
            kn_platform::Waveform::Gc16
        },
        ..Default::default()
    };
    kn_ui::run(
        app,
        Box::new(display),
        fonts,
        input,
        PowerMonitor::start(),
        kn_ui::RefreshScheduler::new(info.width, info.height, policy),
    )
    .map_err(|e| e.to_string())
}
#[cfg(not(target_os = "linux"))]
fn device(_root: std::path::PathBuf, _config: Config) -> Result<(), String> {
    Err("设备入口只支持 Kindle；主机请用 --preview output.pgm".into())
}

fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|s| s == "--restore-display") {
        #[cfg(target_os = "linux")]
        kn_platform::display::restore_after_crash();
        return Ok(());
    }
    if args.iter().any(|s| s == "--help") {
        println!("Kindlia\n  --preview OUT.pgm [--discover | --book PROVIDER NOVEL | --read PROVIDER NOVEL CHAPTER]\n  [--settings | --wenku NOVEL] [--offline] [--tap X,Y] [--swipe-left | --swipe-right]\n  --check-volume NOVEL VOLUME [--human] (不启动 UI)\n  --profile-epub FILE (本地离线性能采样，不启动 UI)\n数据目录：KINDLIA_HOME，设备默认 /mnt/us/extensions/kindlia");
        return Ok(());
    }
    let root = store::root_dir();
    kindlia_app::migration::device(&root)?;
    if args.iter().any(|s| s == "--migrate-data") {
        println!("Kindlia data migration complete");
        return Ok(());
    }
    let mut config = Config::load(&root)?;
    if args.iter().any(|s| s == "--offline") {
        config.offline = true;
    }
    if let Some(i) = args.iter().position(|s| s == "--profile-epub") {
        return profile::run(
            args.get(i + 1).ok_or("--profile-epub 需要本地 EPUB 路径")?,
            root,
            config,
        );
    }
    if let Some(i) = args.iter().position(|s| s == "--check-volume") {
        let novel = args
            .get(i + 1)
            .ok_or("--check-volume 需要小说 ID 和卷文件名")?;
        let volume = args.get(i + 2).ok_or("--check-volume 需要卷文件名")?;
        let mut api = kindlia_app::api::ApiClient::new(
            &config.api_base_url,
            config.timeout_secs,
            config.strict_tls,
        )?;
        api.configure_gateway(
            config.gateway_enabled,
            &config.gateway_url,
            &config.gateway_key,
        )?;
        let store = store::Store::new(root, &config.api_base_url);
        let repo = kindlia_app::repository::Repository {
            api,
            store,
            offline: false,
        };
        let key = pages::cloud::open_volume(
            &repo,
            &config,
            novel,
            volume,
            args.iter().any(|s| s == "--human"),
        )?;
        let book = kindlia_app::epub::index(&repo.store, &key)?;
        println!("EPUB OK: {} chapters={}", book.title, book.chapters.len());
        return Ok(());
    }
    if let Some(i) = args.iter().position(|s| s == "--preview") {
        preview(
            &args,
            root,
            config,
            args.get(i + 1).ok_or("--preview 需要输出文件名")?,
        )
    } else {
        device(root, config)
    }
}
fn main() {
    if let Err(e) = run() {
        eprintln!("[kindlia] {e}");
        std::process::exit(1);
    }
}
