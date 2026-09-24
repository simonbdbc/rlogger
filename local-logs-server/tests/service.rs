use base64::{Engine, engine::general_purpose::STANDARD};
use futures_util::{SinkExt, StreamExt};
use local_logs_server::{Options, Service, start};
use serde_json::{Value, json};
use std::{fs, io::Write, time::Duration};
use tokio_tungstenite::{
    connect_async,
    tungstenite::{Message, client::IntoClientRequest},
};
async fn service() -> Service {
    start(Options {
        port: 0,
        poll: Duration::from_millis(10),
        ack_timeout: Duration::from_millis(150),
        ..Default::default()
    })
    .await
    .unwrap()
}
#[tokio::test]
async fn dist_symlink_is_refused_and_index_remains_available() {
    use std::os::unix::fs::symlink;
    let dist = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    fs::write(dist.path().join("index.html"), "inside-index").unwrap();
    fs::write(outside.path().join("secret.js"), "outside-secret").unwrap();
    symlink(outside.path(), dist.path().join("assets")).unwrap();
    let server = start(Options {
        port: 0,
        dist: dist.path().to_path_buf(),
        ..Default::default()
    })
    .await
    .unwrap();
    let index = client().get(&server.origin).send().await.unwrap();
    assert_eq!(index.status(), 200);
    assert_eq!(index.text().await.unwrap(), "inside-index");
    let escaped = client()
        .get(format!("{}/assets/secret.js", server.origin))
        .send()
        .await
        .unwrap();
    assert_eq!(escaped.status(), 404);
    server.close().await;
}
#[tokio::test]
async fn dev_reload_switches_to_the_completed_export() {
    let builds = tempfile::tempdir().unwrap();
    let first = builds.path().join("first");
    let second = builds.path().join("second");
    fs::create_dir_all(&first).unwrap();
    fs::create_dir_all(&second).unwrap();
    fs::write(first.join("index.html"), "first</body>").unwrap();
    fs::write(second.join("index.html"), "second</body>").unwrap();
    let server = start(Options {
        port: 0,
        dist: first,
        dev_reload: true,
        ..Default::default()
    })
    .await
    .unwrap();
    let browser = client();
    let html = browser
        .get(&server.origin)
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(html.starts_with("first"));
    assert!(html.contains("/__dev/version"));
    assert_eq!(
        browser
            .get(format!("{}/__dev/version", server.origin))
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap(),
        "0"
    );
    server.reload_dist(&second).unwrap();
    let html = browser
        .get(&server.origin)
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(html.starts_with("second"));
    assert!(html.contains("/__dev/version"));
    assert_eq!(
        browser
            .get(format!("{}/__dev/version", server.origin))
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap(),
        "1"
    );
    server.close().await;
}

