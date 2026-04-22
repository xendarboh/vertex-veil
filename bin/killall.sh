#!/usr/bin/env sh
set -eu

PORTS="9000 9001 9002 9003"

for port in $PORTS; do
  if command -v lsof >/dev/null 2>&1; then
    pids=$(lsof -tiUDP:"$port" || true)
  elif command -v fuser >/dev/null 2>&1; then
    pids=$(fuser "$port"/udp 2>/dev/null || true)
  else
    echo "bin/kill.sh requires lsof or fuser" >&2
    exit 1
  fi

  [ -n "$pids" ] || continue

  kill $pids 2>/dev/null || true
done
