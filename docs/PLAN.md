# TabletForge — simplification plan

Procedure plan for the roadmap items in README.md: simplify the UX,
deprecate unnecessary stuff, move settings storage to GNOME-standard
GSettings, and port the UI to Adwaita widgets. Each phase ships working,
committable code — no half-broken trees between phases.

Non-negotiables throughout (see AGENTS.md):

- Daemon safety guarantees stay untouched: release virtual devices before
  removing, grab only while the pen is away, never reuse the real pen's
  serial, stop with SIGTERM.
- `tabletforged` and the app must move together whenever the socket
  protocol changes (both read `tablet_config`).
- Legacy-path migration (`migrate_legacy_paths()`, legacy unit/rule cleanup)
  stays until old installs are a non-issue.

## Phase 0 — Rebrand baseline (in flight)

- [ ] Container build + `cargo test --workspace` green with the renamed
  crates (`tabletforged`, `tabletforge`).
- [ ] Commit and push the rebrand (no release).

## Phase 1 — Inventory and decisions (no code)

- [ ] List every setting, screen, and feature in the current UI
  (`ui/app.js`, `ui/glass.js`) and mark each **keep / demote / remove**.
  Candidates to review: the second UI (*Glass*), backups, legacy gestures,
  per-feature speed fine-tuning, and settings subsets nobody changes.
- [ ] Decide the new primary flow: pen buttons → tip feel → scroll →
  mapping/display reach you on the first screen; everything else moves to
  Preferences (or away).
- [ ] Decide which files each kept feature owns (GSettings keys, XDG data
  files for backups and shell menus).
- Output: a short decision record appended to this file.

## Phase 2 — GSettings schema (target-first)

- [ ] Design the `org.tabletforge` schema (schema XML) that mirrors the
  current `Config` struct minus removed items; complex nested values live
  as serialized keys only where the schema can't express them.
- [ ] Add first-run migration: read `~/.config/tabletforge/config.json`,
  write into dconf, keep the file read-only as a fallback until Phase 4.
- [ ] `tablet-config` stops being the storage format; it remains the wire
  protocol reference (JSON per socket line is fine for live IPC).

## Phase 3 — Adwaita UI port (the big one)

- [ ] Rebuild the app crate on `gtk4-rs` + `libadwaita`; the Tauri
  dependency, WebKitGTK link, `ui/` JS stack, and `tools/` dev server go
  away (single `cargo run` development loop).
- [ ] One standard window — *Glass*'s custom transparent shell is not
  ported; Adwaita styling replaces both frontends with one interface.
- [ ] Screens in priority order: Home (device + service status, setup
  banner, most-used controls), Pen (buttons, tip feel, gestures), Mapping
  (displays), Menus (radial/express grids and actions), Preferences
  (autostart, language, backups import/export).
- [ ] Live status keeps flowing through `$XDG_RUNTIME_DIR/tabletforged.sock`
  — unchanged protocol.
- [ ] Replace `EXTRA_EN` + `ui/locales/*.json` with gettext `.po` files;
  regenerate the 14 languages once and stop tracking keys by hand.

## Phase 4 — Daemon reads settings natively

- [ ] `tabletforged` reads settings from dconf (`gio`/`glib-rs`, no GTK
  link) and follows live change notifications; the JSON `config.json` read
  path is removed (socket stays for status + live apply of staged values).
- [ ] `tablet-config` shrinks to the protocol crate; the app no longer
  writes config files at all.
- [ ] Drop the Phase 2 JSON fallback.

## Phase 5 — Deprecation sweep

- [ ] Implement the Phase 1 removal list in daemon, app, extension:
  delete dead config paths, unused gestures, and unused UI knobs.
- [ ] The GNOME extension reads what it needs (menu data) from the
  standard location decided in Phase 1 — no new private files.
- [ ] Trim `tablet-config` to the subset the remaining features need.

## Phase 6 — Packaging, docs, release

- [ ] Flatpak: no WebKitGTK dependency remains; update
  `packaging/flatpak/org.tabletforge.TabletForge.yml`.
- [ ] `install.sh`/`uninstall.sh`: the system part shrinks (no WebKitGTK
  install step); keep udev rule + legacy cleanups.
- [ ] README features/install rebuilt around the Adwaita UI; update
  `AGENTS.md` (crate layout, i18n, dev loop) and `dev.sh`.
- [ ] Bump version (next beta series), tag only when asked.