#[tokio::test]
async fn thirty_two_http_connections_close_after_their_response() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let dist = tempfile::tempdir().unwrap();
    fs::write(dist.path().join("index.html"), "ok").unwrap();
    let server = start(Options {
        port: 0,
        dist: dist.path().to_path_buf(),
        ..Default::default()
    })
    .await
    .unwrap();
    let address = server.origin.trim_start_matches("http://").to_string();
    let mut tasks = Vec::new();
    for _ in 0..32 {
        let address = address.clone();
        tasks.push(tokio::spawn(async move {
            let mut stream = tokio::net::TcpStream::connect(&address).await.unwrap();
            stream
                .write_all(format!("GET / HTTP/1.1\r\nHost: {address}\r\n\r\n").as_bytes())
                .await
                .unwrap();
            let mut response = Vec::new();
            tokio::time::timeout(Duration::from_secs(3), stream.read_to_end(&mut response))
                .await
                .unwrap()
                .unwrap();
            assert!(response.starts_with(b"HTTP/1.1 200"));
        }));
    }
    for task in tasks {
        task.await.unwrap();
    }
    tokio::time::sleep(Duration::from_millis(50)).await;
    let mut stalled = Vec::new();
    for _ in 0..32 {
        let mut stream = tokio::net::TcpStream::connect(&address).await.unwrap();
        stream
            .write_all(format!("GET / HTTP/1.1\r\nHost: {address}\r\nX-Slow:").as_bytes())
            .await
            .unwrap();
        stalled.push(stream);
    }
    for mut stream in stalled {
        let mut response = Vec::new();
        tokio::time::timeout(Duration::from_secs(7), stream.read_to_end(&mut response))
            .await
            .unwrap()
            .unwrap();
    }
    assert_eq!(
        client().get(&server.origin).send().await.unwrap().status(),
        200
    );
    server.close().await;
}
fn client() -> reqwest::Client {
    reqwest::Client::builder().no_proxy().build().unwrap()
}
async fn session(s: &Service) -> String {
    client()
        .post(format!("{}/api/v1/session", s.origin))
        .header("Origin", &s.origin)
        .send()
        .await
        .unwrap()
        .json::<Value>()
        .await
        .unwrap()["token"]
        .as_str()
        .unwrap()
        .into()
}
async fn request(
    s: &Service,
    token: &str,
    method: reqwest::Method,
    path: &str,
    body: Value,
) -> reqwest::Response {
    client()
        .request(method, format!("{}{path}", s.origin))
        .header("Origin", &s.origin)
        .header("x-local-session", token)
        .json(&body)
        .send()
        .await
        .unwrap()
}
async fn open(s: &Service, token: &str, dir: &str) -> (Value, Value, Value) {
    let root = request(
        s,
        token,
        reqwest::Method::POST,
        "/api/v1/roots",
        json!({"absolutePath":dir}),
    )
    .await
    .json::<Value>()
    .await
    .unwrap();
    let root_id = root["rootId"].as_str().unwrap();
    let page = request(
        s,
        token,
        reqwest::Method::GET,
        &format!("/api/v1/roots/{root_id}/entries"),
        Value::Null,
    )
    .await
    .json::<Value>()
    .await
    .unwrap();
    let file = page["entries"][0].clone();
    let id = file["id"].as_str().unwrap();
    let content = request(
        s,
        token,
        reqwest::Method::GET,
        &format!("/api/v1/roots/{root_id}/files/{id}/content"),
        Value::Null,
    )
    .await
    .json::<Value>()
    .await
    .unwrap();
    (root, file, content)
}
#[tokio::test]
async fn external_request_keeps_a_valid_rlogger_root_unmanaged() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("rlogger");
    rlogger::storage::initialize(&root).unwrap();
    rlogger::storage::open_file(&rlogger::storage::lease_path(&root, "1-2-3"), true, true).unwrap();
    let folder = root.join("1970-01-01/test");
    fs::create_dir_all(&folder).unwrap();
    let active = folder.join("app-00-1-2-3-h0-s1.active.log");
    fs::write(&active, "unfinished\n").unwrap();
    let s = service().await;
    let token = session(&s).await;
    let response = request(
        &s,
        &token,
        reqwest::Method::POST,
        "/api/v1/roots",
        json!({"absolutePath": root, "maintenance": false}),
    )
    .await;
    assert_eq!(response.status(), 200);
    let opened = response.json::<Value>().await.unwrap();
    assert_eq!(
        opened["managedReason"],
        "Maintenance désactivée pour cette ouverture."
    );
    let endpoint = format!(
        "/api/v1/roots/{}/statistics",
        opened["rootId"].as_str().unwrap()
    );
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let snapshot = request(&s, &token, reqwest::Method::GET, &endpoint, Value::Null)
                .await
                .json::<Value>()
                .await
                .unwrap();
            if !snapshot["sampledAt"].as_str().unwrap_or("").is_empty()
                && snapshot["busy"].as_bool() == Some(false)
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert!(active.exists());
    assert!(!folder.join("app-00-1-2-3-h0-s1.recovered.log").exists());
    assert_eq!(
        request(
            &s,
            &token,
            reqwest::Method::POST,
            "/api/v1/roots",
            json!({"absolutePath": root, "maintenance": "no"}),
        )
        .await
        .status(),
        400
    );
    s.close().await;
}
type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;
async fn socket(s: &Service, token: &str) -> Socket {
    let mut request = format!("{}/api/v1/live", s.origin.replace("http:", "ws:"))
        .into_client_request()
        .unwrap();
    request
        .headers_mut()
        .insert("Origin", s.origin.parse().unwrap());
    request.headers_mut().insert(
        "Sec-WebSocket-Protocol",
        format!("local-logs.v1.{token}").parse().unwrap(),
    );
    connect_async(request).await.unwrap().0
}
async fn send(s: &mut Socket, v: Value) {
    s.send(Message::Text(v.to_string().into())).await.unwrap();
}
async fn next(s: &mut Socket, kind: &str) -> Value {
    tokio::time::timeout(Duration::from_secs(4), async {
        loop {
            let m = s.next().await.unwrap().unwrap();
            if let Message::Text(text) = m {
                let v: Value = serde_json::from_str(&text).unwrap();
                if v["type"] == kind {
                    return v;
                }
            }
        }
    })
    .await
    .unwrap()
}
#[tokio::test]
async fn managed_download_tickets_preconditions_and_deletion() {
    use rlogger::storage;
    let s = start(Options {
        port: 0,
        download_ticket_ttl: Duration::from_millis(150),
        ..Default::default()
    })
    .await
    .unwrap();
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("rlogger");
    storage::initialize(&dir).unwrap();
    storage::open_file(&storage::lease_path(&dir, "1-2-3"), true, true).unwrap();
    let parent = dir.join("1970-01-01/test");
    fs::create_dir_all(&parent).unwrap();
    let name = "app-00-1-2-3-h0-s1.log";
    fs::write(parent.join(name), "download 🦀\n").unwrap();
    let token = session(&s).await;
    let root = request(
        &s,
        &token,
        reqwest::Method::POST,
        "/api/v1/roots",
        json!({"absolutePath":dir}),
    )
    .await
    .json::<Value>()
    .await
    .unwrap();
    let rid = root["rootId"].as_str().unwrap();
    let mut node = root["nodeId"].as_str().unwrap().to_owned();
    let mut entry = Value::Null;
    for _ in 0..3 {
        let page = request(
            &s,
            &token,
            reqwest::Method::GET,
            &format!("/api/v1/roots/{rid}/entries?parentId={node}"),
            Value::Null,
        )
        .await
        .json::<Value>()
        .await
        .unwrap();
        entry = page["entries"]
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["name"] != ".rlogger")
            .unwrap()
            .clone();
        node = entry["id"].as_str().unwrap().into();
    }
    let path = format!("/api/v1/roots/{rid}/files/{node}");
    let identity = entry["identity"].as_str().unwrap();
    let prepare = || {
        client()
            .post(format!("{}{path}/download", s.origin))
            .header("Origin", &s.origin)
            .header("x-local-session", &token)
            .header("If-Match", identity)
            .send()
    };
    assert_eq!(
        request(&s, &token, reqwest::Method::DELETE, &path, Value::Null)
            .await
            .status(),
        428
    );
    let expired = prepare().await.unwrap().json::<Value>().await.unwrap();
    tokio::time::sleep(Duration::from_millis(180)).await;
    assert_eq!(
        client()
            .get(format!("{}{}", s.origin, expired["url"].as_str().unwrap()))
            .send()
            .await
            .unwrap()
            .status(),
        410
    );
    let ticket = prepare().await.unwrap().json::<Value>().await.unwrap();
    let url = format!("{}{}", s.origin, ticket["url"].as_str().unwrap());
    let response = client().get(&url).send().await.unwrap();
    assert!(
        response.headers()["content-disposition"]
            .to_str()
            .unwrap()
            .contains("attachment")
    );
    assert_eq!(response.text().await.unwrap(), "download 🦀\n");
    assert_eq!(client().get(url).send().await.unwrap().status(), 410);
    let ticket = prepare().await.unwrap().json::<Value>().await.unwrap();
    request(
        &s,
        &token,
        reqwest::Method::POST,
        "/api/v1/roots",
        json!({"absolutePath":dir}),
    )
    .await;
    assert_eq!(
        client()
            .get(format!("{}{}", s.origin, ticket["url"].as_str().unwrap()))
            .send()
            .await
            .unwrap()
            .status(),
        410
    );
    // A different session cannot reuse the old root/file identifiers.
    let other = session(&s).await;
    assert_ne!(
        request(&s, &other, reqwest::Method::DELETE, &path, Value::Null)
            .await
            .status(),
        200
    );
    s.close().await;
}

