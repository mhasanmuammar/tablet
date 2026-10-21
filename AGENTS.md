# AGENTS.md

GNOME/Wayland tablet driver: `tabletforged` daemon (owns the tablet, re-emits it as
virtual uinput devices) + a Tauri 2 settings app ("TabletForge") + a GNOME
Shell extension. This repo develops and ships the product directly: CI
publishes the pre-built release archives and the Flatpak bundle.
README.md is thorough — read it for features and install steps; this file
covers what agents tend to get wrong.

## Brand identity

- Product: **TabletForge**; app ID `org.tabletforge.TabletForge`.
- Daemon/binary `tabletforged`, unit `tabletforged.service`, socket
  `tabletforged.sock`, config `~/.config/tabletforge/`, udev rule
  `70-tabletforge.rules`, extension `tabletforge@tabletforge.org`, DBus
  `org.tabletforge.Shell`.
- Pre-rebrand installs used `wfpaisa/tablet`-derived names (`tabletd`,
  `tablet-center`, `com.tabletcenter.TabletCenter`,
  `tablet-center@tabletcenter.dev`). `migrate_legacy_paths()` in
  `crates/tablet-config/src/lib.rs` (called from both binaries) moves old
  config/data dirs and the autostart entry; the install/remove flows clean
  the old unit, rule, and extension. Don't drop those legacy cleanups until
  old installs are a non-issue.

## Commands

- `./dev.sh` — dev app, UI only; talks to the installed `tabletforged.service`.
  Serves `ui/` with `bun tools/dev-server.ts` on `127.0.0.1:8765` (Bun
  required). UI edits appear on Ctrl+R, no rebuild.
- `./dev.sh --daemon` — also builds and runs a local `tabletforged`, but **only if
  the systemd service is stopped**; otherwise it just uses the installed
  service. Stop it with `systemctl --user stop tabletforged` first.
- `cargo test --workspace` — what CI runs. Links `tabletforge`, so it needs
  `libwebkit2gtk-4.1-dev libgtk-3-dev librsvg2-dev` on the host. Without
  them, `cargo test -p tabletforged -p tablet-config`. Inside the container:
  `scripts/container.sh test`.
- Single test: `cargo test -p tabletforged pen::` (tests live as inline `mod tests`
  in `tabletforged`'s `pen.rs`, `pad.rs`, `mapping.rs`).

## Packaging and release

- `scripts/package.sh [aarch64]` → `dist/tabletforge-<arch>-linux.tar.gz`.
  Archive name has no version (links use `releases/latest/download/`);
  `dist/` and `target/` are gitignored.
- `scripts/container.sh build|test|release|shell` — full toolchain in a
  container so the host never installs WebKitGTK deps. Podman preferred,
  docker fallback.
- Flatpak: build `packaging/flatpak/org.tabletforge.TabletForge.yml` into a
  standalone `.flatpak` bundle (command in README). `tabletforge` has a
  `flatpak` feature that routes host integration through `flatpak-spawn
  --host`.
- Release flow: push tag `vX.Y.Z` → CI builds archives + bundle and attaches
  them to a prerelease. The workflow **fails unless the tag equals
  `Cargo.toml`'s `[workspace.package] version`**. When bumping the version,
  update `Cargo.toml` *and* `crates/tabletforge/tauri.conf.json` (it holds
  its own copy).

## Layout

- `crates/tabletforged` — daemon, entrypoint `src/main.rs`. Device logic per file:
  `pen.rs`, `pad.rs`, `mice.rs`; plus `mapping.rs`, `displays.rs`,
  `devices.rs`, `shell.rs`.
- `crates/tabletforge` — Tauri app (`src/main.rs`; `tauri.conf.json`
  sets `frontendDist: ../../ui` and `devUrl: http://127.0.0.1:8765`).
  `gen/schemas/` is Tauri-generated and gitignored.
- `crates/tablet-config` — config schema and the socket protocol shared by
  daemon and app; `src/lib.rs` is the wire-format reference.
- `ui/` — vanilla HTML/CSS/JS, no bundler, no package.json, no node_modules.
  Two frontends in parallel: `app.js` (Tablet) and `glass.js`+`glass.html`
  (Glass). The dev server (`tools/`) is the entire frontend toolchain.
- `gnome-extension/` — GNOME Shell extension installed by `install.sh`;
  talks to the daemon over `$XDG_RUNTIME_DIR/tabletforged.sock` and a private
  DBus interface (`org.tabletforge.Shell`).
- `packaging/` — flatpak manifest, desktop/metainfo, udev rule, systemd
  unit. `install.sh`/`uninstall.sh` lay it out under XDG dirs.

## Daemon safety — read before touching `crates/tabletforged`

Per README (and it's the reason to be careful): gnome-shell crashes — taking
the session down — if a virtual tablet disappears or changes owner while the
pen is in proximity. The daemon only stays safe because it:

- releases everything before removing its devices,
- grabs the tablet only while the pen is away from the surface,
- never reuses the real pen's serial.

Keep those guarantees in any change, and stop the daemon with SIGTERM,
never SIGKILL. Real hardware is needed to verify daemon changes; the unit
tests cover geometry/protocol, not device lifecycles.

## i18n

`ui/app.js` has `EXTRA_EN`: stable translation keys → current English copy.
When revising user-facing wording, update `EXTRA_EN` (or the key itself only
if it must change) — don't drift the keys or every `ui/locales/*.json`
becomes stale. 14 languages; English keys are the source of truth.
