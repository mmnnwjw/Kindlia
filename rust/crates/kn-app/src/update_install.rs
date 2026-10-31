//! Transactional installer adapted from KinNovel v1.6.8 (stage/swap/done journal).
use std::io::Read;
use std::path::{Path, PathBuf};
pub fn install(bytes: &[u8], version: &str, dir: &Path) -> Result<usize, String> {
    if dir != Path::new("/mnt/us/extensions/kindlia") && cfg!(target_os = "linux") {
        return Err("更新只能安装到 Kindlia 程序目录".into());
    }
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| e.to_string())?;
    let mut files = vec![];
    let mut seen = std::collections::HashSet::new();
    let mut total = 0u64;
    for i in 0..zip.len() {
        let mut f = zip.by_index(i).map_err(|e| e.to_string())?;
        if f.is_dir() {
            continue;
        }
        let rel = f
            .name()
            .strip_prefix("kindlia/")
            .ok_or("安装包目录无效")?
            .to_owned();
        if !allowed(&rel)
            || !seen.insert(rel.clone())
            || f.unix_mode().is_some_and(|m| m & 0o170000 == 0o120000)
        {
            return Err("安装包含非运行文件、重复路径或链接".into());
        }
        total = total.checked_add(f.size()).ok_or("安装包过大")?;
        if total > 48 * 1024 * 1024 {
            return Err("安装包过大".into());
        }
        let mut data = vec![];
        f.read_to_end(&mut data).map_err(|e| e.to_string())?;
        files.push((rel, data));
    }
    let get = |name: &str| files.iter().find(|(n, _)| n == name).map(|(_, b)| b);
    for n in [
        "bin/kindlia",
        "bin/kindlia.sh",
        "config.xml",
        "menu.json",
        "LICENSE",
    ] {
        if get(n).is_none() {
            return Err(format!("安装包缺少 {n}"));
        }
    }
    if !String::from_utf8_lossy(get("config.xml").unwrap())
        .contains(&format!("<version>{version}</version>"))
    {
        return Err("安装包版本不符".into());
    }
    let elf = get("bin/kindlia").unwrap();
    if elf.len() < 52 || &elf[..6] != b"\x7fELF\x01\x01" || elf[18] != 40 || elf[19] != 0 {
        return Err("安装包不是 Kindle ARM 程序".into());
    }
    let backup = dir.join("backups").join(format!(
        "update-{}-{}",
        crate::stats::unix_now(),
        std::process::id()
    ));
    for (rel, _) in &files {
        let old = dir.join(rel);
        if old.is_file() {
            let to = backup.join("program").join(rel);
            std::fs::create_dir_all(to.parent().unwrap()).map_err(|e| e.to_string())?;
            std::fs::copy(old, to).map_err(|e| e.to_string())?;
        }
    }
    if dir.join("config.toml").exists() {
        std::fs::copy(dir.join("config.toml"), backup.join("config.toml"))
            .map_err(|e| e.to_string())?;
    }
    replace_files(&files, dir, Fault::None)
}
fn allowed(s: &str) -> bool {
    matches!(
        s,
        "bin/kindlia"
            | "bin/kindlia.sh"
            | "config.xml"
            | "menu.json"
            | "config.example.toml"
            | "LICENSE"
            | "THIRD-PARTY-NOTICES.md"
            | "licenses/UPSTREAM-THIRD-PARTY-NOTICES.md"
            | "licenses/LICENSE-rime-pinyin-simp"
            | "licenses/libwebp-COPYING"
            | "licenses/libwebp-PATENTS"
    )
}
const JOURNAL: &str = ".kindlia-install";
const NEW_SUFFIX: &str = ".kindlia-new";
const OLD_SUFFIX: &str = ".kindlia-old";

/// 替换进行到哪一步 (写在安装记录里, 中断后下次启动据此收拾)
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Phase {
    /// 正在写 `.kindlia-new` / 备份 `.kindlia-old`: 目标文件都还没动
    Stage,
    /// 正在把 `.kindlia-new` 换上去: 备份都已完整落盘
    Swap,
    /// 全部换好了, 只剩删备份
    Done,
}

