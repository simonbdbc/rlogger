use local_logs_server::{actions, files::FileRoot};
use rlogger::storage::{self, RootDir};
use std::{fs, io::Read, path::Path, sync::Arc};

#[test]
fn ordinary_files_and_active_names_are_actionable_without_a_marker() {
    let temp = tempfile::tempdir().unwrap();
    for name in [
        "test-demo.log",
        ".hidden",
        "document.bin",
        "app-00-1-2-3-h9999999999-s1.active.log",
    ] {
        fs::write(temp.path().join(name), b"demo").unwrap();
    }
    let mut root = FileRoot::create(temp.path().to_str().unwrap()).unwrap();
    let entries = root.entries(&root.node_id.clone(), None).unwrap().entries;
    assert_eq!(entries.len(), 3);
    assert!(entries.iter().all(|entry| !entry.name.starts_with('.')));
    for entry in entries {
        assert!(entry.eligible, "{}: {}", entry.name, entry.reason);
        let path = Path::new(&entry.relative_path);
        let read = actions::open(root.directory(), path, &entry.identity, false).unwrap();
        assert_eq!(
            actions::open(root.directory(), path, &entry.identity, true)
                .err()
                .unwrap()
                .code,
            "FILE_BUSY"
        );
        drop(read);
        let _delete = actions::open(root.directory(), path, &entry.identity, true).unwrap();
        actions::Tree::collect(root.directory(), path, || false)
            .unwrap()
            .delete(root.directory(), || false)
            .unwrap();
        assert!(!temp.path().join(path).exists());
    }
}

#[tokio::test]
async fn archive_preserves_all_extensions_hidden_empty_unicode_and_long_names() {
    let temp = tempfile::tempdir().unwrap();
    let folder = temp.path().join("dossier été");
    fs::create_dir_all(folder.join("empty")).unwrap();
    fs::write(folder.join(".hidden"), "caché").unwrap();
    fs::write(folder.join("data.bin"), [0, 255, 10]).unwrap();
    fs::hard_link(folder.join("data.bin"), folder.join("linked.bin")).unwrap();
    let long = format!("{}.log", "x".repeat(150));
    fs::write(folder.join(&long), "日本語 🦀").unwrap();
    let root = Arc::new(RootDir::open(temp.path()).unwrap());
    let path = Path::new("dossier été");
    let token = storage::token(&fs::metadata(&folder).unwrap());
    let action = actions::open(&root, path, &token, false).unwrap();
    let child = path.join("data.bin");
    let child_token = storage::token(&fs::metadata(temp.path().join(&child)).unwrap());
    assert_eq!(
        actions::open(&root, &child, &child_token, true)
            .err()
            .unwrap()
            .code,
        "FILE_BUSY"
    );
    let tree = actions::Tree::collect(&root, path, || false).unwrap();
    let size = tree.archive_len().unwrap();
    let mut bytes = Vec::new();
    tree.archive(root.clone(), &mut bytes).await.unwrap();
    assert_eq!(bytes.len() as u64, size);
    let mut archive = tar::Archive::new(bytes.as_slice());
    let mut found = std::collections::BTreeMap::new();
    for entry in archive.entries().unwrap() {
        let mut entry = entry.unwrap();
        let name = entry.path().unwrap().into_owned();
        let mut content = Vec::new();
        entry.read_to_end(&mut content).unwrap();
        found.insert(name, content);
    }
    assert_eq!(found[&path.join(".hidden")], "caché".as_bytes());
    assert_eq!(found[&child], [0, 255, 10]);
    assert_eq!(found[&path.join(long)], "日本語 🦀".as_bytes());
    assert!(found.contains_key(&path.join("empty")));
    drop(action);
    let _delete = actions::open(&root, path, &token, true).unwrap();
    actions::Tree::collect(&root, path, || false)
        .unwrap()
        .delete(&root, || false)
        .unwrap();
    assert!(!folder.exists());
}

#[test]
fn descendant_transfer_blocks_parent_delete_and_preflight_refuses_symlink_or_change() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir(temp.path().join("folder")).unwrap();
    fs::write(temp.path().join("folder/a.log"), b"keep").unwrap();
    let root = RootDir::open(temp.path()).unwrap();
    let path = Path::new("folder/a.log");
    let token = storage::token(&fs::metadata(temp.path().join(path)).unwrap());
    let read = actions::open(&root, path, &token, false).unwrap();
    let parent_token = storage::token(&fs::metadata(temp.path().join("folder")).unwrap());
    assert_eq!(
        actions::open(&root, Path::new("folder"), &parent_token, true)
            .err()
            .unwrap()
            .code,
        "FILE_BUSY"
    );
    drop(read);
    let subtree = RootDir::open(&temp.path().join("folder")).unwrap();
    let subtree_read = actions::open(&subtree, Path::new("a.log"), &token, false).unwrap();
    assert_eq!(
        actions::open(&root, Path::new("folder"), &parent_token, true)
            .err()
            .unwrap()
            .code,
        "FILE_BUSY"
    );
    drop(subtree_read);
    fs::write(temp.path().join(path), b"changed").unwrap();
    assert_eq!(
        actions::open(&root, path, &token, true).err().unwrap().code,
        "FILE_CHANGED"
    );
    std::os::unix::fs::symlink(temp.path(), temp.path().join("folder/escape")).unwrap();
    assert!(actions::Tree::collect(&root, Path::new("folder"), || false).is_err());
    assert!(actions::Tree::collect(&root, Path::new("folder"), || true).is_err());
    assert_eq!(fs::read(temp.path().join(path)).unwrap(), b"changed");
    assert!(actions::open(&root, Path::new("../escape"), &token, true).is_err());
}

#[test]
fn explicit_technical_gate_deletion_does_not_lock_it_twice() {
    let temp = tempfile::tempdir().unwrap();
    storage::initialize(temp.path()).unwrap();
    let root = RootDir::open(temp.path()).unwrap();
    let path = Path::new(".rlogger/gate");
    let expected = storage::token(&root.open_file(path).unwrap().metadata().unwrap());
    let _action = actions::open(&root, path, &expected, true).unwrap();
    actions::Tree::collect(&root, path, || false)
        .unwrap()
        .delete(&root, || false)
        .unwrap();
    assert!(!temp.path().join(path).exists());
}
