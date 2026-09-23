//! Rust-only local companion; the logger remains an independent crate.
pub mod actions;
pub mod files;
pub mod management;
pub mod protocol;
use axum::{
    Json, Router,
    body::{Body, to_bytes},
    extract::{
        Request, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::{HeaderMap, Method, StatusCode, header},
    response::{IntoResponse, Response},
};
use files::FileRoot;
use hyper_util::{
    rt::{TokioExecutor, TokioIo, TokioTimer},
    server::conn::auto::Builder,
    service::TowerToHyperService,
};
use protocol::*;
use rlogger::storage::RootDir;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, HashMap},
    path::{Path, PathBuf},
    pin::Pin,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    task::{Context, Poll},
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, ReadBuf},
    net::{TcpListener, TcpStream},
    sync::{OwnedSemaphorePermit, Semaphore, watch},
    task::JoinHandle,
};
use tokio_util::{io::ReaderStream, task::TaskTracker};

#[derive(Clone)]
pub struct Options {
    pub port: u16,
    pub dist: PathBuf,
    pub poll: Duration,
    pub ack_timeout: Duration,
    pub session_ttl: Duration,
    pub download_ticket_ttl: Duration,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            port: 4317,
            dist: Path::new(env!("CARGO_MANIFEST_DIR")).join("../front-react-logger/dist"),
            poll: Duration::from_millis(250),
            ack_timeout: Duration::from_secs(10),
            session_ttl: Duration::from_secs(120),
            download_ticket_ttl: Duration::from_secs(30),
        }
    }
}
struct Ack {
    end: String,
    sent: Instant,
}
struct Data {
    root: Option<FileRoot>,
    selection: Option<Selection>,
    ack: Option<Ack>,
    branches: BTreeMap<String, String>,
    unavailable: Option<String>,
    tree_at: Instant,
    job: Option<Arc<management::Job>>,
}
struct Ticket {
    key: String,
    root: String,
    file: String,
    identity: String,
    expires: Instant,
}
struct Session {
    data: Arc<tokio::sync::Mutex<Data>>,
    cancel: watch::Sender<u64>,
    connected: AtomicBool,
    touched: AtomicU64,
    closed: AtomicBool,
    ticket: Mutex<Option<Ticket>>,
    transfer: Arc<Semaphore>,
    root_epoch: AtomicU64,
}
struct App {
    options: Options,
    dist: Option<RootDir>,
    host: String,
    origin: String,
    id: String,
    sessions: Mutex<HashMap<String, Arc<Session>>>,
    start: Instant,
    stop: watch::Sender<bool>,
    tasks: TaskTracker,
    management: management::Hub,
    transfers: Arc<Semaphore>,
}
impl App {
    fn now(&self) -> u64 {
        self.start.elapsed().as_millis() as u64
    }
    fn session(&self, headers: &HeaderMap) -> Result<Arc<Session>> {
        let token = headers
            .get("x-local-session")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        let session = self
            .sessions
            .lock()
            .unwrap()
            .get(token)
            .cloned()
            .ok_or_else(|| {
                Error::new(
                    "SESSION_EXPIRED",
                    "Session locale expirée. Rouvrez le dossier.",
                    401,
                )
            })?;
        session.touched.store(self.now(), Ordering::Relaxed);
        Ok(session)
    }
}
pub struct Service {
    pub origin: String,
    app: Arc<App>,
    task: Option<JoinHandle<()>>,
}
impl Service {
    pub async fn close(mut self) {
        self.app.stop.send_replace(true);
        if let Some(task) = self.task.take() {
            let _ = task.await;
        }
        self.app.tasks.close();
        self.app.tasks.wait().await;
    }
}
impl Drop for Service {
    fn drop(&mut self) {
        self.app.stop.send_replace(true);
    }
}
/// Connection permit follows the stream into a WebSocket upgrade, not just the HTTP task.
struct Connection {
    stream: TcpStream,
    _permit: OwnedSemaphorePermit,
}
impl AsyncRead for Connection {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.stream).poll_read(cx, buf)
    }
}
impl AsyncWrite for Connection {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut self.stream).poll_write(cx, buf)
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.stream).poll_flush(cx)
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.stream).poll_shutdown(cx)
    }
}
pub async fn start(options: Options) -> std::io::Result<Service> {
    let dist = RootDir::open(&options.dist).ok();
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, options.port)).await?;
    let host = listener.local_addr()?.to_string();
    let origin = format!("http://{host}");
    let (stop, _) = watch::channel(false);
    let app = Arc::new(App {
        options,
        dist,
        host,
        origin: origin.clone(),
        id: id(),
        sessions: Mutex::new(HashMap::new()),
        start: Instant::now(),
        stop,
        tasks: TaskTracker::new(),
        management: management::Hub::default(),
        transfers: Arc::new(Semaphore::new(4)),
    });
    let router = Router::new().fallback(endpoint).with_state(app.clone());
    let state = app.clone();
    let task = tokio::spawn(async move {
        let semaphore = Arc::new(Semaphore::new(32));
        let mut tasks = tokio::task::JoinSet::new();
        let mut stop = state.stop.subscribe();
        let mut cleanup = tokio::time::interval(Duration::from_secs(1));
        loop {
            tokio::select! {
                            _=stop.changed()=>break,
                            _=cleanup.tick()=>{let now=state.now();state.sessions.lock().unwrap().retain(|_,s| {let keep=s.connected.load(Ordering::Relaxed)||now.saturating_sub(s.touched.load(Ordering::Relaxed))<state.options.session_ttl.as_millis() as u64;if !keep{s.closed.store(true,Ordering::Relaxed);}
            if let Ok(d)=s.data.try_lock() && let Some(job)=&d.job {job.kick(false);} keep});},
                            Some(_)=tasks.join_next(),if !tasks.is_empty()=>{},
                            accepted=listener.accept()=>{
                                let Ok((stream,_))=accepted else {break;};
                                let Ok(permit)=semaphore.clone().try_acquire_owned() else {drop(stream);continue;};
                                let router=router.clone();let mut stop=state.stop.subscribe();
                                tasks.spawn(async move {
                                    let io=TokioIo::new(Connection{stream,_permit:permit});
                                    let mut builder=Builder::new(TokioExecutor::new()).http1_only();builder.http1().timer(TokioTimer::new()).header_read_timeout(Duration::from_secs(5));
                                    let service=TowerToHyperService::new(router);
                                    tokio::select! {_=stop.changed()=>{},_=builder.serve_connection_with_upgrades(io,service)=>{}}
                                });
                            }
                        }
        }
        state.sessions.lock().unwrap().clear();
        tasks.abort_all();
        while tasks.join_next().await.is_some() {}
    });
    Ok(Service {
        origin,
        app,
        task: Some(task),
    })
}
fn guard(app: &App, headers: &HeaderMap, require_origin: bool) -> Result<()> {
    let value = |key| headers.get(key).and_then(|v| v.to_str().ok());
    if value("host") != Some(app.host.as_str())
        || value("origin").is_some_and(|v| v != app.origin)
        || require_origin && value("origin") != Some(app.origin.as_str())
        || value("sec-fetch-site") == Some("cross-site")
    {
        return Err(Error::new("FORBIDDEN", "Origine locale requise.", 403));
    }
    Ok(())
}
fn root<'a>(data: &'a mut Data, id: &str) -> Result<&'a mut FileRoot> {
    data.root
        .as_mut()
        .filter(|r| r.id == id)
        .ok_or_else(|| Error::new("ROOT_CHANGED", "La racine active a changé.", 409))
}
async fn disk<T: Send + 'static>(
    session: Arc<Session>,
    f: impl FnOnce(&mut Data) -> Result<T> + Send + 'static,
) -> Result<T> {
    if session.closed.load(Ordering::Relaxed) {
        return Err(Error::new(
            "SESSION_EXPIRED",
            "Session locale expirée.",
            401,
        ));
    }
    let mut data = session
        .data
        .clone()
        .try_lock_owned()
        .map_err(|_| Error::new("LIMIT", "Lecture en cours, réessayez.", 429))?;
    tokio::task::spawn_blocking(move || f(&mut data))
        .await
        .map_err(|_| Error::new("INTERNAL", "Lecture interrompue.", 500))?
}
// A browser attachment cannot retry a JSON 429 response. Transfer permits bound
// the waiters; wait fairly behind the short live-reader operation instead.
async fn download_disk<T: Send + 'static>(
    session: Arc<Session>,
    f: impl FnOnce(&mut Data) -> Result<T> + Send + 'static,
) -> Result<T> {
    let mut data = tokio::time::timeout(Duration::from_secs(2), session.data.clone().lock_owned())
        .await
        .map_err(|_| {
            Error::new(
                "LIMIT",
                "Lecture indisponible, préparez un nouveau téléchargement.",
                429,
            )
        })?;
    if session.closed.load(Ordering::Relaxed) {
        return Err(Error::new(
            "SESSION_EXPIRED",
            "Session locale expirée.",
            401,
        ));
    }
    tokio::task::spawn_blocking(move || f(&mut data))
        .await
        .map_err(|_| Error::new("INTERNAL", "Lecture interrompue.", 500))?
}
async fn endpoint(State(app): State<Arc<App>>, request: Request) -> Response {
    let mut response = match handle(app, request).await {
        Ok(r) => r,
        Err(e) => e.into_response(),
    };
    for (key, value) in [
        ("x-content-type-options", "nosniff"),
        ("referrer-policy", "no-referrer"),
        ("cache-control", "no-store"),
        ("cross-origin-resource-policy", "same-origin"),
        (
            "content-security-policy",
            "default-src 'self'; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; font-src 'self' data:; connect-src 'self' ws://127.0.0.1:*; frame-ancestors 'none'; base-uri 'none'",
        ),
    ] {
        response
            .headers_mut()
            .insert(key, header::HeaderValue::from_static(value));
    }
    if response.status() != StatusCode::SWITCHING_PROTOCOLS {
        response.headers_mut().insert(
            header::CONNECTION,
            header::HeaderValue::from_static("close"),
        );
    }
    response
}
async fn handle(app: Arc<App>, request: Request) -> Result<Response> {
    guard(&app, request.headers(), false)?;
    let method = request.method().clone();
    let pathname = request.uri().path().to_owned();
    if pathname == "/api/v1/health" && method == Method::GET {
        return Ok(Json(json!({"version":1,"serverId":app.id})).into_response());
    }
    if pathname == "/api/v1/session" && method == Method::POST {
        guard(&app, request.headers(), true)?;
        let mut sessions = app.sessions.lock().unwrap();
        if sessions.len() >= 4 {
            return Err(Error::new(
                "LIMIT",
                "Quatre lecteurs sont déjà ouverts.",
                429,
            ));
        }
        let token = format!(
            "{}{}",
            uuid::Uuid::new_v4().simple(),
            uuid::Uuid::new_v4().simple()
        );
        let (cancel, _) = watch::channel(0);
        sessions.insert(
            token.clone(),
            Arc::new(Session {
                data: Arc::new(tokio::sync::Mutex::new(Data {
                    root: None,
                    selection: None,
                    ack: None,
                    branches: BTreeMap::new(),
                    unavailable: None,
                    tree_at: Instant::now() - Duration::from_secs(1),
                    job: None,
                })),
                cancel,
                connected: AtomicBool::new(false),
                touched: AtomicU64::new(app.now()),
                closed: AtomicBool::new(false),
                ticket: Mutex::new(None),
                transfer: Arc::new(Semaphore::new(1)),
                root_epoch: AtomicU64::new(0),
            }),
        );
        return Ok(Json(json!({"version":1,"serverId":app.id,"token":token})).into_response());
    }
    if pathname == "/api/v1/live" {
        guard(&app, request.headers(), true)?;
        let protocol = request
            .headers()
            .get("sec-websocket-protocol")
            .and_then(|s| s.to_str().ok())
            .filter(|p| p.starts_with("local-logs.v1."))
            .ok_or_else(|| Error::new("FORBIDDEN", "Flux refusé.", 403))?
            .to_owned();
        let session = app
            .sessions
            .lock()
            .unwrap()
            .get(&protocol[14..])
            .cloned()
            .ok_or_else(|| Error::new("SESSION_EXPIRED", "Session expirée.", 401))?;
        let (mut parts, _) = request.into_parts();
        use axum::extract::FromRequestParts;
        let ws = WebSocketUpgrade::from_request_parts(&mut parts, &())
            .await
            .map_err(|_| Error::new("INVALID_REQUEST", "Flux invalide.", 400))?;
        return Ok(ws
            .protocols([protocol])
            .max_message_size(BODY)
            .max_frame_size(BODY)
            .write_buffer_size(0)
            .max_write_buffer_size(1024 * 1024)
            .on_upgrade(move |socket| async move {
                let tasks = app.tasks.clone();
                tasks.spawn(async move {
                    let mut stop = app.stop.subscribe();
                    if *stop.borrow() {
                        return;
                    }
                    tokio::select! {_=stop.changed()=>{},_=live(app,session,socket)=>{}}
                });
            })
            .into_response());
    }
    if pathname == "/api/v1/session" {
        let session = app.session(request.headers())?;
        if method == Method::GET {
            return Ok(Json(json!({"version":1,"serverId":app.id})).into_response());
        }
        if method == Method::DELETE {
            session.closed.store(true, Ordering::Relaxed);
            session.cancel.send_modify(|v| *v += 1);
            app.sessions
                .lock()
                .unwrap()
                .remove(request.headers()["x-local-session"].to_str().unwrap());
            return Ok(Json(json!({"closed":true})).into_response());
        }
    }
    if method == Method::GET
        && let Some(key) = pathname.strip_prefix("/api/v1/downloads/")
    {
        return download(&app, key).await;
    }
    if pathname.starts_with("/api/") {
        let session = app.session(request.headers())?;
        let query: HashMap<String, String> =
            url::form_urlencoded::parse(request.uri().query().unwrap_or("").as_bytes())
                .into_owned()
                .collect();
        if pathname == "/api/v1/roots" && method == Method::POST {
            let bytes =
                tokio::time::timeout(Duration::from_secs(10), to_bytes(request.into_body(), BODY))
                    .await
                    .map_err(|_| Error::new("LIMIT", "Requête expirée.", 408))?
                    .map_err(|_| Error::new("LIMIT", "Requête trop volumineuse.", 413))?;
            let json: Value = serde_json::from_slice(&bytes)
                .map_err(|_| Error::new("INVALID_REQUEST", "JSON invalide.", 400))?;
            let path = json
                .get("absolutePath")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    Error::new(
                        "INVALID_REQUEST",
                        "Saisissez un chemin absolu de dossier.",
                        400,
                    )
                })?
                .to_owned();
            let manager = app.clone();
            let current = session.clone();
            return Ok(Json(
                disk(session, move |d| {
                    let opened = FileRoot::create(&path)?;
                    let dto = opened.dto();
                    let job = manager.management.get(opened.absolute().to_owned());
                    job.kick(false);
                    d.root = Some(opened);
                    d.job = Some(job);
                    current.ticket.lock().unwrap().take();
                    current.root_epoch.fetch_add(1, Ordering::Relaxed);
                    d.selection = None;
                    d.ack = None;
                    d.unavailable = None;
                    d.branches.clear();
                    Ok(dto)
                })
                .await?,
            )
            .into_response());
        }
        let parts: Vec<_> = pathname.trim_start_matches('/').split('/').collect();
        if parts.len() >= 4 && parts[..3] == ["api", "v1", "roots"] {
            let root_id = parts[3].to_owned();
            if parts.len() == 4 && method == Method::DELETE {
                let current = session.clone();
                return Ok(Json(
                    disk(session, move |d| {
                        root(d, &root_id)?;
                        d.root = None;
                        d.job = None;
                        current.ticket.lock().unwrap().take();
                        current.root_epoch.fetch_add(1, Ordering::Relaxed);
                        d.selection = None;
                        d.ack = None;
                        d.branches.clear();
                        Ok(json!({"closed":true}))
                    })
                    .await?,
                )
                .into_response());
            }
            if parts.len() == 5 && parts[4] == "statistics" && method == Method::GET {
                return Ok(Json(
                    disk(session, move |d| {
                        root(d, &root_id)?;
                        let job = d.job.as_ref().unwrap();
                        job.kick(query.get("refresh").is_some_and(|s| s == "1"));
                        Ok(job.snapshot())
                    })
                    .await?,
                )
                .into_response());
            }
            if parts.len() >= 6 && parts[4] == "files" {
                let file_id = parts[5].to_owned();
                if parts.len() == 6 && method == Method::GET {
                    return Ok(Json(
                        disk(session, move |d| {
                            let r = root(d, &root_id)?;
                            if r.entry(&file_id)?.kind == Kind::File {
                                r.relocate(&file_id)?;
                            }
                            r.entry(&file_id)
                        })
                        .await?,
                    )
                    .into_response());
                }
                if method == Method::DELETE && parts.len() == 6
                    || method == Method::POST && parts.len() == 7 && parts[6] == "download"
                {
                    guard(&app, request.headers(), true)?;
                    let expected = request
                        .headers()
                        .get("if-match")
                        .and_then(|v| v.to_str().ok())
                        .ok_or_else(|| {
                            Error::new("PRECONDITION_REQUIRED", "Identité du fichier requise.", 428)
                        })?
                        .to_owned();
                    let deleting = method == Method::DELETE;
                    if deleting {
                        return delete_entry(session, root_id, file_id, expected).await;
                    }
                    let current = session.clone();
                    let ticket_ttl = app.options.download_ticket_ttl.min(Duration::from_secs(30));
                    let value = disk(session, move |d| {
                        let r = root(d, &root_id)?;
                        let relative = r.management_path(&file_id)?;
                        let action = actions::open(r.directory(), &relative, &expected, false)?;
                        {
                            drop(action);
                            if current.transfer.available_permits() == 0 {
                                return Err(Error::new(
                                    "FILE_BUSY",
                                    "Téléchargement en cours.",
                                    409,
                                ));
                            }
                            let key = format!(
                                "{}{}",
                                uuid::Uuid::new_v4().simple(),
                                uuid::Uuid::new_v4().simple()
                            );
                            *current.ticket.lock().unwrap() = Some(Ticket {
                                key: key.clone(),
                                root: root_id,
                                file: file_id,
                                identity: expected,
                                expires: Instant::now() + ticket_ttl,
                            });
                            Ok(json!({"url":format!("/api/v1/downloads/{key}")}))
                        }
                    })
                    .await?;
                    return Ok(Json(value).into_response());
                }
            }
            if parts.len() == 5 && parts[4] == "entries" && method == Method::GET {
                return Ok(Json(
                    disk(session, move |d| {
                        let r = root(d, &root_id)?;
                        let parent = query
                            .get("parentId")
                            .cloned()
                            .unwrap_or_else(|| r.node_id.clone());
                        r.entries(&parent, query.get("cursor").map(String::as_str))
                    })
                    .await?,
                )
                .into_response());
            }
            if parts.len() == 7
                && parts[4] == "files"
                && parts[6] == "content"
                && method == Method::GET
            {
                let file_id = parts[5].to_owned();
                return Ok(Json(
                    disk(session, move |d| {
                        root(d, &root_id)?.content(
                            &file_id,
                            ContentOptions {
                                generation: query.get("generation").cloned(),
                                before: query.get("before").cloned(),
                                after: query.get("after").cloned(),
                                limit: None,
                            },
                        )
                    })
                    .await?,
                )
                .into_response());
            }
        }
        return Err(Error::new("NOT_FOUND", "Opération inconnue.", 404));
    }
    if method != Method::GET && method != Method::HEAD {
        return Err(Error::new("FORBIDDEN", "Méthode non permise.", 405));
    }
    let decoded = percent_encoding::percent_decode_str(&pathname)
        .decode_utf8()
        .map_err(|_| Error::new("FORBIDDEN", "Asset refusé.", 403))?;
    let relative = Path::new(decoded.trim_start_matches('/'));
    if relative
        .components()
        .any(|c| !matches!(c, std::path::Component::Normal(_)))
    {
        return Err(Error::new("FORBIDDEN", "Asset refusé.", 403));
    }
    let dist = app
        .dist
        .as_ref()
        .ok_or_else(|| Error::new("NOT_FOUND", "Build absent : exécutez npm run build.", 404))?;
    let target = if decoded == "/"
        || relative.extension().is_none()
        || dist.open_directory(relative).is_ok()
    {
        Path::new("index.html")
    } else {
        relative
    };
    let file = dist
        .open_file(target)
        .map_err(|_| Error::new("NOT_FOUND", "Build absent ou asset refusé.", 404))?;
    let file = tokio::fs::File::from_std(file);
    let mime = match target.extension().and_then(|e| e.to_str()).unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" => "application/json",
        "png" => "image/png",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        _ => "application/octet-stream",
    };
    let body = if method == Method::HEAD {
        Body::empty()
    } else {
        Body::from_stream(ReaderStream::new(file))
    };
    Ok((StatusCode::OK, [(header::CONTENT_TYPE, mime)], body).into_response())
}
async fn send(socket: &mut WebSocket, message: Value) -> bool {
    tokio::time::timeout(
        Duration::from_secs(2),
        socket.send(Message::Text(message.to_string().into())),
    )
    .await
    .is_ok_and(|r| r.is_ok())
}
async fn delete_entry(
    session: Arc<Session>,
    root_id: String,
    file_id: String,
    expected: String,
) -> Result<Response> {
    let current = session.clone();
    let (directory, relative, action, epoch, job, selected, expected) =
        disk(session.clone(), move |d| {
            let r = root(d, &root_id)?;
            let relative = r.management_path(&file_id)?;
            let action = actions::open(r.directory(), &relative, &expected, true)?;
            let directory = r.directory_handle();
            let selected = d
                .selection
                .as_ref()
                .filter(|s| {
                    d.root.as_ref().is_some_and(|r| {
                        r.management_path(&s.file_id)
                            .is_ok_and(|p| p.starts_with(&relative))
                    })
                })
                .map(|s| s.file_id.clone());
            Ok((
                directory,
                relative,
                action,
                current.root_epoch.load(Ordering::Relaxed),
                d.job.clone(),
                selected,
                expected,
            ))
        })
        .await?;
    // Inventory and recursive work do not monopolize the session's live reader.
    let current = session.clone();
    let result = tokio::task::spawn_blocking(move || {
        let _action = action;
        let cancelled = || {
            current.closed.load(Ordering::Relaxed)
                || current.root_epoch.load(Ordering::Relaxed) != epoch
        };
        let tree = actions::Tree::collect(&directory, &relative, cancelled)?;
        tree.verify_target(&expected)?;
        tree.delete(&directory, cancelled)
    })
    .await
    .map_err(|_| Error::new("INTERNAL", "Suppression interrompue.", 500))?;
    if let Some(job) = job {
        job.invalidate();
    }
    result?;
    let current = session.clone();
    let _ = download_disk(session, move |d| {
        if current.root_epoch.load(Ordering::Relaxed) == epoch
            && selected.is_some()
            && d.selection.as_ref().map(|s| &s.file_id) == selected.as_ref()
        {
            d.selection = None;
            d.ack = None;
        }
        Ok(())
    })
    .await;
    Ok(Json(json!({"deleted":true})).into_response())
}

