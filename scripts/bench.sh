#!/bin/bash
# Compares Encore with Apple's Music app: launch time, memory and CPU while idle and while
# playing, and size on disk. Both apps are quit and relaunched, and played silently.
#
#   ./scripts/build.sh --features tour   # the playing measurement uses the tour's silent playback
#   ./scripts/bench.sh                   # RUNS=5 SETTLE=20 SAMPLE=30 to change the defaults
#
# Sign in to Apple Music in both apps first so both play full songs. Results go to stdout
# and target/bench/results.md.
set -euo pipefail
cd "$(dirname "$0")/.."

RUNS=${RUNS:-5}
SETTLE=${SETTLE:-20}
SAMPLE=${SAMPLE:-30}
ENCORE_APP=$(cd "$(dirname "${ENCORE_APP:-target/Encore.app}")" && pwd)/$(basename "${ENCORE_APP:-target/Encore.app}")
MUSIC_APP=/System/Applications/Music.app

mkdir -p target/bench
tool=target/bench/benchtool
swiftc -O scripts/bench/benchtool.swift -o "$tool"

quit() {
  osascript -e "quit app \"$1\"" >/dev/null 2>&1 || true
  for _ in $(seq 50); do pgrep -x "$1" >/dev/null || return 0; sleep 0.2; done
  pkill -x "$1" || true
  sleep 1
}

median() { sort -n | awk '{ a[NR] = $1 } END { print (NR % 2) ? a[(NR + 1) / 2] : (a[NR / 2] + a[NR / 2 + 1]) / 2 }'; }

# Prints "<footprint MB> <average CPU %>" for the app and its helpers over SAMPLE seconds.
measure() {
  local pid; pid=$(pgrep -x "$1" | head -1)
  read -r _ cpu0 _ < <("$tool" stats "$pid")
  sleep "$SAMPLE"
  read -r mem cpu1 procs < <("$tool" stats "$pid")
  awk -v m="$mem" -v c0="$cpu0" -v c1="$cpu1" -v s="$SAMPLE" -v p="$procs" \
    'BEGIN { printf "%.0f %.1f %d\n", m / 1048576, (c1 - c0) / (s * 1e9) * 100, p }'
}

launch_times() { # <process name> <open args…>
  local name=$1; shift
  for _ in $(seq "$RUNS"); do
    quit "$name"
    sleep 2
    "$tool" launch "$name" "$@" || { echo "launching $name failed" >&2; exit 1; }
  done | median
}

music_was_running=$(pgrep -x Music >/dev/null && echo 1 || echo 0)
music_volume=$(osascript -e 'tell application "Music" to sound volume' 2>/dev/null || echo 100)
restore() {
  osascript -e "tell application \"Music\" to set sound volume to $music_volume" >/dev/null 2>&1 || true
  [ "$music_was_running" = 1 ] || quit Music
}
trap restore EXIT

echo "Launch (median of $RUNS)…" >&2
encore_launch=$(launch_times Encore -a "$ENCORE_APP")
[ -n "$encore_launch" ] || exit 1
sleep "$SETTLE"
read -r encore_idle_mem encore_idle_cpu _ < <(measure Encore)

music_launch=$(launch_times Music -a "$MUSIC_APP")
[ -n "$music_launch" ] || exit 1
sleep "$SETTLE"
read -r music_idle_mem music_idle_cpu _ < <(measure Music)

echo "Playing…" >&2
# A bare `play` does nothing right after launch; pick a song (streamed for subscribers).
osascript -e 'tell application "Music"' -e 'set sound volume to 0' -e 'play (some track of library playlist 1)' -e 'end tell' >/dev/null
sleep "$SETTLE"
read -r music_play_mem music_play_cpu music_procs < <(measure Music)
osascript -e 'tell application "Music" to pause' >/dev/null

quit Encore
open -a "$ENCORE_APP" --env ENCORE_TOUR=1 --env ENCORE_TOUR_ONLY=none --env ENCORE_TOUR_PLAY=1
sleep "$SETTLE"
read -r encore_play_mem encore_play_cpu encore_procs < <(measure Encore)
quit Encore

encore_size=$(du -sm "$ENCORE_APP" | cut -f1)
music_size=$(du -sm "$MUSIC_APP" | cut -f1)

machine="$(sysctl -n machdep.cpu.brand_string), $(($(sysctl -n hw.memsize) / 1073741824)) GB, macOS $(sw_vers -productVersion)"
music_version=$(defaults read "$MUSIC_APP/Contents/Info.plist" CFBundleShortVersionString)
encore_version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)

tee target/bench/results.md <<EOF
$machine · Encore $encore_version · Music $music_version · $(date +%Y-%m-%d)

| | Encore | Music |
| --- | ---: | ---: |
| Launch to first window (warm, median of $RUNS) | ${encore_launch} ms | ${music_launch} ms |
| Memory, idle | ${encore_idle_mem} MB | ${music_idle_mem} MB |
| Memory, playing | ${encore_play_mem} MB | ${music_play_mem} MB |
| CPU, idle (${SAMPLE} s average) | ${encore_idle_cpu}% | ${music_idle_cpu}% |
| CPU, playing (${SAMPLE} s average) | ${encore_play_cpu}% | ${music_play_cpu}% |
| Processes while playing | ${encore_procs} | ${music_procs} |
| Size on disk | ${encore_size} MB | ${music_size} MB |
EOF
