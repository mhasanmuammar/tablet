//! Tablet Center: settings UI. The tablet itself is driven by
//! `tabletd`; this app edits the shared config and asks the daemon to reload.

use std::{
    fs,
    io::{self, BufRead, BufReader, Write},
    path::PathBuf,
    os::unix::net::UnixStream,
    process::Command,
    sync::Mutex,
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use gtk::prelude::*;
use tauri::{Emitter, Manager, WebviewUrl, WebviewWindowBuilder};
use tablet_config::{Config, KeyAction, Request, Response};

// Commands that touch the disk, the daemon or other programs are `async`,
// which Tauri runs off the main thread, so a slow answer never freezes the
// window.

/// One connection to tabletd, kept open: the UI asks for the status up to 25
/// times a second, and each new connection costs the daemon a thread.
static DAEMON: Mutex<Option<BufReader<UnixStream>>> = Mutex::new(None);

fn ask(request: &Request) -> Result<Response, String> {
    let mut daemon = DAEMON.lock().unwrap();
    // A kept connection may have died with a daemon restart: retry once.
    for _ in 0..2 {
        if daemon.is_none() {
            let stream = UnixStream::connect(tablet_config::socket_path())
                .map_err(|_| "tabletd is not running".to_string())?;
            stream.set_read_timeout(Some(Duration::from_millis(500))).map_err(|e| e.to_string())?;
            *daemon = Some(BufReader::new(stream));
        }
        match exchange(daemon.as_mut().unwrap(), request) {
            Ok(response) => return Ok(response),
            Err(_) => *daemon = None,
        }
    }
    Err("tabletd is not responding".into())
}

fn exchange(conn: &mut BufReader<UnixStream>, request: &Request) -> io::Result<Response> {
    let mut line = serde_json::to_vec(request)?;
    line.push(b'\n');
    conn.get_mut().write_all(&line)?;
    let mut reply = String::new();
    if conn.read_line(&mut reply)? == 0 {
        return Err(io::ErrorKind::UnexpectedEof.into());
    }
    Ok(serde_json::from_str(&reply)?)
}

#[tauri::command(async)]
fn get_config() -> Config {
    tablet_config::load()
}

/// Saves the settings; returns whether the running daemon applied them.
#[tauri::command(async)]
fn save_config(config: Config) -> Result<bool, String> {
    tablet_config::save(&config).map_err(|e| e.to_string())?;
    Ok(ask(&Request::Reload).is_ok())
}

/// Menus for the GNOME Shell extension, as the UI resolves them (see
/// shellMenus in app.js); the extension reads the file when it opens one.
#[tauri::command(async)]
fn save_shell_menus(menus: serde_json::Value) -> Result<(), String> {
    let path = tablet_config::shell_menus_path();
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, menus.to_string()).and_then(|()| fs::rename(&tmp, &path)).map_err(|e| e.to_string())
}

#[tauri::command(async)]
fn status() -> Response {
    ask(&Request::Status).unwrap_or_else(|error| Response { error: Some(error), ..Default::default() })
}

#[tauri::command(async)]
fn restart_daemon() -> Result<(), String> {
    let ok = Command::new("systemctl")
        .args(["--user", "restart", "tabletd.service"])
        .status()
        .map_err(|e| e.to_string())?
        .success();
    if ok { Ok(()) } else { Err("systemctl --user restart tabletd.service failed".into()) }
}

fn home() -> Result<PathBuf, String> {
    std::env::var_os("HOME").map(PathBuf::from).ok_or_else(|| "HOME is not set".into())
}

fn autostart_path() -> Result<PathBuf, String> {
    Ok(home()?.join(".config/autostart/tablet-center.desktop"))
}

#[tauri::command(async)]
fn autostart_enabled() -> Result<bool, String> {
    Ok(autostart_path()?.is_file())
}

#[tauri::command(async)]
fn set_autostart(enabled: bool) -> Result<(), String> {
    let path = autostart_path()?;
    if enabled {
        fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
        let binary = home()?.join(".local/bin/tablet-center");
        let binary = binary.to_string_lossy().replace('\\', "\\\\").replace('"', "\\\"")
            .replace('`', "\\`").replace('$', "\\$").replace('%', "%%");
        fs::write(path, format!("[Desktop Entry]\nType=Application\nName=Tablet Center\nExec=\"{binary}\"\nIcon=tablet-center\nX-GNOME-Autostart-enabled=true\n"))
            .map_err(|e| e.to_string())
    } else {
        match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    }
}