async fn download(app: &Arc<App>, key: &str) -> Result<Response> {
    let sessions: Vec<_> = app.sessions.lock().unwrap().values().cloned().collect();
    for session in sessions {
        let ticket = {
            let mut ticket = session.ticket.lock().unwrap();
            if ticket.as_ref().is_some_and(|t| t.key == key) {
                ticket.take()
            } else {
                None
            }
        };
        let Some(ticket) = ticket else {
            continue;
        };
        if ticket.expires <= Instant::now() || session.closed.load(Ordering::Relaxed) {
            break;
        }
        let local = session
            .transfer
            .clone()
            .try_acquire_owned()
            .map_err(|_| Error::new("FILE_BUSY", "Téléchargement en cours.", 409))?;
        let global = app
            .transfers
            .clone()
            .try_acquire_owned()
            .map_err(|_| Error::new("LIMIT", "Trop de téléchargements.", 429))?;
        let current = session.clone();
        let (action, directory, path, mut name, epoch) = download_disk(session.clone(), move |d| {
            if ticket.expires <= Instant::now() {
                return Err(Error::new(
                    "DOWNLOAD_EXPIRED",
                    "Lien de téléchargement expiré.",
                    410,
                ));
            }
            let r = root(d, &ticket.root)?;
            let p = r.management_path(&ticket.file)?;
            let action = actions::open(r.directory(), &p, &ticket.identity, false)?;
            Ok((
                action,
                r.directory_handle(),
                p.clone(),
                p.file_name().unwrap().to_string_lossy().into_owned(),
                current.root_epoch.load(Ordering::Relaxed),
            ))
        })
        .await?;
        let is_directory = action.file.metadata()?.is_dir();
        let archive = if is_directory {
            name.push_str(".tar");
            let root = directory.clone();
            let current = session.clone();
            let expected = action.identity.clone();
            Some(
                tokio::task::spawn_blocking(move || {
                    let tree = actions::Tree::collect(&root, &path, || {
                        current.closed.load(Ordering::Relaxed)
                            || current.root_epoch.load(Ordering::Relaxed) != epoch
                    })?;
                    tree.verify_target(&expected)?;
                    Ok::<_, Error>(tree)
                })
                .await
                .map_err(|_| Error::new("INTERNAL", "Inventaire interrompu.", 500))??,
            )
        } else {
            None
        };
        let len = if let Some(tree) = &archive {
            tree.archive_len()?
        } else {
            action.file.metadata()?.len()
        };
        // The bounded pipe decouples cancellation from HTTP body polling: an
        // unresponsive client must not retain the file after root/session closure.
        let (mut output, transfer) = tokio::io::duplex(CHUNK);
        let mut stop = app.stop.subscribe();
        app.tasks.spawn(async move {
            let _permits = (local, global);
            let cancelled = async {
                loop {
                    if *stop.borrow()
                        || session.closed.load(Ordering::Relaxed)
                        || session.root_epoch.load(Ordering::Relaxed) != epoch
                    {
                        break;
                    }
                    tokio::select! {
                        _ = stop.changed() => break,
                        _ = tokio::time::sleep(Duration::from_millis(50)) => {}
                    }
                }
            };
            let transfer = async {
                // Retain target and ancestor locks until the last byte/cancellation.
                if let Some(tree) = archive {
                    tree.archive(directory, &mut output).await
                } else {
                    let mut source = tokio::fs::File::from_std(action.file.try_clone()?);
                    source.set_max_buf_size(CHUNK);
                    tokio::io::copy(&mut source.take(len), &mut output)
                        .await
                        .map(|_| ())
                }
            };
            tokio::select! {
                _ = transfer => {},
                _ = cancelled => {},
            }
            drop(action);
        });
        let mut response =
            Body::from_stream(ReaderStream::with_capacity(transfer, CHUNK)).into_response();
        let headers = response.headers_mut();
        headers.insert(
            header::CONTENT_TYPE,
            "application/octet-stream".parse().unwrap(),
        );
        headers.insert(header::CONTENT_LENGTH, len.to_string().parse().unwrap());
        headers.insert(
            header::CONTENT_DISPOSITION,
            format!(
                "attachment; filename=\"log.log\"; filename*=UTF-8''{}",
                percent_encoding::utf8_percent_encode(&name, percent_encoding::NON_ALPHANUMERIC)
            )
            .parse()
            .unwrap(),
        );
        headers.insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
        headers.insert("referrer-policy", "no-referrer".parse().unwrap());
        return Ok(response);
    }
    Err(Error::new(
        "DOWNLOAD_EXPIRED",
        "Lien de téléchargement expiré ou déjà utilisé.",
        410,
    ))
}
async fn live(app: Arc<App>, session: Arc<Session>, mut socket: WebSocket) {
    let mut cancel = session.cancel.subscribe();
    let generation = {
        let mut data = session.data.lock().await;
        data.selection = None;
        data.ack = None;
        data.unavailable = None;
        session.cancel.send_modify(|v| *v += 1);
        *session.cancel.borrow()
    };
    cancel.borrow_and_update();
    session.connected.store(true, Ordering::Relaxed);
    let mut tick = tokio::time::interval(app.options.poll);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut heartbeat = Instant::now() - Duration::from_secs(5);
    loop {
        if session.closed.load(Ordering::Relaxed) || *cancel.borrow() != generation {
            break;
        }
        session.touched.store(app.now(), Ordering::Relaxed);
        let message = tokio::select! {_=cancel.changed()=>break,_=tick.tick()=>None,message=socket.recv()=>match message{Some(Ok(Message::Text(text)))=>Some(text),Some(Ok(Message::Ping(bytes)))=>{if socket.send(Message::Pong(bytes)).await.is_err(){break;}continue;},Some(Ok(Message::Pong(_)))=>continue,_=>break}};
        if let Some(message) = message {
            let parsed = serde_json::from_str::<ClientMessage>(&message)
                .map_err(|_| Error::new("INVALID_REQUEST", "Message invalide.", 400));
            let result = match parsed {
                Ok(msg) => apply(&session, msg).await,
                Err(e) => Err(e),
            };
            let reply = match result {
                Ok(value) => value,
                Err(e) => Some(json!({"type":"error","code":e.code,"message":e.message})),
            };
            if let Some(value) = reply
                && !send(&mut socket, value).await
            {
                break;
            }
        }
        if heartbeat.elapsed() >= Duration::from_secs(5) {
            if !send(&mut socket, json!({"type":"heartbeat"})).await {
                break;
            }
            heartbeat = Instant::now();
        }
        let timeout = app.options.ack_timeout;
        match disk(session.clone(), move |d| poll(d, timeout)).await {
            Ok(messages) => {
                let mut close = false;
                for message in messages {
                    close |= message["type"] == "resync-required";
                    if !send(&mut socket, message).await {
                        close = true;
                        break;
                    }
                }
                if close {
                    break;
                }
            }
            Err(e) if e.status == 429 => {}
            Err(e) => {
                if !send(
                    &mut socket,
                    json!({"type":"error","code":e.code,"message":e.message}),
                )
                .await
                {
                    break;
                }
            }
        }
    }
    if *session.cancel.borrow() == generation {
        session.connected.store(false, Ordering::Relaxed);
        session.touched.store(app.now(), Ordering::Relaxed);
        let mut data = session.data.lock().await;
        data.selection = None;
        data.ack = None;
        data.branches.clear();
    }
    let _ = tokio::time::timeout(
        Duration::from_millis(200),
        socket.send(Message::Close(None)),
    )
    .await;
}
async fn apply(session: &Session, message: ClientMessage) -> Result<Option<Value>> {
    let mut data = session.data.lock().await;
    match message {
        ClientMessage::Subscribe { selection } => {
            root(&mut data, &selection.root_id)?.check_node(&selection.file_id, Kind::File)?;
            offset(&selection.offset)?;
            if selection.revision > MAX_OFFSET || selection.generation.len() > 80 {
                return Err(Error::new("INVALID_REQUEST", "Sélection invalide.", 400));
            }
            let message = json!({"type":"subscribed","revision":selection.revision,"generation":selection.generation,"offset":selection.offset});
            data.selection = Some(selection);
            data.ack = None;
            data.unavailable = None;
            Ok(Some(message))
        }
        ClientMessage::Unsubscribe => {
            data.selection = None;
            data.ack = None;
            Ok(None)
        }
        ClientMessage::Ack {
            revision,
            generation,
            end,
        } => {
            if data
                .selection
                .as_ref()
                .is_some_and(|s| s.revision == revision && s.generation == generation)
                && data.ack.as_ref().is_some_and(|a| a.end == end)
            {
                data.selection.as_mut().unwrap().offset = end;
                data.ack = None;
                Ok(None)
            } else {
                Err(Error::new("CURSOR_INVALID", "Acquittement périmé.", 409))
            }
        }
        ClientMessage::Branches { root_id, ids } => {
            if ids.len() > BRANCHES {
                return Err(Error::new("LIMIT", "Trop de dossiers ouverts.", 400));
            }
            let root = root(&mut data, &root_id)?;
            for id in &ids {
                root.check_node(id, Kind::Directory)?;
            }
            data.branches = ids
                .into_iter()
                .map(|id| {
                    let rev = data.branches.get(&id).cloned().unwrap_or_default();
                    (id, rev)
                })
                .collect();
            Ok(None)
        }
    }
}
fn poll(data: &mut Data, ack_timeout: Duration) -> Result<Vec<Value>> {
    let mut messages = Vec::new();
    if data
        .ack
        .as_ref()
        .is_some_and(|ack| ack.sent.elapsed() > ack_timeout)
    {
        return Ok(vec![
            json!({"type":"resync-required","revision":data.selection.as_ref().map(|s|s.revision).unwrap_or(0),"reason":"Client lent : reprise requise."}),
        ]);
    }
    if let Some(selected) = data.selection.clone().filter(|_| data.ack.is_none()) {
        let r = root(data, &selected.root_id)?;
        if r.relocate(&selected.file_id)? {
            messages.push(json!({"type":"file-renamed","revision":selected.revision,"entry":r.entry(&selected.file_id)?}));
        }
        let result = root(data, &selected.root_id)?.content(
            &selected.file_id,
            ContentOptions {
                generation: Some(selected.generation.clone()),
                after: Some(selected.offset.clone()),
                limit: Some(CHUNK),
                ..Default::default()
            },
        );
        match result {
            Ok(chunk) => {
                if data.unavailable.take().is_some() {
                    messages.push(json!({"type":"subscribed","revision":selected.revision,"generation":selected.generation,"offset":selected.offset}));
                }
                if chunk.end != chunk.start {
                    data.ack = Some(Ack {
                        end: chunk.end.clone(),
                        sent: Instant::now(),
                    });
                    let mut value = serde_json::to_value(chunk).unwrap();
                    value["type"] = json!("chunk");
                    value["revision"] = json!(selected.revision);
                    messages.push(value);
                }
            }
            Err(e) if e.code == "GENERATION_CHANGED" || e.code == "CURSOR_INVALID" => {
                data.selection = None;
                data.ack = None;
                messages
                    .push(json!({"type":"reset","revision":selected.revision,"reason":e.message}));
            }
            Err(e) => {
                if e.code != "NOT_FOUND" && e.code != "PERMISSION" {
                    data.selection = None;
                }
                let key = format!("{}:{}", selected.revision, e.code);
                if data.unavailable.as_deref() != Some(&key) {
                    messages.push(json!({"type":"file-unavailable","revision":selected.revision,"reason":e.message}));
                }
                data.unavailable = Some(key);
            }
        }
    }
    if data.tree_at.elapsed() >= Duration::from_secs(1) {
        data.tree_at = Instant::now();
        if let Some(root) = &data.root {
            let mut changed = vec![];
            let mut removed = vec![];
            for (id, prior) in &mut data.branches {
                match root.directory_revision(id) {
                    Ok(rev) => {
                        if *prior != rev {
                            changed.push(id.clone());
                        }
                        *prior = rev;
                    }
                    Err(_) => {
                        changed.push(id.clone());
                        removed.push(id.clone());
                    }
                }
            }
            for id in removed {
                data.branches.remove(&id);
            }
            if !changed.is_empty() {
                messages.push(json!({"type":"tree-changed","ids":changed}));
            }
        }
    }
    Ok(messages)
}
