# shotpipe

Seamless screenshots from your Mac to your dev servers — for people who run
CLI agents (pi, codex, claude code, opencode…) over SSH.

**Before:** screenshot → save → scp → paste path into the agent.
**After:** just paste the image, or say *"look at my screen"*.

One Rust binary, two roles:

- **Mac (default / `tray`)** — a menu bar app that:
  - watches your screenshots folder and mirrors it to every enabled server,
    keeping a `latest.png` symlink pointing at the newest image,
  - serves your clipboard over per-server `ssh -R` tunnels,
  - gives you a global pause switch, per-server toggles, manual sync,
    start-at-login, all in the menu bar.
- **Server (`shotpipe in` / `shotpipe latest`)** — pulls the current clipboard
  image or prints the newest synced screenshot path.

```
┌────────────── macOS ──────────────┐          ┌──────────── server ────────────┐
│ ~/Screenshots ───rsync──▶──────────┼──ssh───▶ │ ~/shots/                        │
│ (FSEvents watcher)                │          │   └─ latest.png → newest        │
│                                   │          │                                 │
│ clipboard ──▶ 127.0.0.1:8777 ─ssh -R───────▶ │ 127.0.0.1:8777                  │
│ (NSPasteboard → PNG)              │          │   └─ `shotpipe in`              │
│                                   │          │                                 │
│ paste-image temp files            │          │  `shotpipe resolve <pasted>`    │
│ (/var/folders/...T/clipboard-*) ─▶│────────▶ │  → ~/shots/<same basename>     │
└───────────────────────────────────┘          └─────────────────────────────────┘
```

Three capture flows, all hands-off:

| What you do on the Mac | What lands on the server |
|---|---|
| `Cmd+V` an image into the agent's terminal | the pasted path just works (`shotpipe resolve`) |
| `Cmd+Shift+5 → Cmd+C` (clipboard only) | `~/shots/clip-*.png` + `latest.png` |
| `Cmd+Shift+4` / any file in the watched folder | `~/shots/<name>` + `latest.png` |

## Quick start

### Mac

Either run the binary directly:

```bash
cargo build --release
# point macOS screenshots at a dedicated folder (one-time)
mkdir -p ~/Screenshots
defaults write com.apple.screencapture location ~/Screenshots && killall SystemUIServer
# run it (a menu bar icon appears; default config is created at
# ~/.config/shotpipe/config.toml with a `dev` server entry to edit)
./target/release/shotpipe
```

Or build a proper app bundle with the icon and drag it into `/Applications`:

```bash
scripts/make-app.sh   # → Shotpipe.app
```

Use **Start at login** in the menu to install a LaunchAgent automatically.

### Each server

Build the same crate on the server (it skips all macOS-only dependencies):

```bash
rsync -a --exclude target --exclude .git ./ server:shotpipe-src/
ssh server 'cd shotpipe-src && cargo build --release'
ssh server 'cp <target-dir>/release/shotpipe ~/.local/bin/'   # CARGO_TARGET_DIR may relocate it
```

Nothing to configure on the server: the tunnel is established by the Mac.

### Tell your agents

Add to `AGENTS.md` / `CLAUDE.md` on the server (and a pi skill if you like —
[skill template](docs/pi-skill.md)):

> - If I paste a file path that doesn't exist on this machine (usually a Mac
>   path like `/var/folders/.../T/clipboard-*.png`), run `shotpipe resolve <path>`
>   and use the path it prints.
> - `shotpipe in` — saves the image currently on my Mac's clipboard to /tmp and prints the path.
> - `~/shots/latest.png` — newest synced screenshot (older ones next to it, `ls -t`).
>
> When I mention a screenshot or say "look at this", fetch the image with the
> above, then read the resulting path as an image.

## Config

`~/.config/shotpipe/config.toml` — hot-reloaded on save; the menu bar toggles
rewrite it, so the file is always the source of truth. Override the directory
with `SHOTPIPE_CONFIG_DIR` (used by the tests).

```toml
[general]
screenshot_dir    = "~/Screenshots" # watched folder (follows config changes live)
remote_dir        = "shots"          # destination, relative to $HOME on the server
clipboard_port    = 8777             # local server + remote forwarded port
token             = ""               # optional shared secret for the clipboard server
mirror_clipboard  = true             # auto-save copied images into the synced folder
paste_capture     = true             # capture terminal paste-images (/var/folders/...)
downscale         = { enabled = true, max_width = 1600 }  # retina → agent-sized PNGs

[[servers]]
host    = "dev"                   # anything ssh accepts: alias, user@host, jumps
name    = "dev"                   # optional menu label
enabled = true                    # toggled by the menu bar checkbox
sync      = true                  # mirror the screenshots folder
clipboard = true                  # maintain an ssh -R clipboard tunnel
remote_dir = "shots"              # optional per-server override

[[servers]]
host    = "user@other.example.com"
enabled = false
```

Requirements: key-based SSH auth to each server (the app never prompts), and
`rsync` on both ends. `mosh` does not forward ports — if you use mosh, keep a
separate `ssh -N -R 8777:127.0.0.1:8777 <host>` connection.

## Menu bar

```
[ ]  screenshot brackets; filled dot = running, hollow ring = paused
 ├─ ⏸ Pause all              # global on/off (kills tunnels, pauses sync)
 ├─ ✓ dev  ·  tunnel ✓ · synced 14:32
 ├─ ✗ box  ·  off
 ├─ ⟳ Sync now
 ├─ Start at login
 ├─ Open config…
 └─ Quit shotpipe
```

## Security notes

- The clipboard server binds `127.0.0.1` on the Mac and reaches the server only
  through your SSH connection.
- On multi-user servers the tunnel lands on `localhost:8777` — other local
  users could read your clipboard there. Set `token` in the config (and
  `SHOTPIPE_TOKEN` / `--token` on the server side) to require auth.
- Images are downscaled to `max_width` before leaving the Mac (configurable).

## Development

```bash
cargo test                                     # unit tests
cargo build                                    # debug build (Mac)
SHOTPIPE_CONFIG_DIR=/tmp/x ./target/debug/shotpipe daemon   # headless engine
```

Verified end-to-end against a real server: initial sync, watcher-triggered
sync, `latest.png` retargeting, tunnel health, token rejection (401),
`shotpipe in` pulling the live Mac clipboard through the tunnel, clipboard
mirroring, and `shotpipe resolve` mapping a pasted `/var/folders/...` path
to the synced copy.

## Layout

```
src/main.rs       dispatch + usage (cross-platform)
src/config.rs     TOML config, hot-reload-friendly (cross-platform)
src/client.rs     `in` / `latest` / `resolve` server commands (cross-platform)
src/clipboard.rs  NSPasteboard → PNG (+TIFF decode, downscale, cache)   [macOS]
src/daemon.rs     engine: watchers, rsync fan-out, tunnels, HTTP, reload [macOS]
src/tray.rs       menu bar icon + menu, login item                       [macOS]
scripts/make-icons.py  regenerate assets/Shotpipe.icns (pure stdlib)
scripts/make-app.sh    build Shotpipe.app with icon + ad-hoc signature
```

## License

[MIT](LICENSE)
