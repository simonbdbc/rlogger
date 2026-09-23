//! Synthetic local files for the companion and web reader demo.
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;

fn main() -> Result {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let directory = args
        .first()
        .filter(|arg| !arg.starts_with("--"))
        .map(String::as_str);
    fixtures(
        directory,
        args.iter().any(|arg| arg == "--large"),
        args.iter().any(|arg| arg == "--managed"),
    )
}

fn fixtures(directory: Option<&str>, large: bool, managed: bool) -> Result {
    let mut expected_directory = None;
    let directory = match directory {
        Some(path) => {
            let path = PathBuf::from(path);
            let absolute = if path.is_absolute() {
                path
            } else {
                std::env::current_dir()?.join(path)
            };
            let mut component = PathBuf::new();
            for part in absolute.components() {
                if matches!(part, std::path::Component::ParentDir) {
                    return Err("Fixture directory may not contain parent traversal".into());
                }
                component.push(part);
                let system_alias = (component == Path::new("/tmp")
                    && fs::canonicalize(&component).ok().as_deref()
                        == Some(Path::new("/private/tmp")))
                    || (component == Path::new("/var")
                        && fs::canonicalize(&component).ok().as_deref()
                            == Some(Path::new("/private/var")));
                if fs::symlink_metadata(&component)?.file_type().is_symlink() && !system_alias {
                    return Err("Fixture directory contains a symbolic link".into());
                }
            }
            let metadata = fs::symlink_metadata(&absolute)?;
            use std::os::unix::fs::MetadataExt;
            if !metadata.is_dir()
                || metadata.uid() != unsafe { libc::geteuid() }
                || metadata.mode() & 0o077 != 0
            {
                return Err(
                    "Fixture directory must be private and owned by the current account".into(),
                );
            }
            expected_directory = Some(metadata);
            absolute
        }
        None => tempfile::Builder::new()
            .prefix("local-logs-fixtures-")
            .tempdir()?
            .keep(),
    };
    let rooted = rlogger::storage::RootDir::open(&directory)?;
    if let Some(expected) = expected_directory
        && !rlogger::storage::same_file(&expected, &rooted.metadata()?)
    {
        return Err("Fixture directory changed during validation".into());
    }
    #[cfg(target_os = "macos")]
    if !fixture_acl_absent(rooted.file()) {
        return Err("Fixture directory has an extended ACL".into());
    }
    for (name, text) in [
        (
            "run-contract/test/2026-09-08/app.log",
            include_str!("../../lib-rust-logger/fixtures/v1/rust.log"),
        ),
        (
            "generic/notes.txt",
            "Texte générique\r\nUnicode : été, 日本語, 🦀\nFin sans newline",
        ),
        ("generic/empty.log", ""),
        (
            "run-demo/agents/2026-09-08/workflow-12.log",
            "Agent : début\n",
        ),
        (
            "run-demo/agents/2026-09-09/workflow-00.log",
            "Rotation : nouveau jour\n",
        ),
    ] {
        let relative = Path::new(name);
        let mut parent = PathBuf::new();
        for part in relative.parent().unwrap().components() {
            parent.push(part);
            rooted.create_directory(&parent)?;
        }
        match rooted.create_new(relative) {
            Ok(mut file) => file.write_all(text.as_bytes())?,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error.into()),
        }
    }
    if large {
        rooted.open_directory(Path::new("generic"))?;
        let temporary = (0..100)
            .find_map(|attempt| {
                let relative = PathBuf::from(format!(
                    "generic/.large-{}-{attempt}.tmp",
                    std::process::id()
                ));
                match rooted.create_new(&relative) {
                    Ok(file) => Some(Ok((relative, file))),
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => None,
                    Err(error) => Some(Err(error)),
                }
            })
            .ok_or("No unique temporary fixture path available")??;
        let (temporary_path, mut file) = temporary;
        let block = "Ligne synthétique 🦀\n".repeat(4096);
        for _ in 0..128 {
            file.write_all(block.as_bytes())?;
        }
        file.sync_all()?;
        drop(file);
        rooted.replace(&temporary_path, Path::new("generic/large.log"))?;
    }
    if managed {
        use chrono::Timelike;
        use rlogger::storage;
        let root = directory.join("rlogger");
        storage::initialize(&root)?;
        let managed_root = storage::RootDir::open(&root)?;
        match managed_root.create_new(Path::new(".rlogger/1-2-3.lock")) {
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error.into()),
        }
        let now = chrono::Local::now();
        let start = now.timestamp() - i64::from(now.minute()) * 60 - i64::from(now.second());
        let current = format!(
            "{}/test/current-{:02}-1-2-3-h{start}-s1.active.log",
            now.format("%Y-%m-%d"),
            now.hour()
        );
        for (relative, text) in [
            (
                "1970-01-01/test/archive-00-1-2-3-h0-s1.log",
                "Archive téléchargeable 🦀\n",
            ),
            (
                "1970-01-01/test/crash-00-1-2-3-h0-s2.active.log",
                "Dernière ligne incomplète",
            ),
            (&current, "Fichier actif\n"),
        ] {
            let relative = Path::new(relative);
            let mut parent = PathBuf::new();
            for part in relative.parent().unwrap().components() {
                parent.push(part);
                managed_root.create_directory(&parent)?;
            }
            match managed_root.create_new(relative) {
                Ok(mut file) => file.write_all(text.as_bytes())?,
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error.into()),
            }
        }
        managed_root.create_directory(Path::new("1970-01-02"))?;
        managed_root.create_directory(Path::new("1970-01-02/empty"))?;
    }
    println!("{}", fs::canonicalize(directory)?.display());
    Ok(())
}

#[cfg(target_os = "macos")]
fn fixture_acl_absent(file: &fs::File) -> bool {
    use std::os::fd::AsRawFd;
    unsafe extern "C" {
        fn acl_get_fd_np(fd: libc::c_int, kind: libc::c_int) -> *mut libc::c_void;
        fn acl_free(acl: *mut libc::c_void) -> libc::c_int;
    }
    let acl = unsafe { acl_get_fd_np(file.as_raw_fd(), 0x00000100) };
    if acl.is_null() {
        return std::io::Error::last_os_error().raw_os_error() == Some(libc::ENOENT);
    }
    unsafe { acl_free(acl) };
    false
}
