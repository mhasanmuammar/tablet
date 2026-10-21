#!/bin/sh
# Opens TabletForge for development. The window loads ui/ from a local
# server, so edits to ui/ show up after Ctrl+R without rebuilding.
# Usage: app/dev.sh            -> UI only (talks to the installed tabletforged.service)
#        app/dev.sh --daemon   -> also runs a local tabletforged build if the service is stopped
set -eu
cd "$(dirname "$0")"

# Reuse a server that is already up (another dev.sh, or a manual one).
pids=""
if ! curl -s -o /dev/null http://127.0.0.1:8765/index.html; then
	bun tools/dev-server.ts &
	pids=$!
fi
if [ "${1:-}" = "--daemon" ] && systemctl --user is-active -q tabletforged.service; then
	echo "tabletforged.service is already running; using it (stop it to test a local build)."
elif [ "${1:-}" = "--daemon" ]; then
	cargo build -q -p tabletforged
	./target/debug/tabletforged &
	pids="$pids $!"
fi
trap 'kill $pids 2>/dev/null' EXIT INT TERM
cargo run -q -p tabletforge --no-default-features
