//! The shotpipe engine: clipboard HTTP server, folder watcher, rsync fan-out,
//! per-server ssh -R tunnel supervision, config hot-reload, command handling.
//! macOS only (this is the Mac side of the pipe).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant, SystemTime};

use chrono::{DateTime, Local};
use notify::{RecursiveMode, Watcher};

use crate::clipboard;
use crate::config::{Config, ServerCfg, sh_quote};

// ---------------------------------------------------------------- state ----

#[derive(Clone, Debug, PartialEq, Default)]
pub enum TunnelState {
    #[default]
    Off, // clipboard tunnel disabled for this server
    Up,
    Down,
}

#[derive(Clone, Debug, Default)]
pub struct ServerStatus {
    pub tunnel: TunnelState,
    pub tunnel_note: String,
    pub last_sync: Option<DateTime<Local>>,
    pub last_error: Option<String>,
}

/// Commands the UI (or CLI) can send to the engine.
#[derive(Clone, Debug)]
pub enum Cmd {
    TogglePause,
    ToggleServer(String),
    SyncNow,
    OpenConfig,
    Quit,
}

pub struct Shared {
    pub cfg: RwLock<Config>,
    pub cfg_path: PathBuf,
    pub paused: AtomicBool,
    /// Bumped whenever anything visible changes; the tray polls this.
    pub dirty: AtomicU64,
    /// Bumped on config change; workers exit when their generation is stale.
    pub generation: AtomicU64,
    pub servers: Mutex<HashMap<String, ServerStatus>>,
    /// Per-server "sync now" channels, owned here so any thread can kick.
    sync_txs: Mutex<HashMap<String, Sender<()>>>,
    pub cmd_tx: Sender<Cmd>,
}

impl Shared {
    fn new(cfg: Config, cfg_path: PathBuf, cmd_tx: Sender<Cmd>) -> Arc<Self> {
        Arc::new(Self {
            cfg: RwLock::new(cfg),
            cfg_path,
            paused: AtomicBool::new(false),
            dirty: AtomicU64::new(1),
            generation: AtomicU64::new(1),
            servers: Mutex::new(HashMap::new()),
            sync_txs: Mutex::new(HashMap::new()),
            cmd_tx,
        })
    }

    pub fn is_paused(&self) -> bool {
        self.paused.load(Ordering::Relaxed)
    }

    pub fn touch(&self) {
        self.dirty.fetch_add(1, Ordering::Relaxed);
    }

    fn server_cfgs(&self) -> Vec<ServerCfg> {
        self.cfg.read().unwrap().servers.clone()
    }

    pub fn status_snapshot(&self) -> Vec<(ServerCfg, ServerStatus)> {
        let cfgs = self.server_cfgs();
        let st = self.servers.lock().unwrap();
        cfgs.into_iter()
            .map(|s| {
                let status = st.get(&s.host).cloned().unwrap_or_default();
                (s, status)
            })
            .collect()
    }

    fn set_status(&self, host: &str, f: impl FnOnce(&mut ServerStatus)) {
        {
            let mut st = self.servers.lock().unwrap();
            f(st.entry(host.to_string()).or_default());
        }
        self.touch();
    }

    /// Ask every sync worker to run a sync pass.
    pub fn kick_all(&self) {
        let txs = self.sync_txs.lock().unwrap();
        for tx in txs.values() {
            let _ = tx.send(());
        }
    }
}