fn backups_dir() -> Result<PathBuf, String> {
    Ok(home()?.join(".config/tablet-center/backups"))
}

fn backup_path(filename: &str) -> Result<PathBuf, String> {
    if !filename.ends_with(".json") || filename.starts_with('.')
        || !filename.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.')) {
        return Err("Invalid backup filename".into());
    }
    let path = backups_dir()?.join(filename);
    if fs::symlink_metadata(&path).map(|m| m.file_type().is_symlink()).unwrap_or(false) {
        return Err("Invalid backup file".into());
    }
    Ok(path)
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Backup {
    title: String,
    config: Config,
}

#[derive(serde::Serialize)]
struct BackupInfo {
    filename: String,
    title: String,
    created: u64,
}

#[tauri::command(async)]
fn list_backups() -> Result<Vec<BackupInfo>, String> {
    let dir = backups_dir()?;
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let mut backups = Vec::new();
    for entry in fs::read_dir(dir).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let filename = entry.file_name().to_string_lossy().into_owned();
        let Ok(path) = backup_path(&filename) else { continue };
        if !entry.file_type().map_err(|e| e.to_string())?.is_file() { continue; }
        let Ok(backup) = serde_json::from_slice::<Backup>(&fs::read(path).map_err(|e| e.to_string())?) else { continue };
        let created = entry.metadata().and_then(|m| m.modified()).map_err(|e| e.to_string())?
            .duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
        backups.push(BackupInfo { filename, title: backup.title, created });
    }
    backups.sort_by(|a, b| b.created.cmp(&a.created).then_with(|| a.filename.cmp(&b.filename)));
    Ok(backups)
}

#[tauri::command(async)]
fn create_backup(title: String) -> Result<(), String> {
    let title = title.trim();
    if title.is_empty() || title.chars().count() > 120 {
        return Err("Backup title must contain 1 to 120 characters".into());
    }
    fs::create_dir_all(backups_dir()?).map_err(|e| e.to_string())?;
    let name: String = title.chars().take(60).map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
    let timestamp = SystemTime::now().duration_since(UNIX_EPOCH).map_err(|e| e.to_string())?.as_nanos();
    let path = backup_path(&format!("{name}-{timestamp}.json"))?;
    let mut file = fs::OpenOptions::new().write(true).create_new(true).open(path).map_err(|e| e.to_string())?;
    serde_json::to_writer_pretty(&mut file, &Backup { title: title.into(), config: get_config() }).map_err(|e| e.to_string())
}

#[tauri::command(async)]
fn restore_backup(filename: String) -> Result<bool, String> {
    let bytes = fs::read(backup_path(&filename)?).map_err(|e| e.to_string())?;
    let backup: Backup = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    save_config(backup.config)
}

#[tauri::command(async)]
fn delete_backup(filename: String) -> Result<(), String> {
    fs::remove_file(backup_path(&filename)?).map_err(|e| e.to_string())
}

#[tauri::command(async)]
fn open_backups_folder() -> Result<(), String> {
    let dir = backups_dir()?;
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    if Command::new("xdg-open").arg(dir).status().map_err(|e| e.to_string())?.success() {
        Ok(())
    } else {
        Err("Could not open backup folder".into())
    }
}

#[tauri::command(async)]
fn reset_config() -> Result<bool, String> {
    save_config(Config::default())
}

/// Runs an action picked from an on-screen shortcut. The window hides first
/// so focus goes back to the app underneath before the keys arrive.
#[tauri::command]
fn run_shortcut_action(window: tauri::WebviewWindow, action: KeyAction) {
    hide_shortcut(&window);
    thread::spawn(move || {
        thread::sleep(Duration::from_millis(150));
        let _ = ask(&Request::Run { action });
    });
}

#[tauri::command]
fn close_shortcut(window: tauri::WebviewWindow) {
    hide_shortcut(&window);
}

