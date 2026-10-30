//! `shotpipe in` / `shotpipe latest` — the server-side (client) commands.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use chrono::Local;

use crate::config::expand_tilde;

const IMAGE_EXTS: [&str; 6] = ["png", "PNG", "jpg", "jpeg", "JPG", "webp"];

/// Fetch the image currently on the laptop's clipboard through the SSH tunnel
/// and save it locally. Prints the saved path on success.
pub fn cmd_in(args: &[String]) -> i32 {
    let mut port: u16 = std::env::var("SHOTPIPE_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(8777);
    let mut token = std::env::var("SHOTPIPE_TOKEN").unwrap_or_default();
    let mut out_dir = PathBuf::from("/tmp");

    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--port" => match it.next().and_then(|v| v.parse().ok()) {
                Some(p) => port = p,
                None => {
                    eprintln!("shotpipe in: --port needs a number");
                    return 2;
                }
            },
            "--token" => match it.next() {
                Some(t) => token = t.clone(),
                None => {
                    eprintln!("shotpipe in: --token needs a value");
                    return 2;
                }
            },
            "--out" => match it.next() {
                Some(d) => out_dir = expand_tilde(Path::new(d)),
                None => {
                    eprintln!("shotpipe in: --out needs a directory");
                    return 2;
                }
            },
            other => {
                eprintln!("shotpipe in: unknown argument {other}");
                return 2;
            }
        }
    }

    let url = format!("http://127.0.0.1:{port}/image.png");
    let mut req = ureq::get(&url);
    if !token.is_empty() {
        req = req.header("X-Shotpipe-Token", &token);
    }
    let req = req
        .config()
        .timeout_global(Some(std::time::Duration::from_secs(20)))
        .build();

    let resp = match req.call() {
        Ok(r) => r,
        Err(ureq::Error::StatusCode(404)) => {
            eprintln!("shotpipe in: no image in clipboard");
            return 1;
        }
        Err(ureq::Error::StatusCode(401)) => {
            eprintln!("shotpipe in: bad token (set --token or SHOTPIPE_TOKEN)");
            return 1;
        }
        Err(e) => {
            eprintln!(
                "shotpipe in: tunnel unreachable — is the Mac running shotpipe and the ssh -R tunnel up? ({e})"
            );
            return 1;
        }
    };
    if resp.status().as_u16() != 200 {
        eprintln!("shotpipe in: unexpected status {}", resp.status());
        return 1;
    }
    let bytes = match resp.into_body().read_to_vec() {
        Ok(b) => b,
        Err(e) => {
            eprintln!("shotpipe in: read body: {e}");
            return 1;
        }
    };
    if !bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        eprintln!("shotpipe in: response is not a PNG");
        return 1;
    }

    let stamp = Local::now()
        .format("%Y%m%d-%H%M%S%.3f")
        .to_string()
        .replace('.', "-");
    let path = out_dir.join(format!("clip-{stamp}.png"));
    if let Err(e) = std::fs::write(&path, &bytes) {
        eprintln!("shotpipe in: write {}: {e}", path.display());
        return 1;
    }
    println!("{}", path.display());
    0
}

/// Print the path of the newest synced screenshot (~/shots/latest.png by default).
pub fn cmd_latest(args: &[String]) -> i32 {
    let mut dir: Option<PathBuf> = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--dir" => match it.next() {
                Some(d) => dir = Some(expand_tilde(Path::new(d))),
                None => {
                    eprintln!("shotpipe latest: --dir needs a path");
                    return 2;
                }
            },
            other => {
                eprintln!("shotpipe latest: unknown argument {other}");
                return 2;
            }
        }
    }
    let dir = dir.unwrap_or_else(|| crate::config::home().join("shots"));

    // Prefer the maintained symlink, fall back to newest file by mtime.
    let latest = dir.join("latest.png");
    if latest.exists() {
        println!("{}", latest.display());
        return 0;
    }
    match newest_image(&dir) {
        Some(p) => {
            println!("{}", p.display());
            0
        }
        None => {
            eprintln!(
                "shotpipe latest: no images in {} (is the Mac-side shotpipe running?)",
                dir.display()
            );
            1
        }
    }
}

/// Resolve a path pasted from the Mac (e.g. /var/folders/.../T/clipboard-*.png,
/// produced by terminals when you Cmd+V an image) to the synced copy in
/// ~/shots/. Waits briefly for the sync to land. Prints the local path.
pub fn cmd_resolve(args: &[String]) -> i32 {
    let mut path: Option<String> = None;
    let mut dir: Option<PathBuf> = None;
    let mut wait_secs: u64 = 8;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--dir" => match it.next() {
                Some(d) => dir = Some(expand_tilde(Path::new(d))),
                None => {
                    eprintln!("shotpipe resolve: --dir needs a path");
                    return 2;
                }
            },
            "--wait" => match it.next().and_then(|v| v.parse().ok()) {
                Some(w) => wait_secs = w,
                None => {
                    eprintln!("shotpipe resolve: --wait needs seconds");
                    return 2;
                }
            },
            other if path.is_none() => path = Some(other.to_string()),
            other => {
                eprintln!("shotpipe resolve: unexpected argument {other}");
                return 2;
            }
        }
    }
    let Some(path) = path else {
        eprintln!("usage: shotpipe resolve <pasted-path> [--dir DIR] [--wait SECS]");
        return 2;
    };

    let dir = dir.unwrap_or_else(|| crate::config::home().join("shots"));
    let base = Path::new(&path)
        .file_name()
        .map(|b| b.to_string_lossy().to_string())
        .unwrap_or_else(|| path.clone());
    let candidate = dir.join(&base);

    // The paste can race the sync by a couple of seconds; retry.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(wait_secs);
    loop {
        if candidate.exists() {
            println!("{}", candidate.display());
            return 0;
        }
        if std::time::Instant::now() >= deadline {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(400));
    }

    // Fall back to the newest synced image, noting the substitution.
    if let Some(p) = newest_image(&dir) {
        eprintln!(
            "shotpipe resolve: {base} not synced yet; using newest: {}",
            p.display()
        );
        println!("{}", p.display());
        return 0;
    }
    eprintln!(
        "shotpipe resolve: nothing matching {base} in {} after {wait_secs}s",
        dir.display()
    );
    1
}

fn newest_image(dir: &Path) -> Option<PathBuf> {
    let mut best: Option<(SystemTime, PathBuf)> = None;
    for entry in std::fs::read_dir(dir).ok()? {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let is_img = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| IMAGE_EXTS.contains(&e))
            .unwrap_or(false);
        if !is_img || entry.file_name().to_string_lossy() == "latest.png" {
            continue;
        }
        let mtime = match entry.metadata().and_then(|m| m.modified()) {
            Ok(t) => t,
            Err(_) => continue,
        };
        if best.as_ref().map(|(t, _)| mtime > *t).unwrap_or(true) {
            best = Some((mtime, path));
        }
    }
    best.map(|(_, p)| p)
}