/// (Re)spawn worker threads for the current config; stale ones exit on their
/// next poll when they see a newer generation.
fn respawn_workers(shared: &Arc<Shared>) {
    shared.generation.fetch_add(1, Ordering::Relaxed);
    let gen_id = shared.generation.load(Ordering::Relaxed);
    let cfgs = shared.cfg.read().unwrap().servers.clone();
    let mut rxs: Vec<(String, Receiver<()>)> = Vec::new();
    {
        let mut txs = shared.sync_txs.lock().unwrap();
        *txs = HashMap::new(); // dropping old senders stops stale workers
        for s in &cfgs {
            let (tx, rx) = std::sync::mpsc::channel::<()>();
            let _ = tx.send(()); // initial sync
            txs.insert(s.host.clone(), tx);
            rxs.push((s.host.clone(), rx));
        }
    }
    {
        let mut st = shared.servers.lock().unwrap();
        for s in &cfgs {
            st.entry(s.host.clone()).or_default();
        }
    }
    for (host, rx) in rxs {
        let this = Arc::clone(shared);
        std::thread::spawn(move || sync_worker(this, host, rx, gen_id));
    }
    for s in cfgs {
        let this = Arc::clone(shared);
        std::thread::spawn(move || tunnel_worker(this, s.host, gen_id));
    }
}

pub fn log(msg: impl AsRef<str>) {
    eprintln!(
        "[shotpipe {}] {}",
        Local::now().format("%H:%M:%S"),
        msg.as_ref()
    );
}

// ---------------------------------------------------------------- start ----

extern "C" fn on_signal(_sig: libc::c_int) {
    // Best-effort immediate exit so launchctl unload / Ctrl-C actually stop us.
    std::process::exit(0);
}

fn install_signal_handlers() {
    unsafe {
        // Note: SIGHUP intentionally NOT handled, so `nohup`/disowned
        // instances survive terminal close; default SIGHUP still applies
        // to foreground runs (Ctrl-C is SIGINT, handled above).
        libc::signal(libc::SIGTERM, on_signal as *const () as libc::sighandler_t);
        libc::signal(libc::SIGINT, on_signal as *const () as libc::sighandler_t);
    }
}

/// Start the engine and return the shared state (tray reads it, commands go
/// through `shared.cmd_tx`; a background thread consumes the receiver).
pub fn start() -> Arc<Shared> {
    install_signal_handlers();
    let (cfg, cfg_path) = Config::load_or_create().unwrap_or_else(|e| {
        log(format!(
            "config error ({e}); using defaults — fix the file, it hot-reloads"
        ));
        (Config::default(), Config::path())
    });
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
    let shared = Shared::new(cfg, cfg_path, cmd_tx);
    respawn_workers(&shared);

    spawn(clipboard_server, shared.clone());
    spawn(clipboard_mirror, shared.clone());
    spawn(temp_paste_watcher, shared.clone());
    spawn(watcher_loop, shared.clone());
    spawn(config_reloader, shared.clone());
    spawn(move |s| command_loop(s, cmd_rx), shared.clone());

    log("engine started");
    shared
}

fn spawn(f: impl FnOnce(Arc<Shared>) + Send + 'static, shared: Arc<Shared>) {
    std::thread::spawn(move || f(shared));
}

// ---------------------------------------------------------- sync worker ----

fn sync_worker(shared: Arc<Shared>, host: String, rx: Receiver<()>, gen_id: u64) {
    loop {
        if shared.generation.load(Ordering::Relaxed) != gen_id {
            return;
        }
        if rx.recv().is_err() {
            return; // senders dropped (respawn) → exit
        }
        if shared.is_paused() {
            continue;
        }
        let (cfg, server) = match lookup(&shared, &host) {
            Some(v) => v,
            None => return,
        };
        if !server.enabled || !server.sync {
            continue;
        }
        do_sync(
            &shared,
            &host,
            &cfg.general.screenshot_dir,
            &cfg.effective_remote_dir(&server),
        );
    }
}

fn lookup(shared: &Arc<Shared>, host: &str) -> Option<(Config, ServerCfg)> {
    let cfg = shared.cfg.read().unwrap().clone();
    let server = cfg.servers.iter().find(|s| s.host == host)?.clone();
    Some((cfg, server))
}

