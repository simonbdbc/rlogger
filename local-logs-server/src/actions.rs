//! Explicit user actions on ordinary files and directory trees, confined by descriptors.
use crate::protocol::{Error, Result};
use rlogger::storage::{self, RootDir};
use std::os::unix::fs::MetadataExt;
use std::{
    fs::{File, Metadata},
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::io::{AsyncReadExt, AsyncWrite, AsyncWriteExt};

pub struct Action {
    pub file: File,
    pub identity: String,
    // Each ancestor is shared. A directory operation locks its target exclusively,
    // preventing deletion/download of descendants by cooperating companions.
    _ancestors: Vec<File>,
}
fn busy() -> Error {
    Error::new(
        "FILE_BUSY",
        "Fichier ou dossier occupé par une opération.",
        409,
    )
}
fn changed() -> Error {
    Error::new(
        "FILE_CHANGED",
        "Le fichier a changé : actualisez avant de réessayer.",
        409,
    )
}
pub fn open(root: &RootDir, path: &Path, expected: &str, delete: bool) -> Result<Action> {
    if path.as_os_str().is_empty() {
        return Err(Error::new(
            "FORBIDDEN",
            "Choisissez une entrée dans le dossier ouvert.",
            403,
        ));
    }
    // Include the opened root: another session may have opened a subtree as
    // its root while a parent session attempts to remove that entire subtree.
    let root_lock = root.open_directory(Path::new(""))?;
    root_lock.try_lock_shared().map_err(|_| busy())?;
    let mut ancestors = vec![root_lock];
    let mut parent = PathBuf::new();
    for part in path.parent().unwrap_or(Path::new("")).components() {
        parent.push(part);
        let f = root.open_directory(&parent)?;
        f.try_lock_shared().map_err(|_| busy())?;
        ancestors.push(f);
    }
    let file = root.open_node(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() && !metadata.is_dir() {
        return Err(Error::new(
            "FORBIDDEN",
            "Type de fichier non pris en charge.",
            403,
        ));
    }
    if delete || metadata.is_dir() {
        file.try_lock().map_err(|_| busy())?;
    } else {
        file.try_lock_shared().map_err(|_| busy())?;
    }
    if storage::token(&file.metadata()?) != expected
        || storage::token(&root.open_node(path)?.metadata()?) != expected
    {
        return Err(changed());
    }
    Ok(Action {
        file,
        identity: expected.into(),
        _ancestors: ancestors,
    })
}

pub struct Item {
    path: PathBuf,
    metadata: Metadata,
}
pub struct Tree {
    items: Vec<Item>,
}
impl Tree {
    pub fn verify_target(&self, expected: &str) -> Result<()> {
        if storage::token(&self.items[0].metadata) != expected {
            return Err(changed());
        }
        Ok(())
    }
    pub fn collect(root: &RootDir, path: &Path, cancelled: impl Fn() -> bool) -> Result<Self> {
        fn visit(
            root: &RootDir,
            path: &Path,
            depth: usize,
            items: &mut Vec<Item>,
            budget: &mut usize,
            cancelled: &impl Fn() -> bool,
        ) -> Result<()> {
            if cancelled() {
                return Err(Error::new("SESSION_EXPIRED", "Opération annulée.", 409));
            }
            *budget = budget.saturating_add(path.as_os_str().len() + 256);
            if depth > 64 || items.len() >= 200_000 || *budget > 32 * 1024 * 1024 {
                return Err(Error::new(
                    "LIMIT",
                    "Dossier trop volumineux : sélectionnez un sous-dossier.",
                    413,
                ));
            }
            let metadata = root.open_node(path)?.metadata()?;
            if !metadata.is_file() && !metadata.is_dir() {
                return Err(Error::new(
                    "FORBIDDEN",
                    "Le dossier contient une entrée non prise en charge.",
                    403,
                ));
            }
            let is_dir = metadata.is_dir();
            let before = storage::token(&metadata);
            items.push(Item {
                path: path.into(),
                metadata,
            });
            if is_dir {
                for name in root.read_directory(path)? {
                    visit(root, &path.join(name?), depth + 1, items, budget, cancelled)?;
                }
                if storage::token(&root.open_directory(path)?.metadata()?) != before {
                    return Err(changed());
                }
            }
            Ok(())
        }
        let mut items = Vec::new();
        visit(root, path, 0, &mut items, &mut 0, &cancelled)?;
        Ok(Self { items })
    }
    pub fn delete(self, root: &RootDir, cancelled: impl Fn() -> bool) -> Result<()> {
        let target = self.items[0].path.clone();
        // Verify the complete inventory before the first removal. No symlink is
        // followed; hidden files and unknown extensions are included.
        for item in &self.items {
            if cancelled() {
                return Err(Error::new("SESSION_EXPIRED", "Suppression annulée.", 409));
            }
            let file = root.open_node(&item.path)?;
            if item.metadata.is_file() && item.path != target {
                file.try_lock().map_err(|_| busy())?;
            }
            if storage::token(&file.metadata()?) != storage::token(&item.metadata) {
                return Err(changed());
            }
        }
        let mut unlinked = std::collections::HashMap::new();
        for (removed, item) in self.items.into_iter().rev().enumerate() {
            let result = (|| -> Result<()> {
                if cancelled() {
                    return Err(Error::new("SESSION_EXPIRED", "Suppression annulée.", 409));
                }
                let handle = root.open_node(&item.path)?;
                if item.metadata.is_file() && item.path != target {
                    handle.try_lock().map_err(|_| busy())?;
                }
                let current = handle.metadata()?;
                if !storage::same_file(&current, &item.metadata) {
                    return Err(changed());
                }
                // A directory's revision changes when we remove its children.
                let key = (current.dev(), current.ino());
                let expected = if item.metadata.is_dir() {
                    storage::token(&current)
                } else {
                    unlinked
                        .get(&key)
                        .cloned()
                        .unwrap_or_else(|| storage::token(&item.metadata))
                };
                // The target gate is already exclusively locked by this action.
                let gate = if item.path == Path::new(".rlogger/gate") {
                    None
                } else {
                    root.open_file(Path::new(".rlogger/gate")).ok()
                };
                if let Some(gate) = &gate {
                    gate.lock()?;
                }
                root.remove(&item.path, item.metadata.is_dir(), Some(&expected))?;
                if item.metadata.is_file() && item.metadata.nlink() > 1 {
                    // unlink changes ctime on other names of the same inode.
                    unlinked.insert(key, storage::token(&handle.metadata()?));
                }
                Ok(())
            })();
            if let Err(mut error) = result {
                if removed > 0 {
                    error.message = format!(
                        "Suppression partielle ({removed} entrées supprimées). {} Actualisez le dossier.",
                        error.message
                    );
                }
                return Err(error);
            }
        }
        Ok(())
    }
    fn prefix(&self) -> &Path {
        self.items[0].path.parent().unwrap_or(Path::new(""))
    }
    pub fn archive_len(&self) -> Result<u64> {
        let mut total = 1024u64;
        for item in &self.items {
            let header = tar_headers(
                item.path.strip_prefix(self.prefix()).unwrap(),
                &item.metadata,
            )?;
            let size = if item.metadata.is_file() {
                item.metadata.len()
            } else {
                0
            };
            total = total
                .checked_add(header.len() as u64)
                .and_then(|n| n.checked_add(size))
                .and_then(|n| n.checked_add((512 - size % 512) % 512))
                .ok_or_else(|| Error::new("LIMIT", "Archive trop volumineuse.", 413))?;
        }
        Ok(total)
    }
    pub async fn archive<W: AsyncWrite + Unpin>(
        self,
        root: Arc<RootDir>,
        output: &mut W,
    ) -> std::io::Result<()> {
        let prefix = self.prefix().to_owned();
        for item in self.items {
            let source_root = root.clone();
            let path = item.path.clone();
            let expected = storage::token(&item.metadata);
            let file = tokio::task::spawn_blocking(move || -> std::io::Result<File> {
                let f = source_root.open_node(&path)?;
                if storage::token(&f.metadata()?) != expected {
                    return Err(std::io::Error::other("archive source changed"));
                }
                Ok(f)
            })
            .await
            .map_err(std::io::Error::other)??;
            let header = tar_headers(item.path.strip_prefix(&prefix).unwrap(), &item.metadata)
                .map_err(std::io::Error::other)?;
            output.write_all(&header).await?;
            if item.metadata.is_file() {
                let len = item.metadata.len();
                let mut file = tokio::fs::File::from_std(file);
                file.set_max_buf_size(crate::protocol::CHUNK);
                let copied = tokio::io::copy(&mut file.take(len), output).await?;
                if copied != len {
                    return Err(std::io::Error::other("archive source truncated"));
                }
                output
                    .write_all(&[0; 512][..((512 - len % 512) % 512) as usize])
                    .await?;
            }
        }
        output.write_all(&[0; 1024]).await
    }
}

fn tar_headers(path: &Path, metadata: &Metadata) -> Result<Vec<u8>> {
    let mut header = tar::Header::new_gnu();
    header.set_mode(if metadata.is_dir() { 0o755 } else { 0o644 });
    header.set_uid(0);
    header.set_gid(0);
    header.set_mtime(
        metadata
            .modified()
            .ok()
            .and_then(|v| v.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |v| v.as_secs()),
    );
    header.set_entry_type(if metadata.is_dir() {
        tar::EntryType::Directory
    } else {
        tar::EntryType::Regular
    });
    header.set_size(if metadata.is_dir() { 0 } else { metadata.len() });
    let mut bytes = Vec::new();
    if header.set_path(path).is_err() {
        use std::os::unix::ffi::OsStrExt;
        let name = path.as_os_str().as_bytes();
        let mut long = tar::Header::new_gnu();
        long.set_path("././@LongLink")?;
        long.set_entry_type(tar::EntryType::GNULongName);
        long.set_size((name.len() + 1) as u64);
        long.set_mode(0o644);
        long.set_uid(0);
        long.set_gid(0);
        long.set_mtime(0);
        long.set_cksum();
        bytes.extend_from_slice(long.as_bytes());
        bytes.extend_from_slice(name);
        bytes.push(0);
        bytes.resize(bytes.len().div_ceil(512) * 512, 0);
        header.set_path("entry")?;
    }
    header.set_cksum();
    bytes.extend_from_slice(header.as_bytes());
    Ok(bytes)
}
