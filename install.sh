#!/bin/sh
# Installs Tablet Center for the current user: the tabletd service that
# starts with the graphical session, the settings app and its GNOME Shell
# extension.
#
# Distribution model: CI ships one pre-built archive per arch
# (tablet-center-x86_64-linux.tar.gz, tablet-center-aarch64-linux.tar.gz).
# This script detects uname -m, downloads (or reuses from dist/) only the
# matching archive, extracts it and lays out XDG/freedesktop dirs. No host
# build needed. Build-from-source remains available via scripts/package.sh
# for developers and tinkerers.
#
# Keep the pen away from the tablet while this runs: restarting tabletd
# replaces its virtual devices, and GNOME Shell can crash if that happens
# while the pen is in proximity.
set -eu

case "${XDG_CURRENT_DESKTOP:-GNOME}" in
*GNOME*) ;;
*) echo "Tablet Center needs GNOME on Wayland; this session is $XDG_CURRENT_DESKTOP." >&2; exit 1 ;;
esac

# ---- architecture detection ---------------------------------------------
arch=$(uname -m)
case "$arch" in
    x86_64|amd64)    arch=x86_64 ;;
    aarch64|arm64)   arch=aarch64 ;;
    *) echo "Unsupported architecture: $arch (only x86_64 and aarch64 are shipped)." >&2; exit 1 ;;
esac

# ---- XDG-derived paths (freedesktop spec, no hardcoding) ----------------
XDG_CONFIG_HOME="${XDG_CONFIG_HOME:-$HOME/.config}"
XDG_DATA_HOME="${XDG_DATA_HOME:-$HOME/.local/share}"
XDG_RUNTIME_DIR="${XDG_RUNTIME_DIR:-$HOME/.local/run}"
XDG_BIN_HOME="${XDG_BIN_HOME:-$HOME/.local/bin}"

config_dir="$XDG_CONFIG_HOME/tablet-center"
data_dir="$XDG_DATA_HOME"
bin_dir="$XDG_BIN_HOME"
service_dir="$XDG_CONFIG_HOME/systemd/user"
apps_dir="$XDG_DATA_HOME/applications"
icons_dir="$XDG_DATA_HOME/icons/hicolor"
ext_dir="$XDG_DATA_HOME/gnome-shell/extensions/tablet-center@tabletcenter.dev"

# ---- source checkout fallback -------------------------------------------
# When run from a git checkout whose dist/ already holds the right archive,
# use it directly (offline, no network). Otherwise download from GitHub.
archive=""
if [ -d dist ] && [ -f "dist/tablet-center-${arch}-linux.tar.gz" ]; then
    archive="dist/tablet-center-${arch}-linux.tar.gz"
elif command -v curl >/dev/null 2>&1 || command -v wget >/dev/null 2>&1; then
    tmpdir=$(mktemp -d)
    trap 'rm -rf "$tmpdir"' EXIT
    archive="$tmpdir/tablet-center-${arch}-linux.tar.gz"
    url="https://github.com/mhasanmuammar/tablet/releases/latest/download/tablet-center-${arch}-linux.tar.gz"
    if command -v curl >/dev/null 2>&1; then
        curl -fsSL "$url" -o "$archive"
    else
        wget -qO "$archive" "$url"
    fi
else
    echo "Neither curl nor wget is available, and no local dist/ archive for $arch." >&2
    exit 1
fi

stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT
tar -xzf "$archive" -C "$stage"

# ---- system part (sudo once): WebKitGTK, uinput rule, input group -------
webkit=""
if ! ldconfig -p 2>/dev/null | grep -q 'libwebkit2gtk-4\.1\.so\.0'; then
    if command -v apt-get >/dev/null; then webkit="apt-get install -y libwebkit2gtk-4.1-0"
    elif command -v dnf >/dev/null; then webkit="dnf install -y webkit2gtk4.1"
    elif command -v pacman >/dev/null; then webkit="pacman -S --needed --noconfirm webkit2gtk-4.1"
    elif command -v zypper >/dev/null; then webkit="zypper --non-interactive install libwebkit2gtk-4_1-0"
    else echo "Install WebKitGTK 4.1 with your package manager, then run this again." >&2; exit 1
    fi
fi
rule=/etc/udev/rules.d/70-tablet-center.rules
need_rule=false
cmp -s "$stage/packaging/70-tablet-center.rules" "$rule" 2>/dev/null || need_rule=true
need_group=false
id -nG | tr ' ' '\n' | grep -qx input || need_group=true
if [ -n "$webkit" ] || $need_rule || $need_group; then
    echo "Administrator rights are needed once to set up the system (sudo)."
    [ -n "$webkit" ] && sudo $webkit
    if $need_rule; then
        sudo install -m 644 "$stage/packaging/70-tablet-center.rules" "$rule"
        sudo udevadm control --reload
        sudo udevadm trigger --subsystem-match=misc --sysname-match=uinput
    fi
    $need_group && sudo usermod -aG input "$USER"
fi

if [ -t 0 ]; then
    printf 'Move the pen away from the tablet, then press Enter. '
    read -r _
fi

# ---- install binaries, service, desktop, icons, extension ---------------
mkdir -p "$bin_dir" "$service_dir" "$apps_dir"
install -m 755 "$stage/bin/tabletd" "$bin_dir/tabletd"
install -m 755 "$stage/bin/tablet-center" "$bin_dir/tablet-center"
install -m 644 "$stage/packaging/tabletd.service" "$service_dir/tabletd.service"
systemctl --user daemon-reload
systemctl --user enable tabletd.service
systemctl --user restart tabletd.service

sed "s|^Exec=.*|Exec=$bin_dir/tablet-center|" "$stage/packaging/tablet-center.desktop" >"$apps_dir/tablet-center.desktop"
for size in 48 64 128 256 512; do
    dir="$icons_dir/${size}x${size}/apps"
    mkdir -p "$dir"
    install -m 644 "$stage/packaging/icons/tablet-center-$size.png" "$dir/tablet-center.png"
done
gtk-update-icon-cache -q -t "$icons_dir" 2>/dev/null || true
update-desktop-database -q "$apps_dir" 2>/dev/null || true

mkdir -p "$ext_dir"
install -m 644 "$stage/gnome-extension/tablet-center@tabletcenter.dev/"* "$ext_dir/"
enabled=$(gsettings get org.gnome.shell enabled-extensions)
ext_id=tablet-center@tabletcenter.dev
case "$enabled" in
*"'$ext_id'"*) ;;
"@as []") gsettings set org.gnome.shell enabled-extensions "['$ext_id']" ;;
*) gsettings set org.gnome.shell enabled-extensions "${enabled%]}, '$ext_id']" ;;
esac

echo
systemctl --user --no-pager status tabletd.service | head -3
if $need_group; then
    echo "Log out and back in: you were added to the input group, and GNOME Shell loads the extension then."
    echo "If the tablet still does nothing after that, restart the computer."
else
    echo "Installed. GNOME Shell uses the updated extension after logging out and back in."
fi
