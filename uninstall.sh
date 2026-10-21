#!/bin/sh
# Removes TabletForge from the current user. Settings and backups stay
# unless you pass --purge. Keep the pen away from the tablet: stopping
# tabletforged removes its virtual devices.
set -eu

if [ -t 0 ]; then
    printf 'Move the pen away from the tablet, then press Enter. '
    read -r _
fi

# XDG-derived paths (must match install.sh)
XDG_CONFIG_HOME="${XDG_CONFIG_HOME:-$HOME/.config}"
XDG_DATA_HOME="${XDG_DATA_HOME:-$HOME/.local/share}"
XDG_BIN_HOME="${XDG_BIN_HOME:-$HOME/.local/bin}"

service_dir="$XDG_CONFIG_HOME/systemd/user"
bin_dir="$XDG_BIN_HOME"
apps_dir="$XDG_DATA_HOME/applications"
icons_dir="$XDG_DATA_HOME/icons/hicolor"
ext_dir="$XDG_DATA_HOME/gnome-shell/extensions/tabletforge@tabletforge.org"

systemctl --user disable --now tabletforged.service 2>/dev/null || true
# Legacy unit from the pre-rebrand build (best effort).
systemctl --user disable --now tabletd.service 2>/dev/null || true
rm -f "$service_dir/tabletforged.service"
systemctl --user daemon-reload
rm -f "$bin_dir/tabletforged" "$bin_dir/tabletforge" \
    "$apps_dir/tabletforge.desktop" \
    "$HOME/.config/autostart/tabletforge.desktop" \
    "$service_dir/tabletd.service"
for size in 48 64 128 256 512; do
    rm -f "$icons_dir/${size}x${size}/apps/tabletforge.png"
done
gtk-update-icon-cache -q -t "$icons_dir" 2>/dev/null || true

ext_id=tabletforge@tabletforge.org
rm -rf "$ext_dir"
# Names the pre-rebrand build used (best effort).
rm -rf "$XDG_DATA_HOME/gnome-shell/extensions/tablet-center@tabletcenter.dev"
enabled=$(gsettings get org.gnome.shell enabled-extensions)
case "$enabled" in
*"'$ext_id'"*)
    enabled=$(printf '%s' "$enabled" | sed "s/, '$ext_id'//; s/'$ext_id', //; s/'$ext_id'//")
    [ "$enabled" = "[]" ] && enabled="@as []"
    gsettings set org.gnome.shell enabled-extensions "$enabled"
    ;;
esac

if [ "${1:-}" = "--purge" ]; then
    rm -rf "$XDG_CONFIG_HOME/tabletforge" "$XDG_CONFIG_HOME/tablet-center" \
        "$XDG_DATA_HOME/org.tabletforge.TabletForge" "$XDG_DATA_HOME/org.tabletforge.linux" \
        "$XDG_DATA_HOME/tablet-center"
fi

if [ -f /etc/udev/rules.d/70-tabletforge.rules ] || [ -f /etc/udev/rules.d/70-tablet-center.rules ]; then
    echo "Removing the udev rule needs administrator rights (sudo)."
    sudo rm -f /etc/udev/rules.d/70-tabletforge.rules /etc/udev/rules.d/70-tablet-center.rules
    sudo udevadm control --reload
fi
echo "TabletForge removed. You stay in the input group; leave it with: sudo gpasswd -d $USER input"