fn do_sync(shared: &Arc<Shared>, host: &str, src: &std::path::Path, remote_dir: &str) {
    let src_spec = format!("{}/", src.display());
    let dst_spec = format!("{}:{}/", host, remote_dir);

    let out = Command::new("rsync")
        .args([
            "-a",
            "--exclude",
            ".DS_Store",
            "--itemize-changes",
            "--out-format",
            "%n",
            "--timeout",
            "25",
            "-e",
            "ssh -o BatchMode=yes -o ConnectTimeout=10",
            &src_spec,
            &dst_spec,
        ])
        .stderr(Stdio::piped())
        .output();

    match out {
        Ok(o) if o.status.success() => {
            // Only actual file transfers count as a change. rsync -a also
            // restores directory mtimes (e.g. after our symlink update
            // touched the remote dir) and itemize-reports that as "." —
            // which would otherwise trigger a redundant refresh pass.
            let changed = String::from_utf8_lossy(&o.stdout).lines().any(|l| {
                let n = l.trim().trim_end_matches('/');
                !n.is_empty() && n != "."
            });
            if changed {
                refresh_latest_symlink(host, remote_dir);
            }
            shared.set_status(host, |st| {
                st.last_sync = Some(Local::now());
                st.last_error = None;
            });
        }
        Ok(o) => {
            let err = String::from_utf8_lossy(&o.stderr);
            let err = err.lines().last().unwrap_or("rsync failed").to_string();
            shared.set_status(host, |st| {
                st.last_error = Some(err.chars().take(120).collect())
            });
        }
        Err(e) => {
            shared.set_status(host, |st| st.last_error = Some(format!("rsync: {e}")));
        }
    }
}