/// Whether the shortcut window is on screen, and showing which menu.
static SHOWN: Mutex<Option<String>> = Mutex::new(None);
/// The last menu asked for, for a page that loads after the request came.
static PENDING: Mutex<Option<serde_json::Value>> = Mutex::new(None);

#[tauri::command]
fn pending_shortcut() -> Option<serde_json::Value> {
    PENDING.lock().unwrap().clone()
}

fn hide_shortcut(window: &tauri::WebviewWindow) {
    *SHOWN.lock().unwrap() = None;
    let _ = window.hide();
}

/// A request from tabletd: one JSON line on stdin per button press.
#[derive(serde::Deserialize)]
struct ShowRequest {
    name: String,
    at: Option<(i32, i32)>,
    #[serde(default)]
    centered: bool,
}

/// `tablet-center --shortcut-host`: started once by tabletd and kept running,
/// so on-screen shortcuts open at once instead of after the half second a
/// new WebKit app takes. The window is created hidden and reused.
fn shortcut_host(app: &tauri::App) -> tauri::Result<()> {
    shortcut_window(app.handle())?;
    let handle = app.handle().clone();
    thread::spawn(move || {
        for line in io::stdin().lines() {
            let Ok(request) = serde_json::from_str::<ShowRequest>(&line.unwrap_or_default()) else { continue };
            let app = handle.clone();
            let _ = handle.run_on_main_thread(move || {
                if let Err(e) = show_shortcut(&app, &request) {
                    eprintln!("tablet-center: cannot show {}: {e}", request.name);
                }
            });
        }
        // tabletd went away.
        handle.exit(0);
    });
    Ok(())
}

/// A transparent maximized window, so the menu can be drawn where the pen
/// is (Wayland lets no client place a window itself). Not full screen:
/// mutter paints full-screen windows over black.
fn shortcut_window(app: &tauri::AppHandle) -> tauri::Result<tauri::WebviewWindow> {
    if let Some(window) = app.get_webview_window("shortcut") {
        return Ok(window);
    }
    WebviewWindowBuilder::new(app, "shortcut", WebviewUrl::App("shortcut.html".into()))
        .title("Tablet Center")
        .decorations(false)
        .transparent(true)
        .background_color(tauri::window::Color(0, 0, 0, 0))
        .shadow(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .maximized(true)
        .visible(false)
        .build()
}

/// Shows the menu under the pen, or hides it when the same button is
/// pressed again. tabletd moves the mouse pointer under the pen first, and
/// mutter maps the window on the pointer's monitor.
fn show_shortcut(app: &tauri::AppHandle, request: &ShowRequest) -> tauri::Result<()> {
    let window = shortcut_window(app)?;
    let mut shown = SHOWN.lock().unwrap();
    if shown.as_deref() == Some(request.name.as_str()) {
        // The page zooms the menu out, then calls close_shortcut.
        *shown = None;
        return window.eval("window.hideShortcut?.()");
    }
    let monitors: Vec<gtk::gdk::Rectangle> = gtk::gdk::Display::default()
        .map(|d| (0..d.n_monitors()).filter_map(|i| d.monitor(i)).map(|m| m.geometry()).collect())
        .unwrap_or_default();
    let (x, y) = request.at.unwrap_or((i32::MIN, i32::MIN));
    let monitor = monitors
        .iter()
        .find(|g| x >= g.x() && x < g.x() + g.width() && y >= g.y() && y < g.y() + g.height());
    // The pen inside that monitor, and the monitor's size so the page can
    // subtract the top bar the maximized window leaves out. Without them it
    // centres the menu.
    let payload = serde_json::json!({
        "name": request.name,
        "at": monitor.map(|g| (x - g.x(), y - g.y())),
        "monitor": monitor.map(|g| (g.width(), g.height())),
        "centered": request.centered,
        // GNOME Shell animates the window as it maps; the page waits for
        // that to end so the menu does not move with it.
        "mapping": !window.is_visible()?,
    });
    window.eval(format!("window.showShortcut?.({payload})"))?;
    *PENDING.lock().unwrap() = Some(payload);
    if let Some(g) = monitor {
        window.set_size(tauri::LogicalSize::new(f64::from(g.width()), f64::from(g.height())))?;
    }
    window.show()?;
    window.set_focus()?;
    *shown = Some(request.name.clone());
    Ok(())
}

/// Whether we are running inside a Flatpak sandbox.
fn is_flatpak() -> bool {
    std::env::var("FLATPAK_ID").is_ok()
}

/// Run a shell command on the host (via flatpak-spawn --host in Flatpak,
/// or directly otherwise). Returns Ok(()) on success.
fn host_cmd(program: &str, args: &[&str]) -> Result<(), String> {
    let mut cmd = if is_flatpak() {
        let mut c = Command::new("flatpak-spawn");
        c.arg("--host").arg(program);
        c
    } else {
        Command::new(program)
    };
    for a in args {
        cmd.arg(a);
    }
    let status = cmd.status().map_err(|e| e.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{program} exited with {status}"))
    }
}

/// Run a shell command on the host and capture stdout.
fn host_output(program: &str, args: &[&str]) -> Result<String, String> {
    let mut cmd = if is_flatpak() {
        let mut c = Command::new("flatpak-spawn");
        c.arg("--host").arg(program);
        c
    } else {
        Command::new(program)
    };
    for a in args {
        cmd.arg(a);
    }
    let out = cmd.output().map_err(|e| e.to_string())?;
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

/// Run a shell script on the host with administrator rights, on the standard
/// desktop authentication path: polkit. pkexec asks the session's
/// authentication agent (GNOME Shell's "Authentication required" dialog);
/// `sudo` cannot prompt from a GUI window or a Flatpak sandbox, and fails.
/// The script is written where the host can read it (xdg-data is shared with
/// the host in Flatpak) and removed afterwards. All privileged steps should
/// go into one script so there is a single authentication dialog.
fn host_privileged(script: &str) -> Result<(), String> {
    let dir = home()?.join(".local/share/tablet-center");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join("host-setup.sh");
    fs::write(&path, script).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o700));
    }
    // --disable-internal-agent: never fall back to a textual agent, which
    // would block on the spawn pipe. pkexec exit codes (documented):
    // 126 = the user dismissed the dialog, 127 = not authorized / failed.
    let path_arg = path.to_str().unwrap_or_default().to_string();
    let result = host_cmd("pkexec", &["--disable-internal-agent", "sh", &path_arg]);
    let _ = fs::remove_file(&path);
    match result {
        Ok(()) => Ok(()),
        Err(e) if e.contains("exit status: 126") => Err(format!(
            "Setup was canceled at the authentication prompt. Approve the dialog to continue ({e})."
        )),
        Err(e) if e.contains("exit status: 127") => Err(format!(
            "Administrator authentication failed: wrong password, no password-less prompt, or no authentication agent in this session ({e})."
        )),
        Err(e) => Err(format!("Administrator setup on the host failed: {e}")),
    }
}

