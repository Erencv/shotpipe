//! macOS menu bar UI: status icon + menu, built on winit + tray-icon.
//! The menu is rebuilt from engine state whenever it changes.

#![allow(deprecated)] // winit `run` + tray set_icon_as_template are deprecated but functional

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon as TrayIconIcon, TrayIconBuilder};
use winit::event::{Event, StartCause};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::platform::macos::{ActivationPolicy, EventLoopBuilderExtMacOS};

use crate::config::home;
use crate::daemon::{self, Cmd, Shared, TunnelState};

#[derive(Debug)]
enum Msg {
    Menu(String),
}

pub fn run() -> i32 {
    let shared = daemon::start();

    let mut builder = EventLoop::<Msg>::with_user_event();
    builder.with_activation_policy(ActivationPolicy::Accessory);
    let event_loop: EventLoop<Msg> = builder.build().expect("event loop");

    let proxy = event_loop.create_proxy();
    MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
        let _ = proxy.send_event(Msg::Menu(e.id.0.clone()));
    }));

    let tray = TrayIconBuilder::new()
        .with_tooltip("shotpipe")
        .build()
        .expect("tray icon");
    let _ = tray.set_icon_templated(Some(make_icon(false)));

    let mut menu_version: u64 = 0;
    let mut icon_paused: Option<bool> = None;
    let cmd_tx = shared.cmd_tx.clone();

    let result = event_loop.run(move |event, elwt| match event {
        Event::NewEvents(StartCause::Init) => {
            elwt.set_control_flow(ControlFlow::Wait);
        }
        Event::UserEvent(Msg::Menu(id)) => {
            let cmd = match id.as_str() {
                "pause" => Some(Cmd::TogglePause),
                "syncnow" => Some(Cmd::SyncNow),
                "openconfig" => Some(Cmd::OpenConfig),
                "quit" => Some(Cmd::Quit),
                "loginitem" => {
                    let now = !login_item_installed();
                    let ok = set_login_item(now);
                    daemon::log(if ok {
                        if now {
                            "login item installed"
                        } else {
                            "login item removed"
                        }
                    } else {
                        "login item change failed"
                    });
                    None
                }
                other => other
                    .strip_prefix("srv:")
                    .map(|host| Cmd::ToggleServer(host.to_string())),
            };
            if let Some(cmd) = cmd {
                let _ = cmd_tx.send(cmd);
                shared.touch(); // force a menu/icon refresh
            }
        }
        Event::AboutToWait => {
            let v = shared.dirty.load(Ordering::Relaxed);
            if v != menu_version {
                menu_version = v;
                if let Some(menu) = build_menu(&shared) {
                    tray.set_menu(Some(Box::new(menu)));
                }
                let paused = shared.is_paused();
                if icon_paused != Some(paused) {
                    icon_paused = Some(paused);
                    let _ = tray.set_icon_templated(Some(make_icon(paused)));
                }
            }
            elwt.set_control_flow(ControlFlow::WaitUntil(
                Instant::now() + Duration::from_millis(300),
            ));
        }
        _ => {}
    });
    match result {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("shotpipe: event loop error: {e}");
            1
        }
    }
}

fn build_menu(shared: &Arc<Shared>) -> Option<Menu> {
    let paused = shared.is_paused();
    let menu = Menu::new();

    let pause = MenuItem::with_id(
        "pause",
        if paused {
            "▶  Resume all"
        } else {
            "⏸  Pause all"
        },
        true,
        None,
    );
    menu.append(&pause).ok()?;

    menu.append(&PredefinedMenuItem::separator()).ok()?;

    let servers = shared.status_snapshot();
    if servers.is_empty() {
        let empty = MenuItem::with_id(
            "openconfig",
            "No servers configured — open config…",
            true,
            None,
        );
        menu.append(&empty).ok()?;
    }
    for (s, st) in servers {
        let mut text = s.label().to_string();
        if s.enabled {
            let tunnel = match st.tunnel {
                TunnelState::Up => "tunnel ✓",
                TunnelState::Down => "tunnel ✗",
                TunnelState::Off => "no tunnel",
            };
            let synced = st
                .last_sync
                .map(|t| format!("synced {}", t.format("%H:%M")))
                .unwrap_or_else(|| "not synced".into());
            text = format!("{text}  ·  {tunnel} · {synced}");
            if let Some(err) = &st.last_error {
                text = format!("{text}  ·  {err}");
            }
        } else {
            text = format!("{text}  ·  off");
        }
        let item = CheckMenuItem::with_id(format!("srv:{}", s.host), text, true, s.enabled, None);
        menu.append(&item).ok()?;
    }

    menu.append(&PredefinedMenuItem::separator()).ok()?;

    let sync_now = MenuItem::with_id("syncnow", "⟳  Sync now", !paused, None);
    menu.append(&sync_now).ok()?;

    let login = CheckMenuItem::with_id(
        "loginitem",
        "Start at login",
        true,
        login_item_installed(),
        None,
    );
    menu.append(&login).ok()?;

    menu.append(&PredefinedMenuItem::separator()).ok()?;

    let open_cfg = MenuItem::with_id("openconfig", "Open config…", true, None);
    menu.append(&open_cfg).ok()?;

    let quit = MenuItem::with_id("quit", "Quit shotpipe", true, None);
    menu.append(&quit).ok()?;

    Some(menu)
}

