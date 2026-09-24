use base64::{Engine, engine::general_purpose::STANDARD};
use local_logs_server::{files::FileRoot, protocol::*};
use std::{fs, io::Write};
fn append(path: impl AsRef<std::path::Path>, data: &[u8]) {
    fs::OpenOptions::new()
        .append(true)
        .open(path)
        .unwrap()
        .write_all(data)
        .unwrap();
}
fn file(root: &mut FileRoot, name: &str) -> String {
    let node = root.node_id.clone();
    root.entries(&node, None)
        .unwrap()
        .entries
        .into_iter()
        .find(|e| e.name == name)
        .unwrap()
        .id
}
#[test]
fn root_ids_unicode_symlinks_and_permissions() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path();
    fs::create_dir(path.join("日 espaces")).unwrap();
    fs::write(path.join("a.log"), "été 🦀\r\n").unwrap();
    fs::write(path.join("ignored.bin"), "ignored").unwrap();
    fs::write(path.join(".hidden.log"), "hidden").unwrap();
    fs::create_dir(path.join(".rlogger")).unwrap();
    fs::create_dir(path.join("日 espaces/.hidden-dir")).unwrap();
    fs::write(path.join("日 espaces/.hidden"), "hidden").unwrap();
    fs::write(path.join("日 espaces/visible.log"), "visible").unwrap();
    let mut root = FileRoot::create(path.to_str().unwrap()).unwrap();
    let id = root.node_id.clone();
    let entries = root.entries(&id, None).unwrap();
    assert_eq!(entries.entries.len(), 3);
    assert!(entries.entries.iter().any(|e| e.name == "日 espaces"));
    let nested = file(&mut root, "日 espaces");
    let nested_entries = root.entries(&nested, None).unwrap();
    assert_eq!(nested_entries.entries.len(), 1);
    assert_eq!(nested_entries.entries[0].name, "visible.log");
    assert!(FileRoot::create("relative").is_err());
    assert!(FileRoot::create(path.join("a.log").to_str().unwrap()).is_err());
    assert!(root.check_node("../a.log", Kind::File).is_err());
    #[cfg(unix)]
    {
        use std::os::unix::fs::{PermissionsExt, symlink};
        symlink(std::env::temp_dir(), path.join("escape")).unwrap();
        assert_eq!(root.entries(&id, None).unwrap().entries.len(), 3);
        let a = file(&mut root, "a.log");
        fs::set_permissions(path.join("a.log"), fs::Permissions::from_mode(0o0)).unwrap();
        assert_eq!(
            root.content(&a, ContentOptions::default())
                .unwrap_err()
                .code,
            "PERMISSION"
        );
        fs::set_permissions(path.join("a.log"), fs::Permissions::from_mode(0o600)).unwrap();
    }
}
#[test]
fn snapshot_append_truncate_replace_and_same_size_rewrite() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("a.log");
    fs::write(&path, "début 🦀\r\n").unwrap();
    let mut root = FileRoot::create(temp.path().to_str().unwrap()).unwrap();
    let id = file(&mut root, "a.log");
    let first = root.content(&id, ContentOptions::default()).unwrap();
    append(&path, b"between");
    let next = root
        .content(
            &id,
            ContentOptions {
                after: Some(first.end.clone()),
                generation: Some(first.generation.clone()),
                ..Default::default()
            },
        )
        .unwrap();
    let mut bytes = STANDARD.decode(&first.data).unwrap();
    bytes.extend(STANDARD.decode(next.data).unwrap());
    assert_eq!(bytes, fs::read(&path).unwrap());
    fs::write(&path, "").unwrap();
    assert_eq!(
        root.content(
            &id,
            ContentOptions {
                generation: Some(first.generation),
                ..Default::default()
            }
        )
        .unwrap_err()
        .code,
        "GENERATION_CHANGED"
    );
    let empty = root.content(&id, ContentOptions::default()).unwrap();
    assert_eq!(empty.end, "0");
    fs::rename(&path, temp.path().join("old.log")).unwrap();
    fs::write(&path, "replacement").unwrap();
    assert_eq!(
        root.content(
            &id,
            ContentOptions {
                generation: Some(empty.generation),
                ..Default::default()
            }
        )
        .unwrap_err()
        .code,
        "GENERATION_CHANGED"
    );
    let replaced = root.content(&id, ContentOptions::default()).unwrap();
    fs::write(&path, "x".repeat(replaced.size.parse().unwrap())).unwrap();
    assert_eq!(
        root.content(
            &id,
            ContentOptions {
                generation: Some(replaced.generation),
                ..Default::default()
            }
        )
        .unwrap_err()
        .code,
        "GENERATION_CHANGED"
    );
    let before = fs::read(&path).unwrap();
    root.content(&id, ContentOptions::default()).unwrap();
    assert_eq!(before, fs::read(&path).unwrap());
}
#[test]
fn history_is_contiguous_and_bounded() {
    let temp = tempfile::tempdir().unwrap();
    let data = "界🦀\r\n".repeat(70000).into_bytes();
    fs::write(temp.path().join("large.log"), &data).unwrap();
    let mut root = FileRoot::create(temp.path().to_str().unwrap()).unwrap();
    let id = file(&mut root, "large.log");
    let mut chunk = root.content(&id, ContentOptions::default()).unwrap();
    let mut chunks = vec![STANDARD.decode(&chunk.data).unwrap()];
    assert!(chunks[0].len() <= SNAPSHOT);
    while chunk.start != "0" {
        chunk = root
            .content(
                &id,
                ContentOptions {
                    before: Some(chunk.start),
                    generation: Some(chunk.generation),
                    ..Default::default()
                },
            )
            .unwrap();
        chunks.push(STANDARD.decode(&chunk.data).unwrap());
    }
    chunks.reverse();
    assert_eq!(chunks.concat(), data);
    for invalid in ["-1", "9007199254740992", "18446744073709551616", ""] {
        assert!(offset(invalid).is_err());
    }
}
#[test]
fn pagination_and_changed_cursor() {
    let temp = tempfile::tempdir().unwrap();
    for n in 0..510 {
        fs::write(temp.path().join(format!("{n:04}.txt")), "").unwrap();
    }
    let mut root = FileRoot::create(temp.path().to_str().unwrap()).unwrap();
    let id = root.node_id.clone();
    let first = root.entries(&id, None).unwrap();
    assert_eq!(first.entries.len(), 500);
    assert_eq!(first.entries.first().unwrap().name, "0509.txt");
    assert_eq!(first.entries.last().unwrap().name, "0010.txt");
    let last = root.entries(&id, first.cursor.as_deref()).unwrap();
    assert_eq!(last.entries.len(), 10);
    assert_eq!(last.entries.first().unwrap().name, "0009.txt");
    assert_eq!(last.entries.last().unwrap().name, "0000.txt");
    assert_eq!(root.cached_nodes(), 511);
    fs::write(temp.path().join("new.log"), "").unwrap();
    assert_eq!(
        root.entries(&id, first.cursor.as_deref()).unwrap_err().code,
        "CURSOR_INVALID"
    );
}
#[cfg(unix)]
#[test]
fn cached_symlink_and_root_replacement_are_refused() {
    use std::os::unix::fs::symlink;
    let temp = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let path = temp.path().join("root");
    fs::create_dir(&path).unwrap();
    fs::write(path.join("a.log"), "inside").unwrap();
    fs::write(outside.path().join("secret.log"), "outside").unwrap();
    let mut root = FileRoot::create(path.to_str().unwrap()).unwrap();
    let id = file(&mut root, "a.log");
    fs::remove_file(path.join("a.log")).unwrap();
    symlink(outside.path().join("secret.log"), path.join("a.log")).unwrap();
    assert_eq!(
        root.content(&id, ContentOptions::default())
            .unwrap_err()
            .code,
        "FORBIDDEN"
    );
    fs::rename(&path, temp.path().join("old")).unwrap();
    fs::write(&path, "replaced").unwrap();
    let node = root.node_id.clone();
    assert_eq!(root.entries(&node, None).unwrap_err().code, "ROOT_CHANGED");
}
#[cfg(unix)]
#[test]
fn concurrent_directory_substitution_never_reads_outside_bytes() {
    use std::{io::Read, os::unix::fs::symlink, sync::Arc};
    let temp = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let root = temp.path().join("root");
    fs::create_dir(&root).unwrap();
    fs::create_dir(root.join("nested")).unwrap();
    fs::write(root.join("nested/data.log"), b"inside").unwrap();
    fs::write(outside.path().join("data.log"), b"outside-secret").unwrap();
    let directory = Arc::new(rlogger::storage::RootDir::open(&root).unwrap());
    let changing = {
        let root = root.clone();
        let outside = outside.path().to_path_buf();
        std::thread::spawn(move || {
            for _ in 0..500 {
                fs::rename(root.join("nested"), root.join("saved")).unwrap();
                symlink(&outside, root.join("nested")).unwrap();
                fs::remove_file(root.join("nested")).unwrap();
                fs::rename(root.join("saved"), root.join("nested")).unwrap();
            }
        })
    };
    for _ in 0..1500 {
        if let Ok(mut file) = directory.open_file(std::path::Path::new("nested/data.log")) {
            let mut data = String::new();
            file.read_to_string(&mut data).unwrap();
            assert_eq!(data, "inside");
        }
    }
    changing.join().unwrap();
}