#[derive(serde::Serialize)]
struct HostIntegrationStatus {
    extension_installed: bool,
    service_installed: bool,
    service_running: bool,
    udev_rule_installed: bool,
    user_in_input_group: bool,
    all_ok: bool,
}

/// Progress lines streamed to the UI while host setup runs ("host-setup-log"
/// events). kind: "step" (running), "ok", "err", "info".
#[derive(Clone, serde::Serialize)]
struct SetupLog {
    kind: String,
    text: String,
}

fn emit_setup_log(app: &tauri::AppHandle, kind: &str, text: &str) {
    let _ = app.emit(
        "host-setup-log",
        SetupLog { kind: kind.to_string(), text: text.to_string() },
    );
}

fn ext_dir() -> PathBuf {
    home().unwrap_or_default().join(".local/share/gnome-shell/extensions/tablet-center@tabletcenter.dev")
}

fn service_path() -> PathBuf {
    home().unwrap_or_default().join(".config/systemd/user/tabletd.service")
}

fn udev_rule_path() -> PathBuf {
    PathBuf::from("/etc/udev/rules.d/70-tablet-center.rules")
}

/// Where the bundled udev rule file lives. Looks near the current
/// executable's prefix: covers /app (Flatpak) and prefix installs, plus the
/// packaging directory of a checkout. Into privileged scripts these paths
/// must never go (see host_setup_script): /app exists only in the sandbox.
fn udev_rule_source() -> Option<PathBuf> {
    // Native builds: the packaging directory in a checkout (dev runs), or the
    // install prefix share directory for layout variants of package.sh.
    let mut candidates = vec![PathBuf::from("packaging/70-tablet-center.rules")];
    if let Ok(exe) = std::env::current_exe() {
        if let Some(prefix) = exe.parent().and_then(|d| d.parent()) {
            candidates.push(prefix.join("share/udev/rules.d/70-tablet-center.rules"));
        }
    }
    candidates.into_iter().find(|p| p.is_file())
}

