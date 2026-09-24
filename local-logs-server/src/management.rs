//! Bounded inventories and management of explicitly finalized RLOGGER files.
use crate::protocol::{Error, Result};
use rlogger::storage::{self, FileState, LogName};
use serde::Serialize;
#[cfg(unix)]
use std::os::unix::fs::MetadataExt;
use std::{
    collections::HashMap,
    fs::{self, File, Metadata},
    io::Read,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
pub fn now() -> i64 {
    chrono::Utc::now().timestamp()
}
pub fn allocated(m: &Metadata) -> Option<String> {
    #[cfg(unix)]
    {
        Some((u128::from(m.blocks()) * 512).to_string())
    }
    #[cfg(not(unix))]
    {
        let _ = m;
        None
    }
}
pub fn file_state(root: &Path, path: &Path, now: i64) -> (String, bool, String) {
    let Ok(directory) = storage::RootDir::open(root) else {
        return (
            "legacy".into(),
            false,
            "Racine indisponible : consultation seule.".into(),
        );
    };
    file_state_rooted(root, &directory, path, now)
}
pub fn managed_reason(
    root: &Path,
    directory: &storage::RootDir,
) -> std::result::Result<(), String> {
    if root.file_name().is_none_or(|name| name != "rlogger") {
        return Err("Racine non nommée rlogger : consultation seule.".into());
    }
    for relative in [
        Path::new(""),
        Path::new(storage::TECH),
        Path::new(".rlogger/format"),
        Path::new(".rlogger/gate"),
    ] {
        let file = directory
            .open_node(relative)
            .map_err(|_| "Métadonnées RLOGGER incomplètes : consultation seule.".to_owned())?;
        let metadata = file
            .metadata()
            .map_err(|_| "Métadonnées RLOGGER indisponibles : consultation seule.".to_owned())?;
        if (relative.as_os_str().is_empty() || relative == Path::new(storage::TECH))
            != metadata.is_dir()
        {
            return Err("Métadonnées RLOGGER invalides : consultation seule.".into());
        }
        #[cfg(unix)]
        if metadata.uid() != unsafe { libc::geteuid() } || metadata.mode() & 0o022 != 0 {
            return Err(
                "Racine RLOGGER non privée ou appartenant à un autre compte : consultation seule."
                    .into(),
            );
        }
        #[cfg(target_os = "macos")]
        if !has_no_extended_acl(&file) {
            return Err("ACL étendue sur la racine RLOGGER : consultation seule.".into());
        }
    }
    let mut marker = String::new();
    directory
        .open_file(Path::new(".rlogger/format"))
        .map_err(|_| "Marqueur RLOGGER invalide : consultation seule.".to_owned())?
        .take(128)
        .read_to_string(&mut marker)
        .map_err(|_| "Marqueur RLOGGER illisible : consultation seule.".to_owned())?;
    if marker != storage::MARKER {
        return Err("Marqueur RLOGGER invalide : consultation seule.".into());
    }
    Ok(())
}
fn private_directory_chain(
    directory: &storage::RootDir,
    relative: &Path,
) -> std::result::Result<(), String> {
    let mut current = PathBuf::new();
    for part in relative.components() {
        if !matches!(part, std::path::Component::Normal(_)) {
            return Err("Chemin RLOGGER invalide : consultation seule.".into());
        }
        current.push(part);
        let handle = directory
            .open_directory(&current)
            .map_err(|_| "Dossier RLOGGER inaccessible : consultation seule.".to_owned())?;
        let metadata = handle
            .metadata()
            .map_err(|_| "Dossier RLOGGER indisponible : consultation seule.".to_owned())?;
        #[cfg(unix)]
        if metadata.uid() != unsafe { libc::geteuid() } || metadata.mode() & 0o022 != 0 {
            return Err(
                "Dossier RLOGGER accessible en écriture à un autre compte : consultation seule."
                    .into(),
            );
        }
        #[cfg(target_os = "macos")]
        if !has_no_extended_acl(&handle) {
            return Err("ACL étendue sur un dossier RLOGGER : consultation seule.".into());
        }
    }
    Ok(())
}
#[cfg(target_os = "macos")]
fn has_no_extended_acl(file: &File) -> bool {
    use std::os::fd::AsRawFd;
    unsafe extern "C" {
        fn acl_get_fd_np(fd: libc::c_int, kind: libc::c_int) -> *mut libc::c_void;
        fn acl_free(acl: *mut libc::c_void) -> libc::c_int;
    }
    const ACL_TYPE_EXTENDED: libc::c_int = 0x00000100;
    let acl = unsafe { acl_get_fd_np(file.as_raw_fd(), ACL_TYPE_EXTENDED) };
    if acl.is_null() {
        return std::io::Error::last_os_error().raw_os_error() == Some(libc::ENOENT);
    }
    unsafe { acl_free(acl) };
    false
}
pub fn file_state_rooted(
    root: &Path,
    directory: &storage::RootDir,
    path: &Path,
    now: i64,
) -> (String, bool, String) {
    if let Err(reason) = managed_reason(root, directory) {
        return ("legacy".into(), false, reason);
    }
    if let Err(reason) = private_directory_chain(directory, path.parent().unwrap_or(Path::new("")))
    {
        return ("legacy".into(), false, reason);
    }
    if !storage::hourly_layout(path) {
        return (
            "legacy".into(),
            false,
            "Ancien format ou racine générique : consultation seule.".into(),
        );
    }
    let Some(name) = path
        .file_name()
        .and_then(|n| n.to_str())
        .and_then(LogName::parse)
    else {
        return ("legacy".into(), false, "Format horaire non reconnu.".into());
    };
    if directory
        .open_file(&Path::new(storage::TECH).join(format!("{}.lock", name.run_id)))
        .is_err()
    {
        return (
            "unknown".into(),
            false,
            "Métadonnées du run indisponibles.".into(),
        );
    }
    let state = match name.state {
        FileState::Active => "active",
        FileState::Closed => "closed",
        FileState::Recovered => "recovered",
    };
    let reason = if name.state == FileState::Active {
        "Fichier actif : écriture non finalisée."
    } else if !name.ended(now) {
        "L’intervalle horaire n’est pas terminé."
    } else {
        ""
    };
    (state.into(), reason.is_empty(), reason.into())
}
pub fn validate(root: &Path, relative: &Path) -> Result<Metadata> {
    let directory = storage::RootDir::open(root)?;
    validate_rooted(&directory, relative)
}
fn validate_rooted(directory: &storage::RootDir, relative: &Path) -> Result<Metadata> {
    Ok(directory.open_node(relative)?.metadata()?)
}
pub fn action_file(root: &Path, relative: &Path, expected: &str, delete: bool) -> Result<File> {
    let directory = storage::RootDir::open(root)?;
    action_file_rooted(root, &directory, relative, expected, delete)
}
pub fn action_file_rooted(
    root: &Path,
    directory: &storage::RootDir,
    relative: &Path,
    expected: &str,
    delete: bool,
) -> Result<File> {
    let stat = validate_rooted(directory, relative)?;
    let (_, allowed, reason) = file_state_rooted(root, directory, relative, now());
    if !allowed {
        return Err(Error::new("FILE_PROTECTED", reason, 409));
    }
    let file = directory.open_file(relative)?;
    if delete {
        file.try_lock()
            .map_err(|_| Error::new("FILE_BUSY", "Fichier occupé par un téléchargement.", 409))?;
    } else {
        file.try_lock_shared()
            .map_err(|_| Error::new("FILE_BUSY", "Fichier occupé.", 409))?;
    }
    let opened = file.metadata()?;
    if storage::token(&stat) != expected
        || storage::token(&opened) != expected
        || !storage::same_file(&opened, &validate_rooted(directory, relative)?)
    {
        return Err(Error::new(
            "FILE_CHANGED",
            "Le fichier a changé : actualisez avant de réessayer.",
            409,
        ));
    }
    Ok(file)
}
#[derive(Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Size {
    pub content: String,
    pub allocated: Option<String>,
    pub partial: bool,
}
#[derive(Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Inventory {
    pub items: HashMap<String, Size>,
    pub sampled_at: String,
    pub partial: bool,
    pub busy: bool,
    pub notices: Vec<String>,
}
#[derive(Default)]
pub struct Hub(Mutex<HashMap<(PathBuf, bool), Weak<Job>>>);
impl Hub {
    pub fn get(&self, path: PathBuf) -> Arc<Job> {
        self.get_with_maintenance(path, true)
    }
    pub fn get_with_maintenance(&self, path: PathBuf, maintenance: bool) -> Arc<Job> {
        let mut jobs = self.0.lock().unwrap();
        jobs.retain(|_, j| j.strong_count() > 0);
        if let Some(job) = jobs
            .get(&(path.clone(), maintenance))
            .and_then(Weak::upgrade)
            && job.valid()
        {
            return job;
        }
        let job = Arc::new(Job {
            identity: fs::metadata(&path).ok(),
            directory: storage::RootDir::open(&path).ok(),
            path,
            maintenance,
            snapshot: Mutex::new(Inventory::default()),
            running: AtomicBool::new(false),
            dirty: AtomicBool::new(false),
            last: Mutex::new(None),
            #[cfg(test)]
            scan_runs: std::sync::atomic::AtomicUsize::new(0),
        });
        jobs.insert((job.path.clone(), maintenance), Arc::downgrade(&job));
        job
    }
}
pub struct Job {
    path: PathBuf,
    maintenance: bool,
    identity: Option<Metadata>,
    directory: Option<storage::RootDir>,
    snapshot: Mutex<Inventory>,
    running: AtomicBool,
    dirty: AtomicBool,
    last: Mutex<Option<Instant>>,
    #[cfg(test)]
    scan_runs: std::sync::atomic::AtomicUsize,
}
impl Job {
    fn valid(&self) -> bool {
        self.identity
            .as_ref()
            .zip(fs::symlink_metadata(&self.path).ok().as_ref())
            .is_some_and(|(a, b)| storage::same_file(a, b) && !b.file_type().is_symlink())
    }
    pub fn snapshot(&self) -> Inventory {
        let mut v = self.snapshot.lock().unwrap().clone();
        v.busy = self.running.load(Ordering::Relaxed);
        v
    }
    pub fn kick(self: &Arc<Self>, _force: bool) {
        self.start_scan(false);
    }
    pub fn invalidate(self: &Arc<Self>) {
        self.start_scan(true);
    }
    fn start_scan(self: &Arc<Self>, internal: bool) {
        if !internal
            && self
                .last
                .lock()
                .unwrap()
                .is_some_and(|t| t.elapsed() < Duration::from_secs(60))
        {
            return;
        }
        if self.running.swap(true, Ordering::AcqRel) {
            if internal {
                self.dirty.store(true, Ordering::Release);
            }
            return;
        }
        let job = self.clone();
        tokio::task::spawn_blocking(move || {
            #[cfg(test)]
            job.scan_runs.fetch_add(1, Ordering::Relaxed);
            let valid = job
                .identity
                .as_ref()
                .zip(job.directory.as_ref())
                .is_some_and(|(a, d)| d.metadata().is_ok_and(|b| storage::same_file(a, &b)))
                && job.valid();
            let mut scan = Scan {
                job: &job,
                seen: 0,
                result: Inventory::default(),
                now: now(),
                today: chrono::Local::now().date_naive(),
                managed: job.maintenance
                    && job
                        .directory
                        .as_ref()
                        .is_some_and(|d| managed_reason(&job.path, d).is_ok()),
            };
            if valid {
                scan.directory(Path::new(""), 0);
            } else {
                scan.notice("Racine remplacée : inventaire interrompu.");
            }
            scan.result.sampled_at = chrono::Utc::now().to_rfc3339();
            *job.snapshot.lock().unwrap() = scan.result;
            *job.last.lock().unwrap() = Some(Instant::now());
            job.running.store(false, Ordering::Release);
            if job.dirty.swap(false, Ordering::AcqRel) && Arc::strong_count(&job) > 1 {
                job.invalidate();
            }
        });
    }
}
#[derive(Default)]
struct Sum {
    bytes: u128,
    blocks: HashMap<(u64, u64), u128>,
    partial: bool,
    unavailable: bool,
}
impl Sum {
    fn size(&self) -> Size {
        Size {
            content: self.bytes.to_string(),
            allocated: (!self.unavailable).then(|| self.blocks.values().sum::<u128>().to_string()),
            partial: self.partial,
        }
    }
    fn add(&mut self, other: Self) {
        self.bytes += other.bytes;
        self.blocks.extend(other.blocks);
        self.partial |= other.partial;
        self.unavailable |= other.unavailable;
    }
}
struct Scan<'a> {
    job: &'a Arc<Job>,
    seen: usize,
    result: Inventory,
    now: i64,
    today: chrono::NaiveDate,
    managed: bool,
}
impl Scan<'_> {
    fn notice(&mut self, s: &str) {
        self.result.partial = true;
        if self.result.notices.len() < 16 {
            self.result.notices.push(s.into());
        }
    }
    fn save(&mut self, path: &Path, size: Size) {
        if self.result.items.len() < 10000 || path.as_os_str().is_empty() {
            self.result
                .items
                .insert(path.to_string_lossy().into_owned(), size);
        } else {
            self.notice("Cache de tailles limité à 10 000 entrées.");
        }
    }
    fn directory(&mut self, relative: &Path, depth: usize) -> Sum {
        let mut sum = Sum::default();
        if depth > 64 || self.seen >= 200000 || Arc::strong_count(self.job) <= 1 {
            sum.partial = true;
            self.notice("Inventaire limité ou annulé.");
            return sum;
        }
        let Some(directory) = self.job.directory.as_ref() else {
            sum.partial = true;
            self.notice("Racine indisponible.");
            return sum;
        };
        let before = match validate_rooted(directory, relative) {
            Ok(m) if m.is_dir() => m,
            _ => {
                sum.partial = true;
                self.notice("Dossier inaccessible.");
                return sum;
            }
        };
        let mut entries = match directory.read_directory(relative) {
            Ok(e) => e,
            Err(_) => {
                sum.partial = true;
                self.notice("Dossier inaccessible.");
                return sum;
            }
        };
        while self.seen < 200000 {
            let Some(entry) = entries.next() else {
                break;
            };
            self.seen += 1;
            if Arc::strong_count(self.job) <= 1 {
                sum.partial = true;
                self.notice("Inventaire limité ou annulé.");
                break;
            }
            if self.seen.is_multiple_of(128) {
                std::thread::sleep(Duration::from_millis(1));
            }
            let Ok(entry) = entry else {
                sum.partial = true;
                continue;
            };
            let Ok(name) = entry.into_string() else {
                sum.partial = true;
                continue;
            };
            let mut child = relative.join(&name);
            if child.as_os_str().len() > 4096 {
                sum.partial = true;
                continue;
            }
            let Ok(meta) = validate_rooted(directory, &child) else {
                sum.partial = true;
                continue;
            };
            if meta.is_dir() {
                let sub = self.directory(&child, depth + 1);
                sum.add(sub);
                if self.managed
                    && self.old_day(&child)
                    && let Err(e) = (|| -> Result<()> {
                        managed_reason(&self.job.path, directory)
                            .map_err(|reason| Error::new("FILE_PROTECTED", reason, 409))?;
                        private_directory_chain(directory, &child)
                            .map_err(|reason| Error::new("FILE_PROTECTED", reason, 409))?;
                        let _g = directory.open_file(Path::new(".rlogger/gate"))?;
                        _g.lock()?;
                        if !self.job.valid() {
                            return Err(Error::new("ROOT_CHANGED", "Racine remplacée.", 409));
                        }
                        validate_rooted(directory, &child)?;
                        match directory.remove(&child, true, None) {
                            Ok(()) => {}
                            Err(e)
                                if matches!(
                                    e.kind(),
                                    std::io::ErrorKind::DirectoryNotEmpty
                                        | std::io::ErrorKind::NotFound
                                ) => {}
                            Err(e) => return Err(e.into()),
                        }
                        Ok(())
                    })()
                {
                    self.notice(&format!("Nettoyage différé : {}", e.message));
                }
            } else if meta.is_file() {
                if self.managed
                    && depth == 2
                    && storage::hourly_layout(&child)
                    && let Some(log) = LogName::parse(&name)
                    && log.state == FileState::Active
                    && log.ended(self.now)
                {
                    match self.recover(&child, &log) {
                        Ok(Some(new)) => child = new,
                        Ok(None) => {}
                        Err(e) => self.notice(&format!("Récupération différée : {}", e.message)),
                    }
                }
                let m = match validate_rooted(directory, &child) {
                    Ok(m) => m,
                    Err(_) => {
                        sum.partial = true;
                        continue;
                    }
                };
                let mut file = Sum {
                    bytes: u128::from(m.len()),
                    partial: storage::token(&meta) != storage::token(&m),
                    ..Default::default()
                };
                #[cfg(unix)]
                file.blocks
                    .insert((m.dev(), m.ino()), u128::from(m.blocks()) * 512);
                #[cfg(not(unix))]
                {
                    file.unavailable = true;
                }
                self.save(&child, file.size());
                sum.add(file);
            }
        }
        if self.seen >= 200000 {
            sum.partial = true;
            self.notice("Limite de 200 000 entrées parcourues atteinte.");
        }
        if !validate_rooted(directory, relative)
            .is_ok_and(|m| storage::token(&m) == storage::token(&before))
        {
            sum.partial = true;
        }
        self.result.partial |= sum.partial;
        self.save(relative, sum.size());
        sum
    }
    fn old_day(&self, path: &Path) -> bool {
        path.components()
            .next()
            .and_then(|p| p.as_os_str().to_str())
            .and_then(|s| chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").ok())
            .is_some_and(|d| d < self.today)
    }
    fn recover(&self, path: &Path, log: &LogName) -> Result<Option<PathBuf>> {
        let directory = self
            .job
            .directory
            .as_ref()
            .ok_or_else(|| Error::new("ROOT_CHANGED", "Racine indisponible.", 409))?;
        managed_reason(&self.job.path, directory)
            .map_err(|reason| Error::new("FILE_PROTECTED", reason, 409))?;
        private_directory_chain(directory, path.parent().unwrap_or(Path::new("")))
            .map_err(|reason| Error::new("FILE_PROTECTED", reason, 409))?;
        let _g = directory.open_file(Path::new(".rlogger/gate"))?;
        _g.lock()?;
        if !self.job.valid() {
            return Err(Error::new("ROOT_CHANGED", "Racine remplacée.", 409));
        }
        validate_rooted(directory, path)?;
        let lease =
            directory.open_file(&Path::new(storage::TECH).join(format!("{}.lock", log.run_id)))?;
        match lease.try_lock() {
            Ok(()) => {}
            Err(std::fs::TryLockError::WouldBlock) => return Ok(None),
            Err(std::fs::TryLockError::Error(e)) => return Err(e.into()),
        }
        let name = path
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .replace(".active.log", ".recovered.log");
        let target = path.with_file_name(name);
        directory.rename(path, &target)?;
        Ok(Some(target))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn traversal_stops_at_the_shared_budget_before_fetching_an_extra_entry() {
        let temp = tempfile::tempdir().unwrap();
        for name in ["a.log", "b.log", "c.log"] {
            fs::write(temp.path().join(name), b"x").unwrap();
        }
        let root = fs::canonicalize(temp.path()).unwrap();
        let job = Hub::default().get(root);
        let _session = job.clone();
        let mut scan = Scan {
            job: &job,
            seen: 199999,
            result: Inventory::default(),
            now: now(),
            today: chrono::Local::now().date_naive(),
            managed: false,
        };
        let sum = scan.directory(Path::new(""), 0);
        assert_eq!(scan.seen, 200000);
        assert_eq!(sum.bytes, 1);
        assert!(sum.partial);
        assert!(scan.result.partial);
    }
    #[tokio::test]
    async fn mutations_during_a_scan_coalesce_to_one_followup() {
        let temp = tempfile::tempdir().unwrap();
        for number in 0..5000 {
            fs::write(temp.path().join(format!("{number:05}.log")), b"x").unwrap();
        }
        let job = Hub::default().get(fs::canonicalize(temp.path()).unwrap());
        job.kick(false);
        fs::write(temp.path().join("new.log"), b"new").unwrap();
        for _ in 0..20 {
            job.invalidate();
        }
        tokio::time::timeout(Duration::from_secs(10), async {
            while job.scan_runs.load(Ordering::Relaxed) < 2 || job.snapshot().busy {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(job.scan_runs.load(Ordering::Relaxed), 2);
        assert!(job.snapshot().items.contains_key("new.log"));
        job.kick(true);
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(job.scan_runs.load(Ordering::Relaxed), 2);
    }
}