impl Phase {
    fn name(self) -> &'static str {
        match self {
            Phase::Stage => "stage",
            Phase::Swap => "swap",
            Phase::Done => "done",
        }
    }
}

/// 测试用的故障注入: 第 n 次替换时出错 (照常回滚) / 直接停下 (模拟掉电, 什么都不收拾)
#[derive(Clone, Copy, PartialEq, Eq)]
#[cfg_attr(not(test), allow(dead_code))]
enum Fault {
    None,
    FailAt(usize),
    CrashAt(usize),
}

fn journal_path_ok(rel: &str) -> bool {
    allowed(rel)
        && !rel.is_empty()
        && !rel.starts_with('/')
        && !rel.contains('\\')
        && !rel.chars().any(char::is_control)
        && !rel
            .split('/')
            .any(|p| p.is_empty() || p == "." || p == "..")
        && !rel.ends_with(NEW_SUFFIX)
        && !rel.ends_with(OLD_SUFFIX)
        && rel != JOURNAL
}

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut s = path.as_os_str().to_os_string();
    s.push(suffix);
    PathBuf::from(s)
}

/// 写文件并落盘 (fsync)。
fn write_synced(path: &Path, data: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let mut f = std::fs::File::create(path)?;
    f.write_all(data)?;
    f.sync_all()
}

/// 目录项落盘 (改名 / 删除之后); vfat 上也支持, 失败不算错。
fn sync_dir(dir: &Path) {
    #[cfg(unix)]
    if let Ok(d) = std::fs::File::open(dir) {
        let _ = d.sync_all();
    }
    #[cfg(not(unix))]
    let _ = dir;
}

fn write_journal(dir: &Path, phase: Phase, entries: &[(String, bool)]) -> Result<(), String> {
    let mut text = format!("{}\n", phase.name());
    for (rel, existed) in entries {
        text.push_str(if *existed { "+ " } else { "- " });
        text.push_str(rel);
        text.push('\n');
    }
    let path = dir.join(JOURNAL);
    let tmp = with_suffix(&path, NEW_SUFFIX);
    write_synced(&tmp, text.as_bytes())
        .and_then(|_| std::fs::rename(&tmp, &path))
        .map_err(|e| format!("{}: {e}", path.display()))?;
    sync_dir(dir);
    Ok(())
}

fn read_journal(dir: &Path) -> Option<(Phase, Vec<(String, bool)>)> {
    let text = std::fs::read_to_string(dir.join(JOURNAL)).ok()?;
    let mut lines = text.lines();
    let phase = match lines.next()? {
        "stage" => Phase::Stage,
        "swap" => Phase::Swap,
        "done" => Phase::Done,
        _ => return None,
    };
    let entries = lines
        .filter_map(|line| {
            let (flag, rel) = line.split_at_checked(2)?;
            let existed = match flag {
                "+ " => true,
                "- " => false,
                _ => return None,
            };
            journal_path_ok(rel).then(|| (rel.to_string(), existed))
        })
        .collect();
    Some((phase, entries))
}