fn user_in_input_group() -> bool {
    // Look the user up by name: `id -nG <name>` queries the group database
    // fresh, while `id -nG` reports the calling process's supplementary
    // groups — stale after a usermod until relogin (e.g. the systemd user
    // manager spawned by flatpak-spawn keeps its old group set).
    let name = match host_output("id", &["-un"]) {
        Ok(s) => s.trim().to_string(),
        Err(_) => return false,
    };
    if name.is_empty() {
        return false;
    }
    host_output("id", &["-nG", &name])
        .map(|s| s.split_whitespace().any(|g| g == "input"))
        .unwrap_or(false)
}

/// Whether the udev rule exists — on the HOST. /etc is not shared with the
/// Flatpak sandbox, so a sandbox-local is_file() check is always false
/// there.
fn udev_rule_installed_on_host() -> bool {
    if is_flatpak() {
        host_cmd("test", &["-f", udev_rule_path().to_str().unwrap_or_default()]).is_ok()
    } else {
        udev_rule_path().is_file()
    }
}

/// Whether the installed rule matches the bundled one. The rule is staged
/// into the host-visible helper directory first (the host cannot read
/// sandbox paths) and compared with cmp.
fn udev_rule_matches_host() -> bool {
    let Some(src) = udev_rule_source() else {
        return false;
    };
    let staged = match stage_host_file(&src, "70-tablet-center.rules") {
        Ok(p) => p,
        Err(_) => return false,
    };
    host_cmd(
        "cmp",
        &["-s", staged.to_str().unwrap_or_default(), udev_rule_path().to_str().unwrap_or_default()],
    )
    .is_ok()
}

/// Copies a file into ~/.local/share/tablet-center, where the host can read
/// it (xdg-data is shared with the host in Flatpak).
fn stage_host_file(src: &std::path::Path, name: &str) -> Result<PathBuf, String> {
    let dir = home()?.join(".local/share/tablet-center");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let dst = dir.join(name);
    // Remove first: copying over a running daemon fails (ETXTBSY).
    let _ = fs::remove_file(&dst);
    fs::copy(src, &dst).map_err(|e| e.to_string())?;
    Ok(dst)
}

/// Builds the single privileged script for host integration. `rule_path`
/// must be a path the HOST can read (it is executed as root on the host,
/// where paths like /app exist only for the Flatpak sandbox, not for it).
fn host_setup_script(rule_path: Option<&std::path::Path>) -> String {
    let mut script = String::from(
        "#!/bin/sh\n# Tablet Center host setup, run as root via\n# pkexec (polkit authentication).\nset -e\nRULE=/etc/udev/rules.d/70-tablet-center.rules\n",
    );
    if let Some(src) = rule_path {
        let quoted = src.to_string_lossy().replace('\'', "'\\''");
        script.push_str(&format!("install -m 644 '{quoted}' \"$RULE\"\n"));
        script.push_str("udevadm control --reload || true\n");
        script.push_str("udevadm trigger --subsystem-match=misc --sysname-match=uinput || true\n");
        // Re-process pugged-in input devices so the new uaccess ACLs apply
        // without a replug.
        script.push_str("udevadm trigger --subsystem-match=input || true\n");
    }
    // pkexec sets PKEXEC_UID to the invoking user; adding that user to
    // the input group is best effort (the UI checks and hints about it).
    script.push_str("OWNER=$(id -nu \"$PKEXEC_UID\" 2>/dev/null || true)\n[ -n \"$OWNER\" ] && usermod -aG input \"$OWNER\" 2>/dev/null || true\n");
    script
}