#[tokio::test]
async fn generic_folder_archive_and_recursive_delete_reconcile_selection() {
    use std::io::Read;
    let s = service().await;
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir_all(temp.path().join("folder/empty")).unwrap();
    fs::write(temp.path().join("folder/.hidden"), b"hidden").unwrap();
    fs::write(temp.path().join("folder/a.bin"), [0, 255, 10]).unwrap();
    let token = session(&s).await;
    let root = request(
        &s,
        &token,
        reqwest::Method::POST,
        "/api/v1/roots",
        json!({"absolutePath":temp.path()}),
    )
    .await
    .json::<Value>()
    .await
    .unwrap();
    let rid = root["rootId"].as_str().unwrap();
    let page = request(
        &s,
        &token,
        reqwest::Method::GET,
        &format!("/api/v1/roots/{rid}/entries"),
        Value::Null,
    )
    .await
    .json::<Value>()
    .await
    .unwrap();
    let entry = &page["entries"][0];
    assert_eq!(entry["name"], "folder");
    assert_eq!(entry["eligible"], true);
    let path = format!(
        "{}/api/v1/roots/{rid}/files/{}",
        s.origin,
        entry["id"].as_str().unwrap()
    );
    let identity = entry["identity"].as_str().unwrap();
    assert_eq!(
        client()
            .delete(&path)
            .header("Origin", &s.origin)
            .header("x-local-session", &token)
            .send()
            .await
            .unwrap()
            .status(),
        428
    );
    let ticket = client()
        .post(format!("{path}/download"))
        .header("Origin", &s.origin)
        .header("x-local-session", &token)
        .header("If-Match", identity)
        .send()
        .await
        .unwrap()
        .json::<Value>()
        .await
        .unwrap();
    let url = format!("{}{}", s.origin, ticket["url"].as_str().unwrap());
    let transfer = client().get(&url).send().await.unwrap();
    assert_eq!(transfer.status(), 200);
    assert!(
        transfer.headers()["content-disposition"]
            .to_str()
            .unwrap()
            .contains("folder%2Etar")
    );
    let size = transfer.content_length().unwrap();
    let bytes = transfer.bytes().await.unwrap();
    assert_eq!(bytes.len() as u64, size);
    let mut archive = tar::Archive::new(bytes.as_ref());
    let mut found = std::collections::BTreeMap::new();
    for entry in archive.entries().unwrap() {
        let mut entry = entry.unwrap();
        let name = entry.path().unwrap().into_owned();
        let mut content = Vec::new();
        entry.read_to_end(&mut content).unwrap();
        found.insert(name, content);
    }
    assert_eq!(found[std::path::Path::new("folder/.hidden")], b"hidden");
    assert_eq!(found[std::path::Path::new("folder/a.bin")], [0, 255, 10]);
    assert_eq!(client().get(url).send().await.unwrap().status(), 410);
    let response = client()
        .delete(&path)
        .header("Origin", &s.origin)
        .header("x-local-session", &token)
        .header("If-Match", identity)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200, "{}", response.text().await.unwrap());
    assert!(!temp.path().join("folder").exists());
    s.close().await;
}
fn subscribe(root: &Value, file: &Value, chunk: &Value, rev: u64) -> Value {
    json!({"type":"subscribe","rootId":root["rootId"],"fileId":file["id"],"generation":chunk["generation"],"offset":chunk["end"],"revision":rev})
}
#[tokio::test]
async fn large_transfer_cancel_releases_lock_and_delete_rechecks_revision() {
    use rlogger::storage;
    let s = service().await;
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("rlogger");
    storage::initialize(&dir).unwrap();
    storage::open_file(&storage::lease_path(&dir, "1-2-3"), true, true).unwrap();
    let parent = dir.join("1970-01-01/test");
    fs::create_dir_all(&parent).unwrap();
    let file_path = parent.join("large-00-1-2-3-h0-s1.log");
    fs::File::create(&file_path)
        .unwrap()
        .set_len(1024 * 1024 * 1024)
        .unwrap();
    let token = session(&s).await;
    let root = request(
        &s,
        &token,
        reqwest::Method::POST,
        "/api/v1/roots",
        json!({"absolutePath":dir}),
    )
    .await
    .json::<Value>()
    .await
    .unwrap();
    let rid = root["rootId"].as_str().unwrap();
    let mut node = root["nodeId"].as_str().unwrap().to_owned();
    let mut entry = Value::Null;
    for _ in 0..3 {
        let page = request(
            &s,
            &token,
            reqwest::Method::GET,
            &format!("/api/v1/roots/{rid}/entries?parentId={node}"),
            Value::Null,
        )
        .await
        .json::<Value>()
        .await
        .unwrap();
        entry = page["entries"]
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["name"] != ".rlogger")
            .unwrap()
            .clone();
        node = entry["id"].as_str().unwrap().into();
    }
    let path = format!("{}/api/v1/roots/{rid}/files/{node}", s.origin);
    let identity = entry["identity"].as_str().unwrap();
    let ticket = client()
        .post(format!("{path}/download"))
        .header("Origin", &s.origin)
        .header("x-local-session", &token)
        .header("If-Match", identity)
        .send()
        .await
        .unwrap()
        .json::<Value>()
        .await
        .unwrap();
    let transfer = client()
        .get(format!("{}{}", s.origin, ticket["url"].as_str().unwrap()))
        .send()
        .await
        .unwrap();
    assert_eq!(transfer.status(), 200);
    assert_eq!(transfer.content_length(), Some(1024 * 1024 * 1024));
    // A second actual companion process shares the OS file lock, not session state.
    struct Companion(std::process::Child);
    impl Drop for Companion {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let mut other = Companion(
        std::process::Command::new(env!("CARGO_BIN_EXE_local-logs-server"))
            .args(["--port", "0"])
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let mut ready = String::new();
    std::io::BufRead::read_line(
        &mut std::io::BufReader::new(other.0.stdout.take().unwrap()),
        &mut ready,
    )
    .unwrap();
    let other_origin = ready.trim().strip_prefix("Local Logs : ").unwrap();
    let other_token = client()
        .post(format!("{other_origin}/api/v1/session"))
        .header("Origin", other_origin)
        .send()
        .await
        .unwrap()
        .json::<Value>()
        .await
        .unwrap()["token"]
        .as_str()
        .unwrap()
        .to_owned();
    let other_request = |method, path: &str| {
        client()
            .request(method, format!("{other_origin}{path}"))
            .header("Origin", other_origin)
            .header("x-local-session", &other_token)
    };
    let other_root = other_request(reqwest::Method::POST, "/api/v1/roots")
        .json(&json!({"absolutePath":dir}))
        .send()
        .await
        .unwrap()
        .json::<Value>()
        .await
        .unwrap();
    let other_rid = other_root["rootId"].as_str().unwrap();
    let mut other_node = other_root["nodeId"].as_str().unwrap().to_owned();
    for _ in 0..3 {
        let page = other_request(
            reqwest::Method::GET,
            &format!("/api/v1/roots/{other_rid}/entries?parentId={other_node}"),
        )
        .send()
        .await
        .unwrap()
        .json::<Value>()
        .await
        .unwrap();
        other_node = page["entries"]
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["name"] != ".rlogger")
            .unwrap()["id"]
            .as_str()
            .unwrap()
            .into();
    }
    let other_path = format!("/api/v1/roots/{other_rid}/files/{other_node}");
    assert_eq!(
        other_request(reqwest::Method::DELETE, &other_path)
            .header("If-Match", identity)
            .send()
            .await
            .unwrap()
            .json::<Value>()
            .await
            .unwrap()["code"],
        "FILE_BUSY"
    );
    let second_ticket = other_request(reqwest::Method::POST, &format!("{other_path}/download"))
        .header("If-Match", identity)
        .send()
        .await
        .unwrap()
        .json::<Value>()
        .await
        .unwrap();
    let second_transfer = client()
        .get(format!(
            "{other_origin}{}",
            second_ticket["url"].as_str().unwrap()
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(second_transfer.status(), 200);
    let delete = || {
        client()
            .delete(&path)
            .header("Origin", &s.origin)
            .header("x-local-session", &token)
            .header("If-Match", identity)
            .send()
    };
    assert_eq!(
        delete().await.unwrap().json::<Value>().await.unwrap()["code"],
        "FILE_BUSY"
    );
    drop(transfer);
    assert_eq!(
        delete().await.unwrap().json::<Value>().await.unwrap()["code"],
        "FILE_BUSY"
    );
    // Keep the second client stalled: closing its root must cancel without a
    // further poll by that HTTP client or waiting for the full transfer.
    assert_eq!(
        other_request(
            reqwest::Method::DELETE,
            &format!("/api/v1/roots/{other_rid}")
        )
        .send()
        .await
        .unwrap()
        .status(),
        200
    );
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let f = fs::File::open(&file_path).unwrap();
            if f.try_lock().is_ok() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    // Same path and length, different file: old presentation cannot delete it.
    drop(second_transfer);
    fs::rename(&file_path, parent.join("keep.txt")).unwrap();
    fs::File::create(&file_path)
        .unwrap()
        .set_len(1024 * 1024 * 1024)
        .unwrap();
    assert_eq!(
        delete().await.unwrap().json::<Value>().await.unwrap()["code"],
        "FILE_CHANGED"
    );
    assert!(file_path.exists());
    let fresh = storage::token(&fs::metadata(&file_path).unwrap());
    assert_eq!(
        client()
            .delete(&path)
            .header("Origin", &s.origin)
            .header("x-local-session", &token)
            .header("If-Match", fresh)
            .send()
            .await
            .unwrap()
            .status(),
        200
    );
    assert!(!file_path.exists());
    assert!(parent.join("keep.txt").exists());
    s.close().await;
}
fn append(file: &std::path::Path, bytes: &[u8]) {
    fs::OpenOptions::new()
        .append(true)
        .open(file)
        .unwrap()
        .write_all(bytes)
        .unwrap();
}
#[tokio::test]
async fn http_boundaries_limits_and_failed_root_preserves_previous() {
    let s = service().await;
    let c = client();
    assert_eq!(
        c.get(format!("{}/api/v1/health", s.origin))
            .header("Host", "evil.example")
            .send()
            .await
            .unwrap()
            .status(),
        403
    );
    assert_eq!(
        c.post(format!("{}/api/v1/session", s.origin))
            .header("Origin", "https://evil.example")
            .send()
            .await
            .unwrap()
            .status(),
        403
    );
    let mut tokens = vec![];
    for _ in 0..4 {
        tokens.push(session(&s).await);
    }
    assert_eq!(
        c.post(format!("{}/api/v1/session", s.origin))
            .header("Origin", &s.origin)
            .send()
            .await
            .unwrap()
            .status(),
        429
    );
    let temp = tempfile::tempdir().unwrap();
    fs::write(temp.path().join("a.log"), "hello").unwrap();
    let (root, _, _) = open(&s, &tokens[0], temp.path().to_str().unwrap()).await;
    let endpoint = format!("/api/v1/roots/{}/entries", root["rootId"].as_str().unwrap());
    assert_eq!(
        request(&s, &tokens[1], reqwest::Method::GET, &endpoint, Value::Null)
            .await
            .status(),
        409
    );
    assert_eq!(
        request(
            &s,
            &tokens[0],
            reqwest::Method::POST,
            "/api/v1/roots",
            json!({"absolutePath":"/missing-root-synthetic"})
        )
        .await
        .status(),
        404
    );
    assert_eq!(
        request(&s, &tokens[0], reqwest::Method::GET, &endpoint, Value::Null)
            .await
            .status(),
        200
    );
    assert_eq!(
        request(
            &s,
            &tokens[0],
            reqwest::Method::DELETE,
            "/api/v1/session",
            Value::Null
        )
        .await
        .status(),
        200
    );
    assert_eq!(
        request(
            &s,
            &tokens[0],
            reqwest::Method::GET,
            "/api/v1/session",
            Value::Null
        )
        .await
        .status(),
        401
    );
    s.close().await;
}
#[tokio::test]
async fn websocket_catchup_reconnect_reset_and_origin() {
    let s = service().await;
    let token = session(&s).await;
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("a.log");
    fs::write(&path, "début 🦀\r\n").unwrap();
    let (root, file, initial) = open(&s, &token, temp.path().to_str().unwrap()).await;
    let mut bad = format!("{}/api/v1/live", s.origin.replace("http:", "ws:"))
        .into_client_request()
        .unwrap();
    bad.headers_mut()
        .insert("Origin", "https://evil.example".parse().unwrap());
    bad.headers_mut().insert(
        "Sec-WebSocket-Protocol",
        format!("local-logs.v1.{token}").parse().unwrap(),
    );
    assert!(connect_async(bad).await.is_err());
    append(&path, b"between");
    let mut ws = socket(&s, &token).await;
    send(&mut ws, subscribe(&root, &file, &initial, 1)).await;
    let chunk = next(&mut ws, "chunk").await;
    assert_eq!(
        STANDARD.decode(chunk["data"].as_str().unwrap()).unwrap(),
        b"between"
    );
    send(
        &mut ws,
        json!({"type":"ack","revision":1,"generation":chunk["generation"],"end":chunk["end"]}),
    )
    .await;
    ws.close(None).await.unwrap();
    drop(ws);
    append(&path, b"after");
    let mut ws = socket(&s, &token).await;
    send(&mut ws, subscribe(&root, &file, &chunk, 2)).await;
    let resumed = next(&mut ws, "chunk").await;
    assert_eq!(
        STANDARD.decode(resumed["data"].as_str().unwrap()).unwrap(),
        b"after"
    );
    fs::write(&path, "").unwrap();
    send(
        &mut ws,
        json!({"type":"ack","revision":2,"generation":resumed["generation"],"end":resumed["end"]}),
    )
    .await;
    next(&mut ws, "reset").await;
    s.close().await;
}
#[tokio::test]
async fn slow_client_has_one_chunk_and_tree_first_observation_invalidates() {
    let s = service().await;
    let token = session(&s).await;
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("a.log");
    fs::write(&path, "").unwrap();
    let (root, file, initial) = open(&s, &token, temp.path().to_str().unwrap()).await;
    let mut ws = socket(&s, &token).await;
    // Creation after the HTTP snapshot but before branch observation must be visible.
    fs::write(temp.path().join("new.log"), "new").unwrap();
    send(
        &mut ws,
        json!({"type":"branches","rootId":root["rootId"],"ids":[root["nodeId"]]}),
    )
    .await;
    next(&mut ws, "tree-changed").await;
    append(&path, &vec![b'x'; 2 * 1024 * 1024]);
    send(&mut ws, subscribe(&root, &file, &initial, 1)).await;
    let chunk = next(&mut ws, "chunk").await;
    assert_eq!(chunk["end"], "65536");
    assert_eq!(
        STANDARD
            .decode(chunk["data"].as_str().unwrap())
            .unwrap()
            .len(),
        65536
    );
    let mut chunks = 1;
    tokio::time::timeout(Duration::from_secs(3), async {
        while let Some(Ok(Message::Text(text))) = ws.next().await {
            let value: Value = serde_json::from_str(&text).unwrap();
            if value["type"] == "chunk" {
                chunks += 1;
            }
            if value["type"] == "resync-required" {
                break;
            }
        }
    })
    .await
    .unwrap();
    assert_eq!(chunks, 1);
    s.close().await;
}
#[tokio::test]
async fn expired_session_and_shutdown_release_resources() {
    let s = start(Options {
        port: 0,
        session_ttl: Duration::from_millis(30),
        ..Default::default()
    })
    .await
    .unwrap();
    let token = session(&s).await;
    tokio::time::sleep(Duration::from_millis(1100)).await;
    assert_eq!(
        request(
            &s,
            &token,
            reqwest::Method::GET,
            "/api/v1/session",
            Value::Null
        )
        .await
        .status(),
        401
    );
    let origin = s.origin.clone();
    s.close().await;
    assert!(client().get(origin).send().await.is_err());
}