/// 替换程序文件, 任何时刻掉电都能恢复, `bin/kindlia` 和 `bin/kindlia.sh` 一直在:
/// 1. 记下要动哪些文件 (阶段 stage), 全部写成 `*.kindlia-new` 并 fsync; 原来就有的文件复制一份 `*.kindlia-old` 并 fsync;
/// 2. 阶段改为 swap, 逐个把 `*.kindlia-new` 改名盖到目标上 (说明 / 许可等在前, 程序和启动脚本最后);
/// 3. 阶段改为 done, 删掉备份和安装记录。
///
/// 中途出错: 已换上的改回 `.kindlia-old`、新增的删掉, 程序目录与安装前逐字节相同。中途掉电: 下次启动时
/// [`recover_install`] 照同样的办法回到旧版本。
fn replace_files(files: &[(String, Vec<u8>)], dir: &Path, fault: Fault) -> Result<usize, String> {
    recover_install(dir);
    if dir.join(JOURNAL).exists() {
        return Err("上次安装没能恢复, 请重启后再试".to_string());
    }
    let mut order: Vec<&(String, Vec<u8>)> = files.iter().collect();
    let rank = |rel: &str| match rel {
        "bin/kindlia" => 1,
        "bin/kindlia.sh" => 2,
        _ => 0,
    };
    order.sort_by_key(|(rel, _)| rank(rel));
    let entries: Vec<(String, bool)> = order
        .iter()
        .map(|(rel, _)| (rel.clone(), dir.join(rel).exists()))
        .collect();
    write_journal(dir, Phase::Stage, &entries)?;
    let result = (|| {
        for (rel, data) in &order {
            let target = dir.join(rel);
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("{}: {e}", parent.display()))?;
            }
            let tmp = with_suffix(&target, NEW_SUFFIX);
            write_synced(&tmp, data).map_err(|e| format!("{}: {e}", tmp.display()))?;
        }
        for (rel, existed) in &entries {
            if *existed {
                let target = dir.join(rel);
                let old = with_suffix(&target, OLD_SUFFIX);
                // 复制而不是改名: 换新的时目标一直在
                std::fs::copy(&target, &old)
                    .and_then(|_| {
                        std::fs::OpenOptions::new()
                            .write(true)
                            .open(&old)?
                            .sync_all()
                    })
                    .map_err(|e| format!("{}: {e}", old.display()))?;
            }
        }
        write_journal(dir, Phase::Swap, &entries)?;
        for (i, (rel, _)) in entries.iter().enumerate() {
            match fault {
                Fault::FailAt(n) if n == i => return Err("测试: 替换失败".to_string()),
                Fault::CrashAt(n) if n == i => return Err("测试: 掉电".to_string()),
                _ => {}
            }
            let target = dir.join(rel);
            std::fs::rename(with_suffix(&target, NEW_SUFFIX), &target)
                .map_err(|e| format!("{}: {e}", target.display()))?;
        }
        for (rel, _) in &entries {
            if let Some(parent) = dir.join(rel).parent() {
                sync_dir(parent);
            }
        }
        write_journal(dir, Phase::Done, &entries)
    })();
    match result {
        Err(e) if matches!(fault, Fault::CrashAt(_)) => Err(e),
        Err(e) => {
            eprintln!("[update] 安装失败, 恢复旧版本: {e}");
            recover_install(dir);
            Err(e)
        }
        Ok(()) => {
            recover_install(dir);
            Ok(entries.len())
        }
    }
}

/// 收拾上次没做完的安装 (启动时、每次安装前调用): stage 阶段只删临时文件; swap 阶段把已换上的文件
/// 改回备份、删掉新增的文件, 回到旧版本 (新版本可以再装一次); done 阶段只删备份。没有安装记录时什么都不做。
pub fn recover_install(dir: &Path) {
    let Some((phase, entries)) = read_journal(dir) else {
        let _ = std::fs::remove_file(dir.join(JOURNAL));
        let _ = std::fs::remove_file(with_suffix(&dir.join(JOURNAL), NEW_SUFFIX));
        return;
    };
    if phase != Phase::Done {
        eprintln!("[update] 上次安装没有完成 ({}), 恢复旧版本", phase.name());
    }
    // 有文件改不回去时留着它的备份和安装记录, 下次启动再试
    let mut stuck = false;
    for (rel, existed) in &entries {
        let target = dir.join(rel);
        let (new, old) = (
            with_suffix(&target, NEW_SUFFIX),
            with_suffix(&target, OLD_SUFFIX),
        );
        // `.kindlia-new` 还在 = 这个文件还没换上
        let swapped = !new.exists();
        if phase == Phase::Swap && swapped {
            if *existed {
                if old.exists() {
                    if let Err(e) = std::fs::rename(&old, &target) {
                        eprintln!("[update] 无法恢复 {}: {e}", target.display());
                        stuck = true;
                        continue;
                    }
                }
            } else {
                let _ = std::fs::remove_file(&target);
            }
        }
        let _ = std::fs::remove_file(&new);
        let _ = std::fs::remove_file(&old);
    }
    for (rel, _) in &entries {
        if let Some(parent) = dir.join(rel).parent() {
            sync_dir(parent);
        }
    }
    if !stuck {
        let _ = std::fs::remove_file(dir.join(JOURNAL));
        sync_dir(dir);
    }
}