#[tauri::command(async)]
fn check_host_integration() -> HostIntegrationStatus {
    let extension_installed = ext_dir().join("extension.js").is_file();
    let service_installed = service_path().is_file();
    let service_running = host_output("systemctl", &["--user", "is-active", "tabletd.service"])
        .map(|s| s.trim() == "active")
        .unwrap_or(false);
    let udev_rule_installed = udev_rule_installed_on_host();
    let user_in_input_group = user_in_input_group();
    let all_ok = extension_installed && service_installed && service_running && udev_rule_installed && user_in_input_group;
    HostIntegrationStatus {
        extension_installed,
        service_installed,
        service_running,
        udev_rule_installed,
        user_in_input_group,
        all_ok,
    }
}

#[tauri::command(async)]
fn install_host_integration(app: tauri::AppHandle) -> Result<(), String> {
    let log = |kind: &str, text: &str| emit_setup_log(&app, kind, text);
    log("step", "Checking what host integration is already installed…");
    let home_dir = home()?;
    let ext = ext_dir();
    let svc = service_path();

    // Install GNOME Shell extension from bundled resources.
    // In Flatpak, resources are under /app; native builds use the install dir.
    let ext_src = if is_flatpak() {
        PathBuf::from("/app/share/gnome-shell/extensions/tablet-center@tabletcenter.dev")
    } else {
        // Find extension relative to the binary or use a known path.
        PathBuf::from("gnome-extension/tablet-center@tabletcenter.dev")
    };
    if ext_src.is_dir() {
        log("step", "Copying the GNOME Shell extension to the host…");
        fs::create_dir_all(&ext).map_err(|e| e.to_string())?;
        for entry in fs::read_dir(&ext_src).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let dest = ext.join(entry.file_name());
            if entry.file_type().map_err(|e| e.to_string())?.is_file() {
                fs::copy(entry.path(), &dest).map_err(|e| e.to_string())?;
            }
        }
        log("ok", "GNOME Shell extension installed.");
    }

    // Install systemd service.
    if let Some(parent) = svc.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    // Exec path depends on runtime: in Flatpak the daemon is not on the host
    // yet, so copy it from /app/bin into the host-visible helper directory
    // (~/.local/share is shared with the host's filesystem) and point
    // systemd there. Native installs can rely on install.sh having placed
    // tabletd in ~/.local/bin.
    let daemon_bin = if is_flatpak() {
        log("step", "Staging the daemon binary for the host…");
        let dst = stage_host_file(std::path::Path::new("/app/bin/tabletd"), "tabletd")
            .map_err(|e| format!("Could not stage the daemon for the host: {e}"))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&dst, fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())?;
        }
        log("ok", "Daemon staged.");
        dst
    } else {
        home_dir.join(".local/bin/tabletd")
    };
    let service_content = format!(
        "[Unit]\nDescription=Tablet Center daemon\nAfter=graphical-session.target\n\n[Service]\nType=simple\nExecStart={}\nRestart=on-failure\n\n[Install]\nWantedBy=graphical-session.target\n",
        daemon_bin.display()
    );
    fs::write(&svc, service_content).map_err(|e| e.to_string())?;
    log("ok", "Systemd service installed.");

    // udev rule and input group need administrator rights on the host. Ask
    // for them the standard way (polkit): one authentication dialog for all
    // privileged steps. The rule is refreshed when its content differs from
    // the bundled one (upgrading an older install).
    let rule_installed = udev_rule_installed_on_host();
    let mut need_rule = !rule_installed;
    let staged_rule = match udev_rule_source() {
        Some(src) => {
            log("step", "Staging the udev rule…");
            let dst = stage_host_file(&src, "70-tablet-center.rules")
                .map_err(|e| format!("Could not stage the udev rule: {e}"))?;
            if rule_installed
                && host_cmd(
                    "cmp",
                    &["-s", dst.to_str().unwrap_or_default(), udev_rule_path().to_str().unwrap_or_default()],
                )
                .is_err()
            {
                need_rule = true; // content differs from an older install
            }
            Some(dst)
        }
        None => {
            if !rule_installed {
                return Err("The udev rule template is missing from this install; re-run install.sh or reinstall the Flatpak.".to_string());
            }
            None
        }
    };
    let need_group = !user_in_input_group();
    if need_rule || need_group {
        log("info", "Administrator rights needed for the udev rule — an authentication dialog will open…");
        host_privileged(&host_setup_script(staged_rule.as_deref()))?;
        log("ok", "Udev rule and user groups updated.");
    }

    // Start the daemon AFTER the privileged steps: its fresh process then
    // sees the new udev ACLs (and the input group if the manager has been
    // restarted meanwhile).
    log("step", "Starting the tabletd service…");
    host_cmd("systemctl", &["--user", "daemon-reload"])?;
    host_cmd("systemctl", &["--user", "enable", "tabletd.service"])?;
    let _ = host_cmd("systemctl", &["--user", "restart", "tabletd.service"]);
    match host_output("systemctl", &["--user", "is-active", "tabletd.service"]) {
        Ok(s) if s.trim() == "active" => log("ok", "The tabletd service is running."),
        Ok(_) => log("err", "The tabletd service did not start — check the status list below (a logout may help)."),
        Err(e) => log("err", &format!("Could not talk to systemd: {e}")),
    }

    // Enable GNOME Shell extension.
    log("step", "Enabling the GNOME Shell extension…");
    let ext_id = "tablet-center@tabletcenter.dev";
    let enabled = host_output("gsettings", &["get", "org.gnome.shell", "enabled-extensions"]).unwrap_or_default();
    if !enabled.contains(ext_id) {
        if enabled.trim() == "@as []" {
            let _ = host_cmd("gsettings", &["set", "org.gnome.shell", "enabled-extensions", &format!("[\"{ext_id}\"]")]);
        } else {
            let new = enabled.trim().trim_end_matches(']').to_string() + &format!(", '{ext_id}']");
            let _ = host_cmd("gsettings", &["set", "org.gnome.shell", "enabled-extensions", &new]);
        }
    }
    log("ok", "GNOME Shell extension enabled.");

    // Mark as checked.
    let marker = home_dir.join(".config/tablet-center/.host-integration-checked");
    if let Some(parent) = marker.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let _ = fs::write(marker, "");

    log("ok", "Host setup complete.");
    Ok(())
}