// --------------------------------------------------------------- icon ------

/// Menu bar icon (template image): screenshot brackets with a filled dot
/// when active, hollow ring when paused. Matches the app icon motif.
fn make_icon(active: bool) -> TrayIconIcon {
    let s = 36usize; // 18pt @2x
    let m = 3usize; // outer margin
    let t = 4usize; // stroke thickness
    let a = 10usize; // bracket arm length
    let mut rgba = vec![0u8; s * s * 4];
    let in_rect =
        |x: i32, y: i32, x0: i32, y0: i32, x1: i32, y1: i32| x >= x0 && x < x1 && y >= y0 && y < y1;
    let lo = m as i32;
    let hi = (s - m) as i32;
    let corners = [
        (lo, lo, 1, 1),
        (hi, lo, -1, 1),
        (lo, hi, 1, -1),
        (hi, hi, -1, -1),
    ];
    let c = (s as f32 - 1.0) / 2.0;
    for y in 0..s {
        for x in 0..s {
            let (xi, yi) = (x as i32, y as i32);
            let mut on = false;
            for &(bx, by, sx, sy) in &corners {
                let cx0 = if sx > 0 { bx } else { bx - t as i32 };
                let cy0 = if sy > 0 { by } else { by - t as i32 };
                let ax0 = if sx > 0 { bx } else { bx - a as i32 };
                let ax1 = if sx > 0 { bx + a as i32 } else { bx };
                let ay0 = if sy > 0 { by } else { by - a as i32 };
                let ay1 = if sy > 0 { by + a as i32 } else { by };
                if in_rect(xi, yi, ax0.min(ax1), cy0, ax0.max(ax1), cy0 + t as i32)
                    || in_rect(xi, yi, cx0, ay0.min(ay1), cx0 + t as i32, ay0.max(ay1))
                {
                    on = true;
                    break;
                }
            }
            // center dot: filled when active, ring when paused
            let d = ((xi as f32 - c) * (xi as f32 - c) + (yi as f32 - c) * (yi as f32 - c)).sqrt();
            if active {
                if d <= 4.5 {
                    on = true;
                }
            } else if (d - 4.2).abs() <= 1.5 {
                on = true;
            }
            if on {
                rgba[(y * s + x) * 4 + 3] = 255;
            }
        }
    }
    TrayIconIcon::from_rgba(rgba, s as u32, s as u32).expect("icon")
}

// ----------------------------------------------------------- login item ----

fn plist_path() -> std::path::PathBuf {
    home()
        .join("Library")
        .join("LaunchAgents")
        .join("com.shotpipe.plist")
}

fn login_item_installed() -> bool {
    plist_path().exists()
}

/// Install/uninstall the LaunchAgent. Called directly from the menu handler
/// thread (winit main thread is busy in its loop).
fn set_login_item(on: bool) -> bool {
    let plist = plist_path();
    if on {
        let Ok(exe) = std::env::current_exe() else {
            return false;
        };
        let exe = exe.display().to_string().replace('"', "");
        let log = std::env::temp_dir()
            .join("shotpipe.log")
            .display()
            .to_string();
        let xml = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key><string>com.shotpipe</string>
    <key>ProgramArguments</key>
    <array><string>{exe}</string><string>tray</string></array>
    <key>RunAtLoad</key><true/>
    <key>StandardOutPath</key><string>{log}</string>
    <key>StandardErrorPath</key><string>{log}</string>
</dict>
</plist>
"#
        );
        if let Some(parent) = plist.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if std::fs::write(&plist, xml).is_err() {
            return false;
        }
        let _ = std::process::Command::new("launchctl")
            .args(["unload", "-w"])
            .arg(&plist)
            .status();
        std::process::Command::new("launchctl")
            .args(["load", "-w"])
            .arg(&plist)
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    } else {
        let _ = std::process::Command::new("launchctl")
            .args(["unload", "-w"])
            .arg(&plist)
            .status();
        let _ = std::fs::remove_file(&plist);
        true
    }
}
