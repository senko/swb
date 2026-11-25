#!/usr/bin/env bash
# Builds swb's FFmpeg (ADR 0047 section 1): `just ffmpeg`.
#
# Reads the pins of tools/ffmpeg/ffmpeg.env and the options of
# tools/ffmpeg/configure.txt, then:
#
# 1. downloads ffmpeg-VERSION.tar.xz and its .asc into out/ffmpeg/ when they
#    are missing (curl, https only);
# 2. checks the size and the SHA-256 (always) and the signature (when gpg is installed,
#    with a temporary key ring that holds only tools/ffmpeg/release-key.asc,
#    whose fingerprint must be the pinned one);
# 3. extracts, configures in out/ffmpeg/build-VERSION/, builds with
#    `make -j N` (the number of cores, at most 8) and installs into
#    out/ffmpeg/VERSION/;
# 4. writes out/ffmpeg/VERSION/swb-build.txt (version, SHA-256, configure
#    options), which the build script of crates/media-ffi checks.
#
# When out/ffmpeg/VERSION/swb-build.txt already matches the pins, it does
# nothing (the CI cache hit). It runs from any directory and writes only
# below out/ffmpeg/ (also its temporary files). Needs a C compiler, make,
# nasm, curl (for the download), xz and sha256sum; gpg is optional.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
pins="$root/tools/ffmpeg/ffmpeg.env"
options_file="$root/tools/ffmpeg/configure.txt"
key_file="$root/tools/ffmpeg/release-key.asc"

pin() {
    local value
    value="$(grep -E "^$1=" "$pins" | head -n 1 | cut -d= -f2-)"
    if [ -z "$value" ]; then
        echo "ffmpeg: $1 is missing in $pins" >&2
        exit 1
    fi
    printf '%s' "$value"
}

version="$(pin FFMPEG_VERSION)"
sha256="$(pin FFMPEG_SHA256)"
size="$(pin FFMPEG_SIZE)"
url="$(pin FFMPEG_URL)"
fingerprint="$(pin FFMPEG_KEY_FINGERPRINT)"

options=()
while IFS= read -r line; do
    case "$line" in
        '' | '#'*) ;;
        *) options+=("$line") ;;
    esac
done < "$options_file"

out="$root/out/ffmpeg"
tarball="$out/ffmpeg-$version.tar.xz"
prefix="$out/$version"
src="$out/src-$version"
build="$out/build-$version"
stamp="$prefix/swb-build.txt"

expected_stamp() {
    echo "# Written by tools/ffmpeg/build.sh; checked by crates/media-ffi/build.rs."
    echo "version=$version"
    echo "sha256=$sha256"
    echo "configure=${options[*]}"
}

if [ -f "$stamp" ] && [ -f "$prefix/lib/libavcodec.a" ] \
    && [ "$(cat "$stamp")" = "$(expected_stamp)" ]; then
    echo "ffmpeg: $prefix is up to date (FFmpeg $version)"
    exit 0
fi

mkdir -p "$out"
# configure and the compilers write temporary files; keep them in out/ffmpeg.
export TMPDIR="$out/tmp"
mkdir -p "$TMPDIR"

download() {
    local name="$1"
    if [ ! -f "$out/$name" ]; then
        echo "ffmpeg: downloading $url/$name"
        curl --fail --location --proto '=https' --tlsv1.2 --silent --show-error \
            --output "$out/$name.part" "$url/$name"
        mv "$out/$name.part" "$out/$name"
    fi
}

download "ffmpeg-$version.tar.xz"
# The size first: a wrong download is rejected before it is hashed.
have_size="$(stat -c %s "$tarball")"
if [ "$have_size" != "$size" ]; then
    echo "ffmpeg: $tarball has $have_size bytes, not the pinned $size; delete the file and run again" >&2
    exit 1
fi
echo "$sha256  $tarball" | sha256sum --check --quiet - || {
    echo "ffmpeg: the SHA-256 of $tarball is not the pinned one; delete the file and run again" >&2
    exit 1
}

if command -v gpg > /dev/null; then
    download "ffmpeg-$version.tar.xz.asc"
    gnupg="$(mktemp -d "$TMPDIR/gnupg.XXXXXX")"
    stop_gpg() {
        gpgconf --homedir "$gnupg" --kill all > /dev/null 2>&1 || true
        rm -rf "$gnupg"
    }
    trap stop_gpg EXIT
    gpg --homedir "$gnupg" --batch --quiet --import "$key_file" 2> /dev/null
    have="$(gpg --homedir "$gnupg" --batch --with-colons --fingerprint | grep '^fpr:' | head -n 1 | cut -d: -f10)"
    if [ "$have" != "$fingerprint" ]; then
        echo "ffmpeg: $key_file has the fingerprint $have, not the pinned $fingerprint" >&2
        exit 1
    fi
    status="$(gpg --homedir "$gnupg" --batch --status-fd 1 --verify \
        "$tarball.asc" "$tarball" 2> /dev/null || true)"
    # VALIDSIG's last field is the fingerprint of the primary key, so a
    # signature by a subkey of the pinned key is accepted (the key ring holds
    # only the pinned key and its subkeys).
    if ! awk -v fp="$fingerprint" \
        '$1 == "[GNUPG:]" && $2 == "VALIDSIG" && $NF == fp { ok = 1 } END { exit !ok }' \
        <<< "$status"; then
        echo "ffmpeg: the signature $tarball.asc does not verify with the pinned key" >&2
        exit 1
    fi
    stop_gpg
    trap - EXIT
    echo "ffmpeg: signature good ($fingerprint)"
else
    echo "ffmpeg: gpg is not installed: the signature is not checked (the SHA-256 is)"
fi

rm -rf "$src" "$build" "$prefix"
mkdir -p "$src" "$build"
tar -xJf "$tarball" -C "$src" --strip-components=1

jobs="$(nproc 2> /dev/null || echo 4)"
if [ "$jobs" -gt 8 ]; then
    jobs=8
fi

echo "ffmpeg: configure ${options[*]}"
# configure writes into the current directory (ffbuild/, config.h): the
# build directory.
(cd "$build" && "$src/configure" --prefix="$prefix" "${options[@]}") > "$out/configure-$version.log"
if ! grep -q "^License: LGPL version 2.1 or later" "$out/configure-$version.log"; then
    echo "ffmpeg: configure did not report LGPL-2.1-or-later; see $out/configure-$version.log" >&2
    exit 1
fi

echo "ffmpeg: make -j $jobs"
make -C "$build" -j "$jobs" > "$out/make-$version.log" 2>&1 || {
    tail -n 40 "$out/make-$version.log" >&2
    exit 1
}
make -C "$build" install > /dev/null

rm -rf "$src" "$build" "$TMPDIR"
expected_stamp > "$stamp.part"
mv "$stamp.part" "$stamp"
echo "ffmpeg: installed FFmpeg $version into $prefix"
