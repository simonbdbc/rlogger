use rlogger::storage;
use serde_json::{Value, json};
use std::{
    fs,
    io::BufRead,
    path::Path,
    process::{Child, Command, Stdio},
    time::Duration,
};

struct Companion(Child, String);
impl Companion {
    fn start() -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_local-logs-server"))
            .args(["--port", "0"])
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let mut line = String::new();
        std::io::BufReader::new(child.stdout.take().unwrap())
            .read_line(&mut line)
            .unwrap();
        Self(
            child,
            line.trim().strip_prefix("Local Logs : ").unwrap().into(),
        )
    }
    async fn open(&self, path: &Path) {
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let token = client
            .post(format!("{}/api/v1/session", self.1))
            .header("Origin", &self.1)
            .send()
            .await
            .unwrap()
            .json::<Value>()
            .await
            .unwrap()["token"]
            .as_str()
            .unwrap()
            .to_owned();
        assert_eq!(
            client
                .post(format!("{}/api/v1/roots", self.1))
                .header("Origin", &self.1)
                .header("x-local-session", token)
                .json(&json!({"absolutePath":path}))
                .send()
                .await
                .unwrap()
                .status(),
            200
        );
    }
}
impl Drop for Companion {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn recovered(parent: &Path) -> usize {
    fs::read_dir(parent)
        .unwrap()
        .filter(|e| {
            e.as_ref()
                .unwrap()
                .file_name()
                .to_string_lossy()
                .ends_with(".recovered.log")
        })
        .count()
}

#[tokio::test]
async fn killed_companion_resumes_partial_recovery_without_rewriting_bytes() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("rlogger");
    storage::initialize(&root).unwrap();
    storage::open_file(&storage::lease_path(&root, "1-2-3"), true, true).unwrap();
    let parent = root.join("1970-01-01/crash");
    fs::create_dir_all(&parent).unwrap();
    let bytes = b"unfinished UTF-8: \xf0\x9f";
    for i in 1..=2000 {
        fs::write(
            parent.join(format!("app-00-1-2-3-h0-s{i}.active.log")),
            bytes,
        )
        .unwrap();
    }
    let mut first = Companion::start();
    first.open(&root).await;
    tokio::time::timeout(Duration::from_secs(10), async {
        while recovered(&parent) == 0 {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    first.0.kill().unwrap();
    first.0.wait().unwrap();
    let done = recovered(&parent);
    assert!(
        done > 0 && done < 2000,
        "must interrupt an unfinished recovery, got {done}"
    );
    let second = Companion::start();
    second.open(&root).await;
    tokio::time::timeout(Duration::from_secs(15), async {
        while recovered(&parent) != 2000 {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(fs::read_dir(&parent).unwrap().count(), 2000);
    for file in fs::read_dir(&parent).unwrap() {
        assert_eq!(fs::read(file.unwrap().path()).unwrap(), bytes);
    }
}
