#!/usr/bin/env bash
# Writes the layout dump, DOM dump and full-page screenshots of every page
# fixture, and the layout dump and a screenshot of every layout test, into
# OUT_DIR. Two snapshots of the same tree are byte-identical, so
# `diff -r A B` shows every rendering change between two trees (for
# example before and after a refactoring). Run it from the repository
# root after `just build` (`just snapshot OUT_DIR` does both).
#
# Usage: tools/snapshot.sh OUT_DIR [SWB_BINARY]
set -euo pipefail
out=$1
swb=${2:-target/release/swb}
mkdir -p "$out"
rm -f "$out/failures"

dump() { # NAME ARGS...: layout dump
  local name=$1
  shift
  "$swb" --headless --test-fonts --timeout 60 "$@" --dump-layout >"$out/$name.layout" 2>/dev/null ||
    echo "failed: $name layout" >>"$out/failures"
}
shot() { # NAME ARGS...: full-page screenshot
  local name=$1
  shift
  "$swb" --headless --test-fonts --timeout 60 "$@" --full-page --screenshot "$out/$name.png" >/dev/null 2>&1 ||
    echo "failed: $name screenshot" >>"$out/failures"
}

for dir in fixtures/pages/*/; do
  name=$(basename "$dir")
  url=$(python3 -c "import json, sys; print(json.load(open(sys.argv[1]))['url'])" "$dir/fixture.json")
  for size in 1280x800 700x900; do
    dump "page-$name-$size" --replay "$dir" --size "$size" "$url"
    shot "page-$name-$size" --replay "$dir" --size "$size" "$url"
  done
  shot "page-$name-1280x800@2" --replay "$dir" --size 1280x800 --scale 2 "$url"
  "$swb" --headless --test-fonts --replay "$dir" --dump-dom "$url" >"$out/page-$name.dom" 2>/dev/null ||
    echo "failed: $name DOM" >>"$out/failures"
done

for file in tests/layout/*.html; do
  name=$(basename "$file" .html)
  dump "layout-$name" "$file"
  shot "layout-$name" "$file"
done

if [[ -e "$out/failures" ]]; then
  cat "$out/failures" >&2
  exit 1
fi
echo "snapshot written to $out"
