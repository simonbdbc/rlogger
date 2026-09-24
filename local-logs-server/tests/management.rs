use local_logs_server::{
    files::FileRoot,
    management::{self, Hub},
    protocol::ContentOptions,
};
use rlogger::storage::{self, FileState, LogName};
use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};
fn fixture() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("rlogger");
    storage::initialize(&root).unwrap();
    let root = fs::canonicalize(root).unwrap();
    storage::open_file(&storage::lease_path(&root, "1-2-3"), true, true).unwrap();
    let path = PathBuf::from("1970-01-01/test/app-00-1-2-3-h0-s1.log");
    fs::create_dir_all(root.join(path.parent().unwrap())).unwrap();
    fs::write(root.join(&path), "hello\n").unwrap();
    (temp, root, path)
}
async fn inventory(root: &Path) -> management::Inventory {
    let hub = Hub::default();
    let job = hub.get(root.into());
    job.kick(true);
    tokio::time::timeout(Duration::from_secs(5), async {
        while job.snapshot().busy {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    job.snapshot()
}
#[tokio::test]
async fn external_opening_never_runs_automatic_maintenance() {
    let (_temp, root, path) = fixture();
    let active = path.with_file_name(
        path.file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .replace(".log", ".active.log"),
    );
    fs::rename(root.join(&path), root.join(&active)).unwrap();
    let hub = Hub::default();
    let external = hub.get_with_maintenance(root.clone(), false);
    let managed = hub.get(root.clone());
    assert!(!std::sync::Arc::ptr_eq(&external, &managed));
    external.kick(false);
    tokio::time::timeout(Duration::from_secs(5), async {
        while external.snapshot().sampled_at.is_empty() || external.snapshot().busy {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert!(root.join(&active).exists());
    assert!(
        external
            .snapshot()
            .items
            .contains_key(&active.to_string_lossy().to_string())
    );
}
#[test]
fn eligibility_and_download_lock_use_current_identity() {
    let (_temp, root, path) = fixture();
    let token = storage::token(&fs::metadata(root.join(&path)).unwrap());
    assert!(management::file_state(&root, &path, 3600).1);
    assert!(!management::file_state(&root, &path, 3599).1);
    let reading = management::action_file(&root, &path, &token, false).unwrap();
    assert_eq!(
        management::action_file(&root, &path, &token, true)
            .unwrap_err()
            .code,
        "FILE_BUSY"
    );
    drop(reading);
    // Parallel process tests can fork while the shared descriptor exists. Wait
    // for their exec to close inherited CLOEXEC handles before checking release.
    let until = std::time::Instant::now() + Duration::from_secs(2);
    loop {
        match management::action_file(&root, &path, &token, true) {
            Ok(file) => {
                drop(file);
                break;
            }
            Err(error) if error.code == "FILE_BUSY" && std::time::Instant::now() < until => {
                std::thread::sleep(Duration::from_millis(5))
            }
            Err(error) => panic!("lock did not release: {error}"),
        }
    }
    fs::write(root.join(&path), "replacement longer").unwrap();
    assert_eq!(
        management::action_file(&root, &path, &token, true)
            .unwrap_err()
            .code,
        "FILE_CHANGED"
    );
    assert!(!management::file_state(&root, Path::new("old.log"), i64::MAX).1);
    assert!(LogName::parse("app-00-../evil-h0-s1.log").is_none());
}
#[test]
fn planted_marker_and_writable_roots_stay_read_only() {
    use std::os::unix::fs::PermissionsExt;
    let temp = tempfile::tempdir().unwrap();
    let fake = temp.path().join("ordinary");
    storage::initialize(&fake).unwrap();
    let fake_dir = storage::RootDir::open(&fake).unwrap();
    assert!(
        management::managed_reason(&fake, &fake_dir)
            .unwrap_err()
            .contains("rlogger")
    );
    let (_hold, root, path) = fixture();
    let directory = storage::RootDir::open(&root).unwrap();
    assert!(management::managed_reason(&root, &directory).is_ok());
    let original = fs::metadata(&root).unwrap().permissions();
    fs::set_permissions(&root, fs::Permissions::from_mode(0o777)).unwrap();
    let reason = management::managed_reason(&root, &directory).unwrap_err();
    assert!(reason.contains("non privée"));
    assert!(!management::file_state_rooted(&root, &directory, &path, i64::MAX).1);
    fs::set_permissions(&root, original).unwrap();
    let marker = root.join(".rlogger/format");
    let permissions = fs::metadata(&marker).unwrap().permissions();
    fs::set_permissions(&marker, fs::Permissions::from_mode(0o666)).unwrap();
    assert!(management::managed_reason(&root, &directory).is_err());
    fs::set_permissions(&marker, permissions).unwrap();
    assert!(management::file_state_rooted(&root, &directory, &path, i64::MAX).1);
    let parent = root.join(path.parent().unwrap());
    let permissions = fs::metadata(&parent).unwrap().permissions();
    fs::set_permissions(&parent, fs::Permissions::from_mode(0o777)).unwrap();
    let reason = management::file_state_rooted(&root, &directory, &path, i64::MAX).2;
    assert!(reason.contains("Dossier RLOGGER accessible"));
    fs::set_permissions(&parent, permissions).unwrap();
    assert!(management::file_state_rooted(&root, &directory, &path, i64::MAX).1);
}
#[cfg(target_os = "macos")]
#[test]
fn extended_acl_disables_managed_actions() {
    for relative in ["", ".rlogger", ".rlogger/format", ".rlogger/gate"] {
        let (_hold, root, path) = fixture();
        let status = std::process::Command::new("chmod")
            .arg("+a")
            .arg("everyone allow write")
            .arg(root.join(relative))
            .status()
            .unwrap();
        assert!(status.success());
        let directory = storage::RootDir::open(&root).unwrap();
        assert!(
            management::managed_reason(&root, &directory)
                .unwrap_err()
                .contains("ACL")
        );
        assert!(!management::file_state_rooted(&root, &directory, &path, i64::MAX).1);
    }
}

#[tokio::test]
async fn explicit_refresh_is_limited_per_shared_root() {
    let (_hold, root, _) = fixture();
    let hub = Hub::default();
    let first = hub.get(root.clone());
    let second = hub.get(root);
    assert!(std::sync::Arc::ptr_eq(&first, &second));
    first.kick(false);
    tokio::time::timeout(Duration::from_secs(5), async {
        while first.snapshot().sampled_at.is_empty() || first.snapshot().busy {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let sampled = first.snapshot().sampled_at;
    for _ in 0..20 {
        second.kick(true);
    }
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(first.snapshot().sampled_at, sampled);
    assert!(!first.snapshot().busy);
}
#[tokio::test]
async fn recovery_respects_live_lease_preserves_bytes_and_is_idempotent() {
    let (_temp, root, path) = fixture();
    let active = path.with_file_name(
        path.file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .replace(".log", ".active.log"),
    );
    fs::rename(root.join(&path), root.join(&active)).unwrap();
    fs::write(root.join(&active), "partial 🦀 without newline").unwrap();
    let lease = storage::open_file(&storage::lease_path(&root, "1-2-3"), true, false).unwrap();
    lease.lock().unwrap();
    inventory(&root).await;
    assert!(root.join(&active).exists());
    drop(lease);
    inventory(&root).await;
    let recovered = active.with_file_name(
        active
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .replace(".active.log", ".recovered.log"),
    );
    assert_eq!(
        fs::read_to_string(root.join(&recovered)).unwrap(),
        "partial 🦀 without newline"
    );
    assert!(!root.join(&active).exists());
    inventory(&root).await;
    assert!(root.join(recovered).exists());
}
#[tokio::test]
async fn totals_deduplicate_allocations_and_cleanup_only_empty_old_days() {
    use std::os::unix::fs::MetadataExt;
    let (_temp, root, path) = fixture();
    fs::hard_link(
        root.join(&path),
        root.join(path.parent().unwrap()).join("copy.txt"),
    )
    .unwrap();
    fs::create_dir_all(root.join("2000-01-01/empty/nested")).unwrap();
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    fs::create_dir_all(root.join(&today).join("empty")).unwrap();
    fs::create_dir_all(root.join("2000-01-02/keep")).unwrap();
    fs::write(root.join("2000-01-02/keep/.hidden"), "keep").unwrap();
    let result = inventory(&root).await;
    assert_eq!(
        result.items[""].content,
        (16 + storage::MARKER.len()).to_string()
    );
    assert_eq!(
        result.items[""].allocated.as_deref(),
        Some(
            ((fs::metadata(root.join(&path)).unwrap().blocks()
                + fs::metadata(root.join("2000-01-02/keep/.hidden"))
                    .unwrap()
                    .blocks()
                + fs::metadata(root.join(".rlogger/format")).unwrap().blocks())
                * 512)
                .to_string()
                .as_str()
        )
    );
    assert!(!root.join("2000-01-01").exists());
    assert!(root.join(today).exists());
    assert!(root.join("2000-01-02/keep/.hidden").exists());
    assert!(root.join(storage::TECH).exists());
}
#[test]
fn publication_preserves_selected_identity_and_generation() {
    let (_temp, root, path) = fixture();
    let active = path.with_file_name(
        path.file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .replace(".log", ".active.log"),
    );
    fs::rename(root.join(&path), root.join(&active)).unwrap();
    let parent = root.join(path.parent().unwrap());
    let mut r = FileRoot::create(parent.to_str().unwrap()).unwrap();
    let page = r.entries(&r.node_id.clone(), None).unwrap();
    let id = &page.entries[0].id;
    let chunk = r.content(id, ContentOptions::default()).unwrap();
    storage::rename_beneath(&root, &active, &path).unwrap();
    assert!(r.relocate(id).unwrap());
    let next = r
        .content(
            id,
            ContentOptions {
                generation: Some(chunk.generation.clone()),
                after: Some(chunk.end),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(next.generation, chunk.generation);
    assert_eq!(
        r.entry(id).unwrap().name,
        path.file_name().unwrap().to_str().unwrap()
    );
}
#[tokio::test]
async fn sparse_large_totals_cache_depth_and_permissions_report_partial() {
    use std::os::unix::fs::PermissionsExt;
    let temp = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(temp.path()).unwrap();
    let sparse = root.join("sparse.log");
    let bytes = 1u64 << 40;
    fs::File::create(&sparse).unwrap().set_len(bytes).unwrap();
    for i in 0..10000 {
        fs::hard_link(&sparse, root.join(format!("alias-{i}.log"))).unwrap();
    }
    let result = inventory(&root).await;
    assert_eq!(
        result.items[""].content,
        (u128::from(bytes) * 10001).to_string()
    );
    assert_eq!(
        result.items[""].allocated,
        management::allocated(&fs::metadata(&sparse).unwrap())
    );
    assert!(result.partial);
    assert!(result.items.len() <= 10001);
    let mut nested = root.clone();
    for _ in 0..65 {
        nested.push("deep");
    }
    fs::create_dir_all(&nested).unwrap();
    fs::write(nested.join("hidden.log"), b"beyond depth budget").unwrap();
    let denied = root.join("denied");
    fs::create_dir(&denied).unwrap();
    fs::set_permissions(&denied, fs::Permissions::from_mode(0o0)).unwrap();
    let result = inventory(&root).await;
    fs::set_permissions(&denied, fs::Permissions::from_mode(0o700)).unwrap();
    assert!(result.partial);
    assert_eq!(
        result.items[""].content,
        (u128::from(bytes) * 10001).to_string()
    );
    assert!(
        result
            .notices
            .iter()
            .any(|s| s.contains("limité") || s.contains("inaccessible"))
    );
}
#[tokio::test]
async fn shared_jobs_reject_replaced_root_and_recovery_conflicts_preserve_active() {
    let (_temp, root, path) = fixture();
    let hub = Hub::default();
    let job = hub.get(root.clone());
    assert!(std::sync::Arc::ptr_eq(&job, &hub.get(root.clone())));
    let active = path.with_file_name("app-00-1-2-3-h0-s2.active.log");
    let recovered = path.with_file_name("app-00-1-2-3-h0-s2.recovered.log");
    fs::write(root.join(&active), b"partial").unwrap();
    fs::write(root.join(&recovered), b"existing").unwrap();
    let legacy_active = root.join("legacy/test/app-00-1-2-3-h0-s1.active.log");
    fs::create_dir_all(legacy_active.parent().unwrap()).unwrap();
    fs::write(&legacy_active, b"legacy protected").unwrap();
    let result = inventory(&root).await;
    assert_eq!(fs::read(&legacy_active).unwrap(), b"legacy protected");
    assert!(
        result
            .notices
            .iter()
            .any(|s| s.contains("Récupération différée"))
    );
    assert_eq!(fs::read(root.join(&active)).unwrap(), b"partial");
    assert_eq!(fs::read(root.join(&recovered)).unwrap(), b"existing");
    // Retry after an interrupted/conflicting publication keeps exactly the bytes.
    fs::remove_file(root.join(&recovered)).unwrap();
    inventory(&root).await;
    assert_eq!(fs::read(root.join(&recovered)).unwrap(), b"partial");
    fs::rename(&root, root.with_file_name("previous")).unwrap();
    storage::initialize(&root).unwrap();
    let replacement = root.join("2000-01-01/empty");
    fs::create_dir_all(&replacement).unwrap();
    job.kick(true);
    while job.snapshot().busy {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(job.snapshot().partial);
    assert!(replacement.exists());
    assert!(!std::sync::Arc::ptr_eq(&job, &hub.get(root.clone())));
}
#[test]
fn confined_mutations_refuse_symlink_parents_and_targets() {
    let (temp, root, path) = fixture();
    let outside = temp.path().join("outside");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("keep.log"), "keep").unwrap();
    std::os::unix::fs::symlink(&outside, root.join("escape")).unwrap();
    assert!(storage::remove_beneath(&root, Path::new("escape/keep.log"), false, None).is_err());
    std::os::unix::fs::symlink(outside.join("keep.log"), root.join("link.log")).unwrap();
    assert!(storage::remove_beneath(&root, Path::new("link.log"), false, None).is_err());
    assert!(
        storage::rename_beneath(&root, Path::new("link.log"), Path::new("published.log")).is_err()
    );
    assert!(management::action_file(&root, &path, "wrong", true).is_err());
    assert_eq!(
        fs::read_to_string(outside.join("keep.log")).unwrap(),
        "keep"
    );
}
#[test]
fn naming_distinguishes_recovery_and_repeated_hours() {
    let a = LogName::parse("app-02-1-2-3-h1792886400-s1.log").unwrap();
    let b = LogName::parse("app-02-1-2-3-h1792890000-s2.recovered.log").unwrap();
    assert_ne!(a.hour_start, b.hour_start);
    assert_eq!(b.state, FileState::Recovered);
    assert!(LogName::parse("app-02-1-2-3-h9223372036854775807-s1.log").is_none());
}
#[test]
fn crash_child() {
    use std::io::Write;
    let Ok(path) = std::env::var("RLOGGER_CRASH_CHILD") else {
        return;
    };
    let rt = rlogger::Runtime::new(rlogger::Config::new(path)).unwrap();
    let l = rt.instance(rlogger::InstanceConfig::new("child")).unwrap();
    rlogger::info!(l, "real crash bytes 🦀");
    rt.flush().unwrap();
    println!("READY:{}", rt.run_id());
    std::io::stdout().flush().unwrap();
    loop {
        std::thread::park();
    }
}
#[tokio::test]
async fn killed_real_logger_releases_lease_and_recovers_without_rewriting_content() {
    use std::io::BufRead;
    struct Child(std::process::Child);
    impl Drop for Child {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let temp = tempfile::tempdir().unwrap();
    let mut child = Child(
        std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "crash_child", "--nocapture"])
            .env("RLOGGER_CRASH_CHILD", temp.path())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let mut reader = std::io::BufReader::new(child.0.stdout.take().unwrap());
    let mut line = String::new();
    let run = loop {
        line.clear();
        assert!(reader.read_line(&mut line).unwrap() > 0);
        if let Some(s) = line.trim().strip_prefix("READY:") {
            break s.to_owned();
        }
    };
    let root = fs::canonicalize(temp.path().join("rlogger")).unwrap();
    let lease = storage::open_file(&storage::lease_path(&root, &run), true, false).unwrap();
    assert!(lease.try_lock().is_err());
    child.0.kill().unwrap();
    child.0.wait().unwrap();
    lease.try_lock().unwrap();
    lease.unlock().unwrap();
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let original = fs::read_dir(root.join(today).join("child"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let bytes = fs::read(&original).unwrap();
    // Model an already elapsed hour without changing the machine clock.
    let parent = root.join("1970-01-01/child");
    fs::create_dir_all(&parent).unwrap();
    let active = parent.join(format!("app-00-{run}-h0-s1.active.log"));
    fs::rename(original, &active).unwrap();
    inventory(&root).await;
    let recovered = parent.join(format!("app-00-{run}-h0-s1.recovered.log"));
    assert_eq!(fs::read(recovered).unwrap(), bytes);
    assert!(!active.exists());
}
#[test]
fn two_actual_producer_processes_get_independent_run_leases() {
    use std::io::BufRead;
    struct Children(Vec<std::process::Child>);
    impl Drop for Children {
        fn drop(&mut self) {
            for child in &mut self.0 {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }
    let temp = tempfile::tempdir().unwrap();
    let mut children = Children(Vec::new());
    for _ in 0..2 {
        children.0.push(
            std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "crash_child", "--nocapture"])
                .env("RLOGGER_CRASH_CHILD", temp.path())
                .stdout(std::process::Stdio::piped())
                .spawn()
                .unwrap(),
        );
    }
    let mut ids = vec![];
    for child in &mut children.0 {
        let mut reader = std::io::BufReader::new(child.stdout.take().unwrap());
        loop {
            let mut line = String::new();
            assert!(reader.read_line(&mut line).unwrap() > 0);
            if let Some(id) = line.trim().strip_prefix("READY:") {
                ids.push(id.to_owned());
                break;
            }
        }
    }
    assert_ne!(ids[0], ids[1]);
    let root = temp.path().join("rlogger");
    let first = fs::File::open(storage::lease_path(&root, &ids[0])).unwrap();
    let second = fs::File::open(storage::lease_path(&root, &ids[1])).unwrap();
    assert!(first.try_lock().is_err());
    assert!(second.try_lock().is_err());
    children.0[0].kill().unwrap();
    children.0[0].wait().unwrap();
    first.try_lock().unwrap();
    assert!(second.try_lock().is_err());
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    assert_eq!(
        fs::read_dir(root.join(today).join("child"))
            .unwrap()
            .count(),
        2
    );
}
