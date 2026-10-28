//! shotpipe — seamless screenshots from your Mac to your dev servers.
//!
//! Mac side (default / `tray`): menu bar app that watches your screenshots
//! folder, mirrors it to every enabled server, and serves your clipboard over
//! per-server `ssh -R` tunnels.
//!
//! Server side: `shotpipe in` pulls the current clipboard image, `shotpipe
//! latest` prints the newest synced screenshot path.

mod client;
mod config;

#[cfg(target_os = "macos")]
mod clipboard;
#[cfg(target_os = "macos")]
mod daemon;
#[cfg(target_os = "macos")]
mod tray;

const USAGE: &str = "\
shotpipe — seamless screenshots from your Mac to your dev servers

USAGE (Mac):
  shotpipe            run the menu bar app
  shotpipe daemon     run headless (no menu bar), e.g. under launchd
  shotpipe config     print the config path (creating a default if missing)

USAGE (server):
  shotpipe in [--port N] [--token T] [--out DIR]
                      fetch the Mac's current clipboard image, print saved path
  shotpipe latest [--dir ~/shots]
                      print the newest synced screenshot path

Config: ~/.config/shotpipe/config.toml (hot-reloaded)
";

fn main() {
    std::process::exit(run());
}

/// Debug logging for the macOS-only clipboard module.
#[cfg(target_os = "macos")
]
fn log_downscale(msg: &str) {
    eprintln!("[shotpipe downscale] {msg}");
}

fn run() -> i32 {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        #[cfg(target_os = "macos")]
        None | Some("tray") => tray::run(),
        #[cfg(not(target_os = "macos"))]
        None | Some("tray") => {
            eprintln!("shotpipe: the menu bar app only runs on macOS\n");
            eprint!("{USAGE}");
            2
        }
        #[cfg(target_os = "macos")]
        Some("daemon") => {
            let _shared = daemon::start();
            loop {
                std::thread::park();
            }
        }
        Some("in") => client::cmd_in(&args[1..]),
        Some("latest") => client::cmd_latest(&args[1..]),
        Some("config") => {
            match config::Config::load_or_create() {
                Ok((_, path)) => {
                    println!("{}", path.display());
                    0
                }
                Err(e) => {
                    eprintln!("shotpipe config: {e}");
                    1
                }
            }
        }
        Some("help") | Some("--help") | Some("-h") => {
            print!("{USAGE}");
            0
        }
        Some(other) => {
            eprintln!("shotpipe: unknown command {other}\n");
            eprint!("{USAGE}");
            2
        }
    }
}