#[tauri::command(async)]
fn remove_host_integration(app: tauri::AppHandle) -> Result<(), String> {
    let log = |kind: &str, text: &str| emit_setup_log(&app, kind, text);
    let ext = ext_dir();
    let svc = service_path();

    log("step", "Stopping and removing the daemon service…");
    // Remove extension.
    let _ = fs::remove_dir_all(&ext);

    // Remove service.
    let _ = host_cmd("systemctl", &["--user", "disable", "--now", "tabletd.service"]);
    let _ = fs::remove_file(&svc);
    let _ = host_cmd("systemctl", &["--user", "daemon-reload"]);
    // Remove the daemon staged for the host (Flatpak installs).
    let _ = fs::remove_file(home().unwrap_or_default().join(".local/share/tablet-center/tabletd"));
    // Best effort: also clear service leftovers from former native installs.
    let _ = fs::remove_file(home().unwrap_or_default().join(".local/bin/tabletd"));

    // Remove udev rule (needs administrator rights; see host_privileged).
    if udev_rule_installed_on_host() {
        log("info", "Administrator rights needed to remove the udev rule — an authentication dialog will open…");
        host_privileged(
            "#!/bin/sh\n# Tablet Center host cleanup, run as root via pkexec.\nset -e\nrm -f /etc/udev/rules.d/70-tablet-center.rules\nudevadm control --reload\nudevadm trigger --subsystem-match=misc --sysname-match=uinput || true\n",
        )?;
    }
    log("ok", "Host integration removed.");

    // Remove extension from gsettings.
    let ext_id = "tablet-center@tabletcenter.dev";
    let enabled = host_output("gsettings", &["get", "org.gnome.shell", "enabled-extensions"]).unwrap_or_default();
    if enabled.contains(ext_id) {
        let new = enabled.replace(&format!(", '{ext_id}'"), "").replace(&format!("'{ext_id}', "), "").replace(&format!("'{ext_id}'"), "");
        let new = if new.trim() == "[]" { "@as []".to_string() } else { new };
        let _ = host_cmd("gsettings", &["set", "org.gnome.shell", "enabled-extensions", &new]);
    }

    // Remove marker.
    let marker = home().unwrap_or_default().join(".config/tablet-center/.host-integration-checked");
    let _ = fs::remove_file(marker);

    Ok(())
}

