#!/usr/bin/env bash
# Start the game server and video bridge, open the stream once a seat renders,
# and tear both down on exit. Expects apps/web/dist and the server binary built.
set -euo pipefail

game_port="${1:?game port}"
stream_port="${2:?stream port}"

pids=()
trap 'kill "${pids[@]}" 2>/dev/null || true' EXIT INT TERM

CITY_BIND="127.0.0.1:${game_port}" cargo run -q -p city-server &
pids+=($!)
CITY_GAME_URL="http://127.0.0.1:${game_port}" CITY_STREAM_PORT="${stream_port}" \
  bun run --cwd apps/stream start &
pids+=($!)

until curl -sf "http://127.0.0.1:${stream_port}/api/status.json" | grep -Eq '"(idle|ready)"'; do
  sleep 1
done
open "http://127.0.0.1:${stream_port}/view"
wait
