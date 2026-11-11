#!/usr/bin/env bash
# Start the server (it renders and streams), open the viewer once it listens,
# and tear it down on exit. Expects apps/viewer/dist built.
set -euo pipefail

game_port="${1:?game port}"

pid=""
trap '[ -n "$pid" ] && kill "$pid" 2>/dev/null || true' EXIT INT TERM

CITY_BIND="127.0.0.1:${game_port}" cargo run -q -p city-server &
pid=$!

until curl -sf -o /dev/null "http://127.0.0.1:${game_port}/"; do
  sleep 1
done
open "http://127.0.0.1:${game_port}/"
wait