/// Point `latest.png` at the newest image in the remote dir.
/// Glob-free so it works under any remote shell (zsh aborts on unmatched
/// globs), and `latest.png` itself is excluded so a refresh can never
/// re-select the symlink (which would create a self-reference).
fn refresh_latest_symlink(host: &str, remote_dir: &str) {
    let remote = format!(
        "cd {dir} && f=$(ls -t 2>/dev/null | grep -Ei '\\.(png|jpe?g|webp|gif)$' | grep -Ev '^latest\\.png$' | head -n1) && [ -n \"$f\" ] && ln -sfn \"$f\" latest.png",
        dir = sh_quote(remote_dir)
    );
    let _ = Command::new("ssh")
        .args([
            "-o",
            "BatchMode=yes",
            "-o",
            "ConnectTimeout=10",
            host,
            &remote,
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

// -------------------------------------------------------- tunnel worker ----

struct ChildGuard(Option<std::process::Child>);

impl ChildGuard {
    fn kill(&mut self) {
        if let Some(mut c) = self.0.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        self.kill();
    }
}

fn tunnel_worker(shared: Arc<Shared>, host: String, gen_id: u64) {
    let mut child = ChildGuard(None);
    let mut next_try = Instant::now();
    let mut backoff = 2u64;

    loop {
        std::thread::sleep(Duration::from_millis(900));
        if shared.generation.load(Ordering::Relaxed) != gen_id {
            return; // ChildGuard drops → ssh killed
        }
        let (cfg, server) = match lookup(&shared, &host) {
            Some(v) => v,
            None => return,
        };
        let want = server.enabled && server.clipboard && !shared.is_paused();
        let port = cfg.general.clipboard_port;

        if !want {
            if child.0.is_some() {
                child.kill();
                shared.set_status(&host, |st| {
                    st.tunnel = TunnelState::Off;
                    st.tunnel_note.clear();
                });
            }
            continue;
        }

        // Reap exited children.
        if let Some(c) = child.0.as_mut() {
            match c.try_wait() {
                Ok(Some(_status)) => {
                    shared.set_status(&host, |st| {
                        st.tunnel = TunnelState::Down;
                        st.tunnel_note = "reconnecting".into();
                    });
                    child.0 = None;
                }
                Ok(None) => {}
                Err(_) => child.0 = None,
            }
        }

        // Spawn/restart with backoff.
        if child.0.is_none() && Instant::now() >= next_try {
            let fwd = format!("127.0.0.1:{port}:127.0.0.1:{port}");
            match Command::new("ssh")
                .args([
                    "-N",
                    "-T",
                    "-o",
                    "BatchMode=yes",
                    "-o",
                    "ExitOnForwardFailure=yes",
                    "-o",
                    "ConnectTimeout=10",
                    "-o",
                    "ServerAliveInterval=15",
                    "-o",
                    "ServerAliveCountMax=3",
                    "-R",
                    &fwd,
                    &host,
                ])
                .stdout(Stdio::null())
                .stderr(Stdio::piped())
                .spawn()
            {
                Ok(c) => {
                    backoff = 2;
                    child.0 = Some(c);
                    shared.set_status(&host, |st| {
                        st.tunnel = TunnelState::Up;
                        st.tunnel_note.clear();
                    });
                }
                Err(e) => {
                    shared.set_status(&host, |st| {
                        st.tunnel = TunnelState::Down;
                        st.tunnel_note = format!("spawn: {e}").chars().take(80).collect();
                    });
                }
            }
            next_try = Instant::now() + Duration::from_secs(backoff);
            backoff = (backoff * 2).min(30);
        }
    }
}

// ------------------------------------------------------ clipboard server ---

fn clipboard_server(shared: Arc<Shared>) {
    let port = shared.cfg.read().unwrap().general.clipboard_port;
    let server = match tiny_http::Server::http(("127.0.0.1", port)) {
        Ok(s) => s,
        Err(e) => {
            log(format!(
                "clipboard server: cannot bind 127.0.0.1:{port}: {e}"
            ));
            return;
        }
    };
    log(format!("clipboard server on 127.0.0.1:{port}"));

    for request in server.incoming_requests() {
        let method = request.method().clone();
        let url = request.url().to_string();
        let token_ok = {
            let want = shared.cfg.read().unwrap().general.token.clone();
            want.is_empty()
                || request
                    .headers()
                    .iter()
                    .any(|h| h.field.equiv("X-Shotpipe-Token") && h.value.as_str() == want)
        };

        let (ds, maxw) = {
            let g = &shared.cfg.read().unwrap().general.downscale;
            (g.enabled, g.max_width)
        };

        let resp = match (method, url.as_str()) {
            (tiny_http::Method::Get, "/health") if token_ok => text_response(200, "ok"),
            (tiny_http::Method::Get, u) if u.starts_with("/image.png") && token_ok => {
                match clipboard::clipboard_png_processed(ds, maxw) {
                    Some(png) => data_response(png),
                    None => text_response(404, "no image in clipboard"),
                }
            }
            (_, _) if !token_ok => text_response(401, "bad token"),
            _ => text_response(404, "not found"),
        };
        let _ = request.respond(resp);
    }
}

fn text_response(code: u16, body: &str) -> tiny_http::Response<std::io::Cursor<Vec<u8>>> {
    tiny_http::Response::from_data(body.as_bytes().to_vec()).with_status_code(code)
}

fn data_response(png: Vec<u8>) -> tiny_http::Response<std::io::Cursor<Vec<u8>>> {
    tiny_http::Response::from_data(png)
        .with_header(
            tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"image/png"[..]).unwrap(),
        )
        .with_status_code(200)
}

// ------------------------------------------------- clipboard mirroring ---

/// Watch the Mac clipboard; whenever it changes and holds an image, save it
/// into the watched screenshots folder. The folder watcher then syncs it to
/// every enabled server and refreshes `latest.png`. This makes clipboard-only
/// habits (Cmd+Shift+5 → Cmd+C) work with `shotpipe latest` too.
fn clipboard_mirror(shared: Arc<Shared>) {
    // Baseline: don't re-mirror whatever is already on the clipboard at start.
    let mut last_cc = clipboard::change_count();
    loop {
        std::thread::sleep(Duration::from_millis(900));
        if shared.is_paused() {
            continue;
        }
        let cfg = shared.cfg.read().unwrap().clone();
        if !cfg.general.mirror_clipboard {
            continue;
        }
        let cc = clipboard::change_count();
        if cc == last_cc {
            continue;
        }
        last_cc = cc;

        if let Some(png) = clipboard::read_clipboard_png() {
            let stamp = Local::now()
                .format("%Y%m%d-%H%M%S%.3f")
                .to_string()
                .replace('.', "-");
            let path = cfg.general.screenshot_dir.join(format!("clip-{stamp}.png"));
            match std::fs::write(&path, &png) {
                Ok(()) => {
                    log(format!("mirrored clipboard image → {}", path.display()));
                    shared.kick_all(); // in case FSEvents is slow to notice
                }
                Err(e) => log(format!("mirror write failed: {e}")),
            }
        }
    }
}

// ------------------------------------------------- temp paste capture ----

/// When you paste an image into a terminal on the Mac, many terminals save it
/// to a temp file (~/.../T/clipboard-2026-10-30-235118-44FCE021.png) and paste
/// the PATH into the remote session. We watch macOS temp dirs for such files
/// and copy them into the synced folder with the SAME basename, so on the
/// server `shotpipe resolve <pasted-path>` finds them by name.
fn temp_paste_watcher(shared: Arc<Shared>) {
    let (tx, rx) = std::sync::mpsc::channel::<notify::Result<notify::Event>>();
    let mut watcher = match notify::recommended_watcher(move |res| {
        let _ = tx.send(res);
    }) {
        Ok(w) => w,
        Err(e) => {
            log(format!("paste watcher init failed: {e}"));
            return;
        }
    };
    let mut watched_any = false;
    for dir in ["/var/folders", "/tmp", "/private/tmp"] {
        if Path::new(dir).exists() {
            match watcher.watch(Path::new(dir), RecursiveMode::Recursive) {
                Ok(()) => watched_any = true,
                Err(e) => log(format!("paste watcher: cannot watch {dir}: {e}")),
            }
        }
    }
    if !watched_any {
        return;
    }

    let is_paste_image = |p: &std::path::Path| -> bool {
        p.file_name()
            .and_then(|n| n.to_str())
            .map(|n| {
                (n.starts_with("clipboard-") || n.starts_with("image-"))
                    && matches!(
                        p.extension().and_then(|e| e.to_str()),
                        Some("png" | "jpg" | "jpeg")
                    )
            })
            .unwrap_or(false)
    };

    loop {
        // First event, then drain briefly to batch.
        let mut candidates: Vec<std::path::PathBuf> = Vec::new();
        if let Ok(Ok(ev)) = rx.recv() {
            candidates.extend(ev.paths.iter().filter(|p| is_paste_image(p)).cloned())
        }
        if rx.recv().is_err() {
            return;
        }
        loop {
            match rx.recv_timeout(Duration::from_millis(300)) {
                Ok(Ok(ev)) => {
                    candidates.extend(ev.paths.iter().filter(|p| is_paste_image(p)).cloned())
                }
                Ok(Err(_)) => continue,
                Err(_) => break,
            }
        }

        if shared.is_paused() {
            continue;
        }
        let cfg = shared.cfg.read().unwrap().clone();
        if !cfg.general.paste_capture {
            continue;
        }
        let dest_dir = cfg.general.screenshot_dir.clone();
        let mut copied = false;
        for src in candidates {
            if !src.is_file() {
                continue;
            }
            let Some(name) = src.file_name() else {
                continue;
            };
            let dest = dest_dir.join(name);
            match std::fs::copy(&src, &dest) {
                Ok(_) => {
                    log(format!(
                        "captured paste image {} → {}",
                        src.display(),
                        dest.display()
                    ));
                    copied = true;
                }
                Err(e) => log(format!("paste capture copy failed: {e}")),
            }
        }
        if copied {
            shared.kick_all(); // push immediately; folder watcher will follow up too
        }
    }
}

// ------------------------------------------------------------- watcher -----

fn watcher_loop(shared: Arc<Shared>) {
    let mut src = shared.cfg.read().unwrap().general.screenshot_dir.clone();
    let _ = std::fs::create_dir_all(&src);
    let (tx, rx) = std::sync::mpsc::channel::<notify::Result<notify::Event>>();

    let mut watcher = match notify::recommended_watcher(move |res| {
        let _ = tx.send(res);
    }) {
        Ok(w) => w,
        Err(e) => {
            log(format!("watcher init failed: {e}"));
            return;
        }
    };
    if let Err(e) = watcher.watch(&src, RecursiveMode::NonRecursive) {
        log(format!("cannot watch {}: {e}", src.display()));
        return;
    }
    log(format!("watching {}", src.display()));

    loop {
        // Wait for the first event, then drain until quiet (debounce).
        if rx.recv().is_err() {
            return;
        }
        while rx.recv_timeout(Duration::from_millis(400)).is_ok() {}

        // Follow screenshot_dir changes without a restart.
        let cfg_dir = shared.cfg.read().unwrap().general.screenshot_dir.clone();
        if cfg_dir != src {
            let _ = std::fs::create_dir_all(&cfg_dir);
            let _ = watcher.unwatch(&src);
            match watcher.watch(&cfg_dir, RecursiveMode::NonRecursive) {
                Ok(()) => {
                    src = cfg_dir;
                    log(format!("now watching {}", src.display()));
                }
                Err(e) => log(format!("cannot watch {}: {e}", cfg_dir.display())),
            }
        }

        if !shared.is_paused() {
            shared.kick_all();
        }
    }
}

// ------------------------------------------------------- config reload -----

fn config_reloader(shared: Arc<Shared>) {
    let mut last_mtime: Option<SystemTime> = None;
    loop {
        std::thread::sleep(Duration::from_secs(1));
        let mtime = shared.cfg_path.metadata().and_then(|m| m.modified()).ok();
        if mtime == last_mtime {
            continue;
        }
        last_mtime = mtime;
        match Config::load(&shared.cfg_path) {
            Ok(new_cfg) => {
                let servers_changed = {
                    let mut cfg = shared.cfg.write().unwrap();
                    let changed = cfg.servers != new_cfg.servers;
                    *cfg = new_cfg;
                    changed
                };
                if servers_changed {
                    respawn_workers(&shared);
                    log("config reloaded; workers respawned");
                } else {
                    log("config reloaded");
                }
                shared.touch();
            }
            Err(e) => log(format!("config reload failed, keeping previous: {e}")),
        }
    }
}

// ------------------------------------------------------- command loop ------

fn command_loop(shared: Arc<Shared>, cmd_rx: Receiver<Cmd>) {
    while let Ok(cmd) = cmd_rx.recv() {
        match cmd {
            Cmd::TogglePause => {
                let now_paused = !shared.is_paused();
                shared.paused.store(now_paused, Ordering::Relaxed);
                log(if now_paused { "paused" } else { "resumed" });
                if !now_paused {
                    shared.kick_all(); // catch up on files saved while paused
                }
                shared.touch();
            }
            Cmd::ToggleServer(host) => {
                let mut now_enabled = None;
                {
                    let mut cfg = shared.cfg.write().unwrap();
                    if let Some(s) = cfg.servers.iter_mut().find(|s| s.host == host) {
                        s.enabled = !s.enabled;
                        now_enabled = Some(s.enabled);
                    }
                }
                if let Some(on) = now_enabled {
                    let cfg_snapshot = shared.cfg.read().unwrap().clone();
                    if let Err(e) = cfg_snapshot.save(&shared.cfg_path) {
                        log(format!("config save failed: {e}"));
                    }
                    log(format!(
                        "server {host}: {}",
                        if on { "enabled" } else { "disabled" }
                    ));
                    respawn_workers(&shared);
                    shared.touch();
                }
            }
            Cmd::SyncNow => {
                log("manual sync");
                shared.kick_all();
            }
            Cmd::OpenConfig => {
                let _ = Command::new("open").arg("-t").arg(&shared.cfg_path).spawn();
            }
            Cmd::Quit => {
                log("bye");
                std::process::exit(0);
            }
        }
    }
}
