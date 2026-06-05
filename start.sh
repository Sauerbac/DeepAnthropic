#!/usr/bin/env bash
# Launches cc-router, then Claude Code pointed at it. Stops the router on exit.
# Claude Code runs in your CURRENT directory (so the alias works from anywhere).
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
exe="$here/target/release/cc-router.exe"

[ -f "$here/config.toml" ] || { echo "config.toml missing. Run: cp config.toml.example config.toml  and fill in your tokens."; exit 1; }
if [ ! -f "$exe" ]; then echo "Building release binary..."; ( cd "$here" && cargo build --release ); fi

port="$(grep -E '^[[:space:]]*port[[:space:]]*=' "$here/config.toml" | head -1 | grep -oE '[0-9]+' || true)"
port="${port:-8788}"
base="http://127.0.0.1:$port"

started_ours=false

if curl -s -o /dev/null "$base"; then
    echo "Router already running on :$port (reusing existing instance)."
else
    started_ours=true
    ( cd "$here" && exec "$exe" ) </dev/null &>"$here/router.log" & router_pid=$!
    for _ in $(seq 1 25); do curl -s -o /dev/null "$base" && break; sleep 0.2; done
    echo "Started router on :$port."
fi

if $started_ours; then
    cleanup() { kill "$router_pid" 2>/dev/null || true; }
    trap cleanup EXIT INT TERM
fi

export ANTHROPIC_BASE_URL="$base"
export ANTHROPIC_AUTH_TOKEN="dummy-local-token"   # real upstream creds live in the router
echo "Launching Claude Code (default tier = deepseek-v4-pro; /model opus for real Opus)."
claude "$@"
