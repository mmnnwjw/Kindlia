//! One-way relocation with an on-device journal and rollback on ordinary I/O failures.
use serde::Serialize;
use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
#[derive(Serialize)]
struct Move {
    from: PathBuf,
    to: PathBuf,
}
struct Transaction {
    moves: Vec<Move>,
    journal: PathBuf,
    conflicts: PathBuf,
}
impl Transaction {
    fn relocate(&mut self, from: &Path, to: &Path) -> Result<(), String> {
        if let Some(parent) = to.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        // Record intent before mutation, so power loss leaves a reviewable recovery trail.
        self.moves.push(Move {
            from: from.to_owned(),
            to: to.to_owned(),
        });
        crate::store::atomic_write(
            &self.journal,
            &serde_json::to_vec_pretty(&self.moves).map_err(|e| e.to_string())?,
            true,
        )
        .map_err(|e| e.to_string())?;
        if let Err(e) = fs::rename(from, to) {
            self.moves.pop();
            return Err(format!("迁移 {}: {e}", from.display()));
        }
        Ok(())
    }
    fn merge(&mut self, source: &Path, target: &Path, relative: &Path) -> Result<(), String> {
        let mut entries = fs::read_dir(source)
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            let from = entry.path();
            let to = target.join(entry.file_name());
            let rel = relative.join(entry.file_name());
            let kind = entry.file_type().map_err(|e| e.to_string())?;
            if kind.is_symlink() {
                return Err(format!("迁移目录含符号链接：{}", from.display()));
            }
            if !to.exists() {
                self.relocate(&from, &to)?;
            } else if kind.is_dir() && to.is_dir() {
                self.merge(&from, &to, &rel)?;
            } else if rel == Path::new("config.toml") || rel.starts_with("data") {
                self.relocate(&to, &self.conflicts.join("previous-target").join(&rel))?;
                self.relocate(&from, &to)?;
            } else {
                self.relocate(&from, &self.conflicts.join("previous-source").join(&rel))?;
            }
        }
        Ok(())
    }
    fn rollback(&self) {
        for item in self.moves.iter().rev() {
            if item.to.exists() && !item.from.exists() {
                if let Some(parent) = item.from.parent() {
                    let _ = fs::create_dir_all(parent);
                }
                if let Err(e) = fs::rename(&item.to, &item.from) {
                    eprintln!("[migration] rollback {}: {e}", item.from.display());
                }
            }
        }
    }
}
fn remove_empty(root: &Path) -> Result<(), String> {
    if !root.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(root).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        if entry.file_type().map_err(|e| e.to_string())?.is_dir() {
            remove_empty(&entry.path())?;
        }
    }
    fs::remove_dir(root).map_err(|e| e.to_string())
}
pub fn migrate(source: &Path, target: &Path) -> Result<usize, String> {
    if !source.exists() {
        return Ok(0);
    }
    fs::create_dir_all(target).map_err(|e| e.to_string())?;
    let source = source.canonicalize().map_err(|e| e.to_string())?;
    let target = target.canonicalize().map_err(|e| e.to_string())?;
    if source == target || source.starts_with(&target) || target.starts_with(&source) {
        return Err("迁移源和目标不可互相包含".into());
    }
    let stamp = format!(
        "migration-{}-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos(),
        std::process::id()
    );
    let backup = target.join("backups").join(stamp);
    fs::create_dir_all(&backup).map_err(|e|e.to_string())?;
    let mut tx = Transaction {
        moves: vec![],
        journal: backup.join("moves.json"),
        conflicts: backup.join("conflicts"),
    };
    if let Err(e) = tx.merge(&source, &target, Path::new("")) {
        tx.rollback();
        return Err(e);
    }
    let count = tx.moves.len();
    // Empty-directory cleanup is deliberately after all content has safely moved.
    remove_empty(&source)?;
    crate::store::atomic_write(&backup.join("complete"), b"migration completed\n", true)
        .map_err(|e| e.to_string())?;
    Ok(count)
}
pub fn device(root: &Path) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    if root == Path::new("/mnt/us/extensions/kindlia") {
        let moved = migrate(Path::new("/mnt/us/kindlia"), root)?;
        if moved > 0 {
            eprintln!("[migration] relocated {moved} entries into application directory");
        }
        let old = Path::new("/mnt/us/kindlia-backups");
        if old.exists() {
            let destination = root.join("backups/legacy");
            fs::create_dir_all(destination.parent().unwrap()).map_err(|e| e.to_string())?;
            if destination.exists() {
                migrate(old, &destination)?;
            } else {
                fs::rename(old, destination).map_err(|e| e.to_string())?;
            }
        }
    }
    let _ = root;
    Ok(())
}
