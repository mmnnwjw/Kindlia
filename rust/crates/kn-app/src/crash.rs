//! Adapted from KinNovel: panic details stay in local logs, a one-shot notice appears next launch.
use std::path::Path;
pub fn install(root: &Path) {
    let marker = root.join("logs/last_crash.txt");
    std::panic::set_hook(Box::new(move |info| {
        let thread = std::thread::current();
        let detail = format!(
            "Kindlia {} / {}: {info}\n{}",
            env!("CARGO_PKG_VERSION"),
            thread.name().unwrap_or("unnamed"),
            std::backtrace::Backtrace::force_capture()
        );
        eprintln!("[panic] {detail}");
        if let Some(dir) = marker.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(&marker, detail);
    }));
}
pub fn take_notice(root: &Path) -> Option<String> {
    let marker = root.join("logs/last_crash.txt");
    let file = std::fs::File::open(&marker).ok()?;
    use std::io::Read;
    let mut text = String::new();
    let _ = file.take(64 * 1024).read_to_string(&mut text);
    let _ = std::fs::remove_file(marker);
    eprintln!("[recovery] {}", text.trim());
    Some("上次异常退出；阅读进度已尝试恢复，详情见 logs/kindlia.log".into())
}
