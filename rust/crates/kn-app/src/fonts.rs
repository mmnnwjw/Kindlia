use kn_render::{FontId, FontStore};
use std::path::{Path, PathBuf};

pub fn directory(root: &Path) -> PathBuf {
    root.join("fonts")
}
pub fn list(root: &Path) -> Vec<String> {
    let dir = directory(root);
    let _ = std::fs::create_dir_all(&dir);
    let mut names: Vec<_> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().is_file())
        .filter_map(|e| {
            let p = e.path();
            let ext = p.extension()?.to_str()?.to_ascii_lowercase();
            matches!(ext.as_str(), "ttf" | "otf" | "ttc")
                .then(|| e.file_name().to_string_lossy().into_owned())
        })
        .collect();
    names.sort();
    names
}
pub fn select(
    fonts: &mut FontStore,
    root: &Path,
    name: &str,
    ui: &[FontId],
    glyphs: &mut kn_render::GlyphCache,
    loaded: &mut Option<(String, FontId)>,
) -> (Vec<FontId>, Option<String>) {
    if name.is_empty() {
        return (ui.to_vec(), None);
    }
    if let Some((old, id)) = loaded {
        if old == name {
            let mut c = vec![*id];
            c.extend_from_slice(ui);
            return (c, None);
        }
    }
    install(fonts, glyphs, name, read(root, name), ui, loaded)
}

/// File access belongs to the local worker, not a rendering pass.
pub fn read(root: &Path, name: &str) -> Result<Vec<u8>, String> {
    if !list(root).iter().any(|n| n == name) {
        return Err(format!("字体不存在：{name}"));
    }
    let bytes =
        std::fs::read(directory(root).join(name)).map_err(|e| format!("字体读取失败：{e}"))?;
    if bytes.starts_with(b"wOF2") {
        kn_render::decode_woff2(&bytes).map_err(|e| format!("字体解码失败：{e}"))
    } else {
        Ok(bytes)
    }
}

pub fn install(
    fonts: &mut FontStore,
    glyphs: &mut kn_render::GlyphCache,
    name: &str,
    bytes: Result<Vec<u8>, String>,
    ui: &[FontId],
    loaded: &mut Option<(String, FontId)>,
) -> (Vec<FontId>, Option<String>) {
    if let Some((_, id)) = loaded.as_ref().filter(|(old, _)| old == name) {
        return (
            std::iter::once(*id).chain(ui.iter().copied()).collect(),
            None,
        );
    }
    match bytes.and_then(|bytes| {
        fonts
            .load_bytes(bytes)
            .map_err(|e| format!("字体加载失败：{e}"))
    }) {
        Ok(id) => {
            if let Some((_, old)) = loaded.take() {
                glyphs.forget_font(old);
                fonts.unload(old);
            }
            *loaded = Some((name.into(), id));
            (
                std::iter::once(id).chain(ui.iter().copied()).collect(),
                None,
            )
        }
        Err(e) => (ui.to_vec(), Some(e)),
    }
}