#[tauri::command(async)]
fn is_first_launch() -> bool {
    let marker = home().unwrap_or_default().join(".config/tablet-center/.host-integration-checked");
    !marker.is_file()
}

/// Interfaces: (id, page, draws its own title bar). Keep in step with
/// UI_STYLES in app.js.
const UI_STYLES: [(&str, &str, bool); 2] = [("classic", "index.html", false), ("glass", "glass.html", true)];

fn ui_style_path() -> PathBuf {
    tablet_config::config_path().with_file_name("interface")
}

/// Remembers the interface, so the next start opens it directly (with or
/// without the system title bar) instead of redirecting after loading.
#[tauri::command(async)]
fn set_ui_style(style: String) -> Result<(), String> {
    if !UI_STYLES.iter().any(|(id, ..)| *id == style) {
        return Err(format!("unknown interface {style}"));
    }
    fs::write(ui_style_path(), style).map_err(|e| e.to_string())
}

fn main_window(app: &tauri::App) -> tauri::Result<()> {
    let Some(config) = app.config().app.windows.iter().find(|w| w.label == "main").cloned() else {
        return Ok(());
    };
    let chosen = fs::read_to_string(ui_style_path()).unwrap_or_default();
    let (_, page, frameless) = UI_STYLES.iter().find(|(id, ..)| *id == chosen.trim()).unwrap_or(&UI_STYLES[0]);
    let mut config = config;
    config.url = tauri::WebviewUrl::App((*page).into());
    config.decorations = !frameless;
    WebviewWindowBuilder::from_config(app.handle(), &config)?.build()?;
    Ok(())
}

fn main() {
    let host = std::env::args().any(|a| a == "--shortcut-host");
    // GTK uses this as the Wayland app_id, which GNOME matches to
    // tablet-center.desktop for the window's icon and name.
    gtk::glib::set_prgname(Some("tablet-center"));
    gtk::glib::set_application_name("Tablet Center");
    tauri::Builder::default()
        .setup(move |app| {
            if host {
                shortcut_host(app)?;
            } else {
                main_window(app)?;
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![get_config, save_config, save_shell_menus, status, restart_daemon,
            autostart_enabled, set_autostart, list_backups, create_backup, restore_backup,
            delete_backup, open_backups_folder, reset_config, run_shortcut_action, close_shortcut, pending_shortcut, set_ui_style,
            check_host_integration, install_host_integration, remove_host_integration, is_first_launch])
        .build(tauri::generate_context!())
        .expect("error while running Tablet Center")
        .run(move |_, event| {
            // The host lives on with its window hidden, until tabletd exits.
            if let tauri::RunEvent::ExitRequested { code: None, api, .. } = event {
                if host {
                    api.prevent_exit();
                }
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn backup_filenames_cannot_escape_the_backup_folder() {
        for filename in ["../config.json", "/tmp/backup.json", "nested/backup.json", "..json", "backup.txt", "backup\\name.json"] {
            assert!(backup_path(filename).is_err(), "{filename}");
        }
        assert_eq!(backup_path("My-tablet-123.json").unwrap(), backups_dir().unwrap().join("My-tablet-123.json"));
    }

    #[test]
    fn host_setup_script_quotes_the_rule_path() {
        let script = host_setup_script(Some(Path::new("/etc/it's a/test.rules")));
        assert!(script.contains("install -m 644 '/etc/it'\\''s a/test.rules' \"$RULE\""), "{script}");
    }

    #[test]
    fn host_setup_script_without_rule_still_adds_the_group() {
        let script = host_setup_script(None);
        assert!(!script.contains("install "), "{script}");
        assert!(!script.contains("udevadm "), "{script}");
        assert!(script.contains("usermod -aG input \"$OWNER\""), "{script}");
    }

    #[test]
    fn host_setup_script_is_sh_compatible() {
        // A trailing && with a false condition must not kill the script
        // under set -e; udevadm steps are best effort like before.
        let with_rule = host_setup_script(Some(Path::new("/etc/udev/rules.d/x.rules")));
        assert!(with_rule.contains("udevadm control --reload || true"));
        assert!(with_rule.contains("usermod -aG input \"$OWNER\" 2>/dev/null || true"));
        assert!(with_rule.ends_with('\n'));
    }
}
