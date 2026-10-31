//! Release Markdown is presentation data; never show manual install/publishing instructions.
/// Clean headings/list markers/links while retaining only product changes.
/// Adapted from KinNovel's plain_notes, with fenced blocks and instruction filtering.
pub fn plain_notes(notes: &str) -> Vec<(String, bool)> {
    let mut out = Vec::new();
    let mut blocked: Option<usize> = None;
    let mut section = false;
    let mut fenced = false;
    for raw in notes.lines().take(2000) {
        let line = raw.trim();
        if line.starts_with("```") || line.starts_with("~~~") {
            fenced = !fenced;
            continue;
        }
        if fenced
            || line.is_empty()
            || line.starts_with('>')
            || line.starts_with("<!--")
            || line.starts_with("![")
        {
            continue;
        }
        if line.starts_with('#') {
            let level = line.chars().take_while(|c| *c == '#').count();
            if blocked.is_some_and(|b| level > b) {
                continue;
            }
            blocked = None;
            let title = plain(line.trim_start_matches('#').trim());
            if excluded(&title) {
                blocked = Some(level);
                section = false;
                continue;
            }
            section = change_heading(&title);
            if section {
                out.push((title, true));
            }
            continue;
        }
        if blocked.is_some() {
            continue;
        }
        let numbered = line
            .split_once(". ")
            .filter(|(n, _)| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
            .map(|(_, t)| t);
        let item = line
            .strip_prefix("- ")
            .or_else(|| line.strip_prefix("* "))
            .or_else(|| line.strip_prefix("+ "))
            .or(numbered);
        let body = item.unwrap_or(line).trim();
        if excluded(body)
            || body.starts_with("---")
            || body.starts_with("http://")
            || body.starts_with("https://")
        {
            continue;
        }
        if item.is_some() || section || starts_change(body) {
            let text = plain(body);
            if !text.is_empty() {
                out.push((
                    if item.is_some() {
                        format!("· {text}")
                    } else {
                        text
                    },
                    false,
                ));
            }
        }
    }
    // Remove empty category headings (e.g. a heading followed only by an excluded download link).
    let mut clean = vec![];
    for (i, l) in out.iter().enumerate() {
        if !l.1 || out.get(i + 1).is_some_and(|n| !n.1) {
            clean.push(l.clone());
        }
    }
    clean
}
fn excluded(text: &str) -> bool {
    let text = text.trim_start_matches(['*', '`', '[']);
    let lower = text.to_ascii_lowercase();
    [
        "安装",
        "卸载",
        "下载",
        "部署",
        "构建",
        "编译",
        "打包",
        "分发",
        "升级方法",
        "更新方法",
        "使用说明",
        "注意事项",
        "已知问题",
        "已知限制",
        "校验",
        "签名",
        "源码",
        "源代码",
        "仓库",
        "许可证",
        "感谢",
        "致谢",
        "贡献者",
        "手动更新",
        "本地工具链",
        "默认直连",
        "反代由",
        "请参考",
        "详见",
        "本次发布",
    ]
    .iter()
    .any(|p| text.starts_with(p))
        || [
            "sha256",
            "checksum",
            "installation",
            "uninstall",
            "download",
            "build ",
            "build:",
            "deploy",
            "assets",
            "source code",
            "contributors",
            "acknowledg",
            "license",
            "known issues",
            "full changelog",
            "compare:",
        ]
        .iter()
        .any(|p| lower.starts_with(p))
}
fn change_heading(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    [
        "新增",
        "新功能",
        "功能更新",
        "改进",
        "优化",
        "修复",
        "修正",
        "变更",
        "更新内容",
        "更新日志",
        "性能",
        "安全",
    ]
    .iter()
    .any(|p| text.starts_with(p))
        || [
            "added",
            "fixed",
            "changed",
            "improved",
            "improvements",
            "enhancements",
            "features",
            "fixes",
            "bug fixes",
            "what's new",
            "changes",
            "changelog",
            "performance",
            "security",
        ]
        .iter()
        .any(|p| lower.starts_with(p))
}
fn starts_change(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    [
        "新增", "增加", "添加", "修复", "修正", "改进", "优化", "支持", "调整", "移除", "删除",
        "解决",
    ]
    .iter()
    .any(|p| text.starts_with(p))
        || [
            "add ",
            "added ",
            "fix ",
            "fixed ",
            "improve ",
            "improved ",
            "feat:",
            "fix:",
            "perf:",
        ]
        .iter()
        .any(|p| lower.starts_with(p))
}
fn plain(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(i) = rest.find('[') {
        out.push_str(&rest[..i]);
        let label = &rest[i + 1..];
        if let Some(end) = label.find("](") {
            if let Some(close) = label[end + 2..].find(')') {
                out.push_str(&label[..end]);
                rest = &label[end + 2 + close + 1..];
                continue;
            }
        }
        out.push('[');
        rest = label;
    }
    out.push_str(rest);
    out.replace("**", "")
        .replace("__", "")
        .replace("~~", "")
        .replace(['`', '*'], "")
}
