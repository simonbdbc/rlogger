//! Read-only filesystem access. IDs and generations are confined to one root.
use crate::protocol::*;
use base64::{
    Engine,
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
};
use serde::{Deserialize, Serialize};
#[cfg(unix)]
use std::os::unix::fs::MetadataExt;
use std::{
    collections::{BTreeMap, HashMap, VecDeque},
    fs::{self, File, Metadata},
    io::{Read, Seek, SeekFrom},
    path::{Component, Path, PathBuf},
    sync::Arc,
};

#[derive(Clone)]
struct Node {
    relative: PathBuf,
    kind: Kind,
}
struct Generation {
    identity: String,
    generation: String,
    size: u64,
    anchor: Vec<u8>,
    anchor_offset: u64,
}
#[derive(Serialize, Deserialize)]
struct Cursor {
    root: String,
    parent: String,
    rev: String,
    after: String,
}
pub struct FileRoot {
    pub id: String,
    pub node_id: String,
    absolute: PathBuf,
    identity: String,
    root: Arc<rlogger::storage::RootDir>,
    nodes: HashMap<String, Node>,
    ids: HashMap<PathBuf, String>,
    order: VecDeque<String>,
    generations: HashMap<String, Generation>,
}
fn identity(s: &Metadata) -> String {
    #[cfg(unix)]
    {
        format!("{}:{}:{:?}", s.dev(), s.ino(), s.created().ok())
    }
    #[cfg(not(unix))]
    {
        format!("{:?}", s.created().ok())
    }
}
fn revision(s: &Metadata) -> String {
    #[cfg(unix)]
    {
        format!(
            "{}:{}:{}:{}:{}",
            identity(s),
            s.mtime(),
            s.mtime_nsec(),
            s.ctime(),
            s.ctime_nsec()
        )
    }
    #[cfg(not(unix))]
    {
        format!("{}:{:?}:{}", identity(s), s.modified().ok(), s.len())
    }
}
impl FileRoot {
    pub fn absolute(&self) -> &Path {
        &self.absolute
    }
    pub fn directory(&self) -> &rlogger::storage::RootDir {
        &self.root
    }
    pub fn directory_handle(&self) -> Arc<rlogger::storage::RootDir> {
        self.root.clone()
    }
    pub fn management_path(&self, id: &str) -> Result<PathBuf> {
        let relative = self.node(id, None)?.relative.clone();
        self.validate(&relative)?;
        Ok(relative)
    }
    pub fn entry(&self, id: &str) -> Result<Entry> {
        let node = self.node(id, None)?;
        let (_, stat) = self.validate(&node.relative)?;
        let (state, _, _) = crate::management::file_state_rooted(
            &self.absolute,
            &self.root,
            &node.relative,
            crate::management::now(),
        );
        let check = crate::actions::open(
            &self.root,
            &node.relative,
            &rlogger::storage::token(&stat),
            true,
        );
        let eligible = check.is_ok();
        let reason = check.err().map(|e| e.message).unwrap_or_default();
        Ok(Entry {
            id: id.into(),
            name: node
                .relative
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned(),
            relative_path: node.relative.to_string_lossy().into_owned(),
            kind: node.kind.clone(),
            size: stat.len().to_string(),
            identity: rlogger::storage::token(&stat),
            allocated_size: crate::management::allocated(&stat),
            state,
            eligible,
            reason,
        })
    }
    pub fn relocate(&mut self, id: &str) -> Result<bool> {
        let node = self.node(id, Some(Kind::File))?.clone();
        let Some(name) = node
            .relative
            .file_name()
            .and_then(|s| s.to_str())
            .filter(|s| s.ends_with(".active.log"))
        else {
            return Ok(false);
        };
        if self.validate(&node.relative).is_ok() {
            return Ok(false);
        }
        let Some(generation) = self.generations.get(id) else {
            return Ok(false);
        };
        for suffix in [".log", ".recovered.log"] {
            let next = node
                .relative
                .with_file_name(name.replace(".active.log", suffix));
            if let Ok((_, stat)) = self.validate(&next)
                && identity(&stat) == generation.identity
            {
                self.ids.remove(&node.relative);
                self.ids.insert(next.clone(), id.into());
                self.nodes.get_mut(id).unwrap().relative = next;
                return Ok(true);
            }
        }
        Ok(false)
    }
    pub fn create(input: &str) -> Result<Self> {
        if !Path::new(input).is_absolute() || input.len() > 4096 || input.contains('\0') {
            return Err(Error::new(
                "INVALID_REQUEST",
                "Saisissez un chemin absolu de dossier.",
                400,
            ));
        }
        let absolute = fs::canonicalize(input)?;
        let root = Arc::new(rlogger::storage::RootDir::open(&absolute)?);
        let stat = root.metadata()?;
        if !stat.is_dir() {
            return Err(Error::new(
                "INVALID_REQUEST",
                "Ce chemin désigne un fichier, pas un dossier.",
                400,
            ));
        }
        drop(root.read_directory(Path::new(""))?);
        let node_id = id();
        Ok(Self {
            id: id(),
            node_id: node_id.clone(),
            absolute,
            identity: identity(&stat),
            root,
            nodes: HashMap::from([(
                node_id.clone(),
                Node {
                    relative: PathBuf::new(),
                    kind: Kind::Directory,
                },
            )]),
            ids: HashMap::from([(PathBuf::new(), node_id)]),
            order: VecDeque::new(),
            generations: HashMap::new(),
        })
    }
    pub fn dto(&self) -> RootDto {
        RootDto {
            root_id: self.id.clone(),
            node_id: self.node_id.clone(),
            absolute_path: self.absolute.to_string_lossy().into_owned(),
            managed_reason: crate::management::managed_reason(&self.absolute, &self.root).err(),
        }
    }
    fn node(&self, id: &str, kind: Option<Kind>) -> Result<&Node> {
        let node = self.nodes.get(id).ok_or_else(|| {
            Error::new(
                "NODE_EXPIRED",
                "Entrée expirée : actualisez son dossier.",
                409,
            )
        })?;
        if kind.is_some_and(|k| k != node.kind) {
            return Err(Error::new(
                "INVALID_REQUEST",
                "Type de fichier incorrect.",
                400,
            ));
        }
        Ok(node)
    }
    pub fn check_node(&self, id: &str, kind: Kind) -> Result<()> {
        self.node(id, Some(kind)).map(|_| ())
    }
    pub fn cached_nodes(&self) -> usize {
        self.nodes.len()
    }
    fn remember(&mut self, relative: PathBuf, kind: Kind) -> String {
        if let Some(id) = self.ids.get(&relative) {
            let node = self.nodes.get_mut(id).expect("ID index matches nodes");
            if node.kind != kind {
                node.kind = kind;
                self.generations.remove(id);
            }
            return id.clone();
        }
        while self.nodes.len() >= NODES {
            if let Some(id) = self.order.pop_front() {
                if let Some(node) = self.nodes.remove(&id) {
                    self.ids.remove(&node.relative);
                }
                self.generations.remove(&id);
            } else {
                break;
            }
        }
        let id = id();
        self.ids.insert(relative.clone(), id.clone());
        self.nodes.insert(id.clone(), Node { relative, kind });
        self.order.push_back(id.clone());
        id
    }
    fn validate(&self, relative: &Path) -> Result<(PathBuf, Metadata)> {
        let stat = fs::symlink_metadata(&self.absolute)?;
        if stat.file_type().is_symlink() || identity(&stat) != self.identity {
            return Err(Error::new(
                "ROOT_CHANGED",
                "Le dossier racine a été remplacé.",
                409,
            ));
        }
        if relative.components().count() > 64 || relative.as_os_str().len() > 4096 {
            return Err(Error::new(
                "LIMIT",
                "Profondeur ou chemin maximal dépassé.",
                400,
            ));
        }
        for part in relative.components() {
            if !matches!(part, Component::Normal(_)) {
                return Err(Error::new("FORBIDDEN", "Chemin hors racine.", 403));
            }
        }
        let node = self.root.open_node(relative).map_err(|error| {
            if matches!(
                error.raw_os_error(),
                Some(libc::ELOOP) | Some(libc::ENOTDIR)
            ) {
                Error::new(
                    "FORBIDDEN",
                    "Les liens symboliques ne sont pas suivis.",
                    403,
                )
            } else {
                error.into()
            }
        })?;
        Ok((self.absolute.join(relative), node.metadata()?))
    }
    pub fn directory_revision(&self, id: &str) -> Result<String> {
        Ok(revision(
            &self
                .validate(&self.node(id, Some(Kind::Directory))?.relative)?
                .1,
        ))
    }
    pub fn entries(&mut self, parent_id: &str, cursor: Option<&str>) -> Result<EntryPage> {
        for id in self.generations.keys().cloned().collect::<Vec<_>>() {
            self.relocate(&id)?;
        }
        let relative = self
            .node(parent_id, Some(Kind::Directory))?
            .relative
            .clone();
        let (_, stat) = self.validate(&relative)?;
        let rev = revision(&stat);
        let after = if let Some(encoded) = cursor {
            let parsed = URL_SAFE_NO_PAD
                .decode(encoded)
                .ok()
                .and_then(|bytes| serde_json::from_slice::<Cursor>(&bytes).ok());
            match parsed {
                Some(c) if c.root == self.id && c.parent == parent_id && c.rev == rev => c.after,
                _ => {
                    return Err(Error::new(
                        "CURSOR_INVALID",
                        "Le dossier a changé : rechargez cette branche.",
                        409,
                    ));
                }
            }
        } else {
            String::new()
        };
        let mut selected = BTreeMap::new();
        for (seen, entry) in self.root.read_directory(&relative)?.enumerate() {
            if seen >= 200_000 {
                return Err(Error::new(
                    "LIMIT",
                    "Ce dossier dépasse la limite de 200 000 entrées.",
                    400,
                ));
            }
            let Ok(name) = entry?.into_string() else {
                continue;
            };
            if name.starts_with('.') || name <= after {
                continue;
            }
            let child = relative.join(&name);
            let ty = match self.root.open_node(&child).and_then(|file| file.metadata()) {
                Ok(ty) => ty,
                Err(_) => continue,
            };
            let kind = if ty.is_dir() {
                Kind::Directory
            } else if ty.is_file() {
                Kind::File
            } else {
                continue;
            };
            selected.insert(name, kind);
            if selected.len() > PAGE + 1 {
                selected.pop_last();
            }
        }
        let has_next = selected.len() > PAGE;
        let mut entries = Vec::with_capacity(PAGE);
        for (name, kind) in selected.into_iter().take(PAGE) {
            let child = relative.join(&name);
            if child.as_os_str().len() > 4096 {
                continue;
            }
            let (_, stat) = self.validate(&child)?;
            let id = self.remember(child.clone(), kind.clone());
            let _ = stat;
            entries.push(self.entry(&id)?);
        }
        if revision(&self.validate(&relative)?.1) != rev {
            return Err(Error::new(
                "CURSOR_INVALID",
                "Le dossier a changé pendant la liste : réessayez.",
                409,
            ));
        }
        let cursor = if has_next {
            entries.last().map(|last| {
                URL_SAFE_NO_PAD.encode(
                    serde_json::to_vec(&Cursor {
                        root: self.id.clone(),
                        parent: parent_id.into(),
                        rev: rev.clone(),
                        after: last.name.clone(),
                    })
                    .expect("cursor serializes"),
                )
            })
        } else {
            None
        };
        Ok(EntryPage {
            entries,
            revision: rev,
            cursor,
        })
    }
    fn safe_open(&self, id: &str) -> Result<(File, Metadata)> {
        let relative = &self.node(id, Some(Kind::File))?.relative;
        let (_, before) = self.validate(relative)?;
        if !before.is_file() {
            return Err(Error::new(
                "INVALID_REQUEST",
                "Ce n’est plus un fichier régulier.",
                400,
            ));
        }
        let handle = self.root.open_file(relative)?;
        let stat = handle.metadata()?;
        let after = self.validate(relative)?.1;
        if !stat.is_file()
            || identity(&stat) != identity(&before)
            || identity(&stat) != identity(&after)
        {
            return Err(Error::new(
                "GENERATION_CHANGED",
                "Le fichier a été remplacé pendant son ouverture.",
                409,
            ));
        }
        Ok((handle, stat))
    }
    fn generation(&mut self, id: &str, handle: &mut File, stat: &Metadata) -> Result<String> {
        let mut changed = true;
        if let Some(prior) = self.generations.get(id) {
            changed = prior.identity != identity(stat) || stat.len() < prior.size;
            if !changed && !prior.anchor.is_empty() {
                changed = read_at(handle, prior.anchor_offset, prior.anchor.len())? != prior.anchor;
            }
        }
        if changed {
            self.generations.insert(
                id.into(),
                Generation {
                    identity: identity(stat),
                    generation: crate::protocol::id(),
                    size: stat.len(),
                    anchor: vec![],
                    anchor_offset: 0,
                },
            );
        }
        let value = self
            .generations
            .get_mut(id)
            .expect("generation initialized");
        value.size = stat.len();
        value.anchor_offset = stat.len().saturating_sub(64);
        value.anchor = read_at(
            handle,
            value.anchor_offset,
            (stat.len() - value.anchor_offset) as usize,
        )?;
        Ok(value.generation.clone())
    }
    pub fn content(&mut self, id: &str, options: ContentOptions) -> Result<Chunk> {
        let (mut handle, stat) = self.safe_open(id)?;
        if stat.len() > MAX_OFFSET {
            return Err(Error::new(
                "LIMIT",
                "Fichier supérieur à 2^53−1 octets.",
                400,
            ));
        }
        let generation = self.generation(id, &mut handle, &stat)?;
        if options.generation.is_some_and(|old| old != generation) {
            return Err(Error::new(
                "GENERATION_CHANGED",
                "Le fichier a changé de génération.",
                409,
            ));
        }
        let limit = options.limit.unwrap_or(SNAPSHOT).min(SNAPSHOT) as u64;
        let (start, end) = if let Some(after) = options.after {
            let start = offset(&after)?;
            if start > stat.len() {
                return Err(Error::new(
                    "GENERATION_CHANGED",
                    "Le fichier a été tronqué.",
                    409,
                ));
            }
            (start, (start + limit).min(stat.len()))
        } else {
            let end = options
                .before
                .as_deref()
                .map(offset)
                .transpose()?
                .unwrap_or(stat.len());
            if end > stat.len() {
                return Err(Error::new(
                    "CURSOR_INVALID",
                    "Position historique invalide.",
                    409,
                ));
            }
            (end.saturating_sub(limit), end)
        };
        let data = read_at(&mut handle, start, (end - start) as usize)?;
        let after = handle.metadata()?;
        let checked = self.validate(&self.node(id, None)?.relative)?.1;
        if identity(&stat) != identity(&after)
            || after.len() < stat.len()
            || data.len() as u64 != end - start
            || identity(&after) != identity(&checked)
        {
            return Err(Error::new(
                "GENERATION_CHANGED",
                "Modification pendant la lecture.",
                409,
            ));
        }
        Ok(Chunk {
            generation,
            start: start.to_string(),
            end: end.to_string(),
            size: stat.len().to_string(),
            data: STANDARD.encode(data),
            partial_start: start > 0,
        })
    }
}
fn read_at(file: &mut File, start: u64, len: usize) -> Result<Vec<u8>> {
    file.seek(SeekFrom::Start(start))?;
    let mut data = vec![0; len];
    let mut read = 0;
    while read < len {
        let n = file.read(&mut data[read..])?;
        if n == 0 {
            break;
        }
        read += n;
    }
    data.truncate(read);
    Ok(data)
}
