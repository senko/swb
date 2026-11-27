#!/usr/bin/env python3
"""Generates the test media of `fixtures/media/` with the ffmpeg CLI.

The files are committed, so builds and tests never run this script. Run it
(`python3 tools/media/generate.py [--out DIR] [--ffmpeg PATH] [NAME...]`) to
remake them, for example after a change of the table below. It needs an ffmpeg
with libx264, libvpx, libopus, libvorbis and libmp3lame (the static build of
johnvansickle.com, N-66244, has them). Only the standard library is used.

Content (FFmpeg's test sources, no files as input):

- Video: `testsrc2` at 320x240 and 25 fps. It draws the time ("00:00:01.200")
  and the frame number in the top left corner and moving color bars, so that
  frames at different times differ in the pixels.
- Audio: a stereo pair of sines, 440 Hz on the left channel and 660 Hz on the
  right one (`sine` sources joined by `amerge`).

Files are 2 s long, except the ones with `long` in their name (10 s, a key
frame every second). The files are made bit-exact (`-bitexact`, no metadata),
so a second run on the same ffmpeg gives the same bytes.

Besides the media files the table has two flat-color posters, three files over
the limits of ADR 0047 section 11 (a frame wider than 16384 pixels, a frame of
more than 8192 x 4352 pixels, 17 streams) and the hostile files (`derived`):
truncated, without an index, with a corrupt middle, with 600 damaged bytes at
30, 60, 80 and 90 percent of the file (`-damaged-N`, see `damage`) and random
bytes. These
are byte changes of the media files, made in Python with fixed seeds, after
the ffmpeg runs. The files are described in docs/testing.md
("Media: test files, real files and probes") and measured against Chromium in
docs/media-diagnosis.md.
"""

from __future__ import annotations

import argparse
import random
import shutil
import subprocess
import sys
from collections.abc import Callable
from pathlib import Path

W, H, FPS = 320, 240, 25

# Bit-exact output with no metadata tags. The external encoders still write
# their own identification (x264's settings SEI, "LAME3.100", "libVorbis"),
# which is the same on every run of the same ffmpeg.
BITEXACT = [
    "-fflags",
    "+bitexact",
    "-flags:v",
    "+bitexact",
    "-flags:a",
    "+bitexact",
    "-map_metadata",
    "-1",
]


def video_src(dur: float, size: tuple[int, int] = (W, H)) -> list[str]:
    return ["-f", "lavfi", "-i", f"testsrc2=size={size[0]}x{size[1]}:rate={FPS}:duration={dur}"]


def audio_graph(dur: float, rate: int = 44100) -> str:
    """A filter graph with the stereo sines as output `[a]` (left 440 Hz, right 660 Hz)."""
    return (
        f"sine=frequency=440:sample_rate={rate}:duration={dur}[l];"
        f"sine=frequency=660:sample_rate={rate}:duration={dur}[r];"
        "[l][r]amerge=inputs=2[a]"
    )


def av(dur: float, rate: int = 44100) -> list[str]:
    """Inputs for a file with video (input 0) and audio (`[a]`)."""
    return [
        *video_src(dur),
        "-filter_complex",
        audio_graph(dur, rate),
        "-map",
        "0:v",
        "-map",
        "[a]",
    ]


def a_only(dur: float, rate: int = 44100) -> list[str]:
    return ["-filter_complex", audio_graph(dur, rate), "-map", "[a]"]


H264 = ["-c:v", "libx264", "-preset", "medium", "-crf", "30", "-pix_fmt", "yuv420p"]
H264_HIGH = [*H264, "-profile:v", "high", "-level", "3.0", "-bf", "2", "-g", str(FPS)]
H264_BASE = [*H264, "-profile:v", "baseline", "-level", "3.0", "-bf", "0", "-g", str(FPS)]
AAC = ["-c:a", "aac", "-b:a", "64k", "-ar", "44100"]
VP8 = [
    "-c:v",
    "libvpx",
    "-b:v",
    "0",
    "-crf",
    "30",
    "-g",
    str(FPS),
    "-auto-alt-ref",
    "0",
    "-pix_fmt",
    "yuv420p",
]
VP9 = ["-c:v", "libvpx-vp9", "-b:v", "0", "-crf", "36", "-g", str(FPS), "-pix_fmt", "yuv420p"]
VORBIS = ["-c:a", "libvorbis", "-q:a", "2"]
OPUS = ["-c:a", "libopus", "-b:a", "48k", "-ar", "48000"]


# Frame sizes of the short untagged files (the 2 s files above keep 640x480,
# 720x576 and 1280x720).
UNTAGGED_SIZES = [
    (1280, 544),
    (720, 720),
    (960, 720),
    (1024, 576),
    (960, 540),
    (1280, 480),
    # Heights between 576 and 720, and a large frame.
    (1280, 600),
    (1024, 640),
    (1280, 704),
    (1280, 718),
    (1920, 1080),
]


# Colors: the pixel format conversion with a given matrix and the tags that
# say which matrix a decoder must use. `testsrc2` makes yuv420p directly, so
# the scale filter converts its frames from the BT.601 matrix that ffmpeg
# assumes for them to the target one.
def tags(name: str, rng: str = "tv") -> list[str]:
    """The color tags of a stream: matrix, primaries and transfer by `name`."""
    return [
        "-colorspace",
        name,
        "-color_primaries",
        name,
        "-color_trc",
        name,
        "-color_range",
        rng,
    ]


def scale_matrix(matrix: str, rng: str = "tv") -> list[str]:
    return [
        "-vf",
        f"scale=in_color_matrix=bt601:out_color_matrix={matrix}:out_range={rng},format=yuv420p",
    ]


def entries() -> dict[str, list[str]]:
    """The file name (relative to the output directory) and the ffmpeg arguments (after `-y`)."""
    e: dict[str, list[str]] = {}
    # MP4: H.264 High with B-frames + AAC-LC; H.264 Baseline + AAC-LC.
    e["mp4-h264-high-aac.mp4"] = [*av(2), *H264_HIGH, *AAC, "-movflags", "+faststart"]
    e["mp4-h264-baseline-aac.mp4"] = [*av(2), *H264_BASE, *AAC, "-movflags", "+faststart"]
    # MP4 with the index at the end (what a plain `ffmpeg` writes; the loader must seek).
    e["mp4-h264-high-aac-moov-at-end.mp4"] = [*av(2), *H264_HIGH, *AAC]
    # M4A: AAC-LC in MP4.
    e["m4a-aac.m4a"] = [*a_only(2), *AAC, "-vn", "-movflags", "+faststart"]
    # WebM.
    e["webm-vp8-vorbis.webm"] = [*av(2), *VP8, *VORBIS]
    e["webm-vp9-opus.webm"] = [*av(2), *VP9, *OPUS]
    # Ogg: Vorbis, Opus, FLAC.
    e["ogg-vorbis.ogg"] = [*a_only(2), *VORBIS, "-f", "ogg"]
    e["ogg-opus.opus"] = [*a_only(2), *OPUS, "-f", "ogg"]
    e["ogg-flac.oga"] = [*a_only(2, 22050), "-c:a", "flac", "-ar", "22050", "-f", "ogg"]
    # Bare audio formats.
    e["mp3.mp3"] = [
        *a_only(2),
        "-c:a",
        "libmp3lame",
        "-b:a",
        "64k",
        "-ar",
        "44100",
        "-id3v2_version",
        "0",
        "-write_xing",
        "1",
    ]
    e["flac.flac"] = [*a_only(2, 22050), "-c:a", "flac", "-ar", "22050"]
    e["wav-pcm-s16.wav"] = [*a_only(2, 22050), "-c:a", "pcm_s16le", "-ar", "22050"]
    e["aac-adts.aac"] = [*a_only(2), *AAC, "-f", "adts"]
    # Video only.
    e["mp4-h264-video-only.mp4"] = [*video_src(2), *H264_HIGH, "-an", "-movflags", "+faststart"]
    e["webm-vp8-video-only.webm"] = [*video_src(2), *VP8, "-an"]
    # An odd frame size (H.264 4:2:0 cannot have one).
    # (`testsrc2` rounds its size to an even number, so the filter crops it.)
    e["webm-vp8-321x241.webm"] = [
        *video_src(2, (322, 242)),
        "-vf",
        "crop=321:241:0:0:exact=1",
        *VP8,
        "-an",
    ]
    e["webm-vp9-321x241.webm"] = [
        *video_src(2, (322, 242)),
        "-vf",
        "crop=321:241:0:0:exact=1",
        *VP9,
        "-an",
    ]
    # Sizes whose width is not a multiple of 16 (M14 feature 7a fix round 2:
    # swscale converted the last columns separately and wrote past the end of
    # the destination for these). Odd sizes in one direction only (VP8; H.264
    # 4:2:0 cannot crop to an odd size), even sizes with H.264 in 4:2:0, 4:2:2,
    # 4:4:4 and 10-bit 4:2:0, 0.4 s each.
    for crop_w, crop_h in ((321, 240), (320, 241)):
        e[f"webm-vp8-{crop_w}x{crop_h}.webm"] = [
            *video_src(0.4, (crop_w + 1, crop_h + 1)),
            "-vf",
            f"crop={crop_w}:{crop_h}:0:0:exact=1",
            *VP8,
            "-an",
        ]
    # These two have smooth color gradients (the `gradients` source, fixed
    # seed), so that the last columns show a conversion error: the bars of
    # `testsrc2` are flat there.
    for width, height in ((854, 480), (322, 242)):
        e[f"mp4-h264-{width}x{height}.mp4"] = [
            "-f",
            "lavfi",
            "-i",
            f"gradients=size=854x480:rate={FPS}:duration=0.4:seed=7:speed=0.005:nb_colors=4",
            "-vf",
            f"scale={width}:{height},setsar=1,format=yuv420p",
            *H264_HIGH,
            "-an",
            "-movflags",
            "+faststart",
        ]
    for pix_fmt, label in (("yuv444p", "444"), ("yuv422p", "422"), ("yuv420p10le", "10bit")):
        e[f"mp4-h264-322x242-{label}.mp4"] = [
            *video_src(0.4, (322, 242)),
            # No `-profile:v`: x264 picks the profile that the format needs.
            *["-c:v", "libx264", "-preset", "medium", "-crf", "30", "-pix_fmt", pix_fmt],
            "-an",
            "-movflags",
            "+faststart",
        ]
    # Color tags. 709 and 601: content converted with that matrix and tagged;
    # untagged-709: converted with 709 and not tagged (the decoder must guess);
    # full range: JPEG range, tagged.
    e["mp4-h264-bt709.mp4"] = [
        *video_src(2),
        *scale_matrix("bt709"),
        *H264_HIGH,
        *tags("bt709"),
        "-an",
        "-movflags",
        "+faststart",
    ]
    e["mp4-h264-bt601.mp4"] = [
        *video_src(2),
        *scale_matrix("smpte170m"),
        *H264_HIGH,
        *tags("smpte170m"),
        "-an",
        "-movflags",
        "+faststart",
    ]
    e["mp4-h264-untagged-bt709.mp4"] = [
        *video_src(2),
        *scale_matrix("bt709"),
        *H264_HIGH,
        "-an",
        "-movflags",
        "+faststart",
    ]
    # Untagged, content converted with 709, at other sizes: which matrix does a
    # decoder guess for an untagged stream (by frame size)?
    for width, height in ((640, 480), (720, 576), (1280, 720)):
        e[f"mp4-h264-untagged-bt709-{width}x{height}.mp4"] = [
            *video_src(2, (width, height)),
            *scale_matrix("bt709"),
            *H264_HIGH,
            "-level",
            "4.0",
            "-crf",
            "40",
            "-an",
            "-movflags",
            "+faststart",
        ]
    # Non-square pixels (M14 feature 7a: the natural size is the display size):
    # 160x120 with a sample aspect ratio of 2:1 and of 1:2.
    for sar in ("2x1", "1x2"):
        e[f"mp4-h264-sar-{sar}.mp4"] = [
            *video_src(0.6, (160, 120)),
            "-vf",
            f"setsar={sar.replace('x', '/')},format=yuv420p",
            *H264_HIGH,
            "-an",
            "-movflags",
            "+faststart",
        ]
    # The boundary of that guess (M14 feature 7a): 0.6 s (15 frames) at sizes
    # that separate "lines", "width" and "area" as the rule.
    for width, height in UNTAGGED_SIZES:
        e[f"mp4-h264-untagged-bt709-{width}x{height}.mp4"] = [
            *video_src(0.6, (width, height)),
            *scale_matrix("bt709"),
            *H264_HIGH,
            "-level",
            "5.1",
            "-crf",
            "40",
            "-an",
            "-movflags",
            "+faststart",
        ]
    e["mp4-h264-fullrange-bt709.mp4"] = [
        *video_src(2),
        *scale_matrix("bt709", "pc"),
        *H264_HIGH,
        *tags("bt709", "pc"),
        "-an",
        "-movflags",
        "+faststart",
    ]
    # A non-zero start time: the first frame and the first sample are at 1.5 s.
    e["webm-vp8-vorbis-start1500.webm"] = [*av(2), *VP8, *VORBIS, "-output_ts_offset", "1.5"]
    e["mp4-h264-aac-start1500.mp4"] = [
        *av(2),
        *H264_HIGH,
        *AAC,
        "-output_ts_offset",
        "1.5",
        "-movflags",
        "+faststart",
    ]
    # Audio far from the video in the file (ADR 0047 section 7, the pipeline
    # stalled on its owner): 2 s of video from 0 s and 2 s of audio from 3 s
    # (`-itsoffset` on an audio input of its own). The muxer writes the
    # packets in time order, so every video packet comes before the first
    # audio packet; with a small packet bound the demuxer fills its queues
    # before it reads any audio, and the ring fills before the video ends.
    e["mp4-h264-aac-late-audio.mp4"] = [
        *video_src(2),
        "-itsoffset",
        "3",
        "-f",
        "lavfi",
        "-i",
        audio_graph(2).replace("[a]", "[out0]"),
        "-map",
        "0:v",
        "-map",
        "1:a",
        *H264_HIGH,
        *AAC,
        "-movflags",
        "+faststart",
    ]
    # Rows for the timeline rules (M14 feature 6a, `swb_media::timeline`):
    # a fragmented MP4 (an empty `moov`, one `moof` per key frame), an MP4
    # with B-frames and without an edit list (the first frame has a time
    # offset that no edit list compensates), and Opus in MP4.
    e["mp4-h264-aac-fragmented.mp4"] = [
        *av(2),
        *H264_HIGH,
        *AAC,
        "-movflags",
        "+frag_keyframe+empty_moov",
    ]
    e["mp4-h264-aac-bframes-no-edit-list.mp4"] = [
        *av(2),
        *H264_HIGH,
        *AAC,
        "-use_editlist",
        "0",
        "-movflags",
        "+faststart",
    ]
    e["mp4-opus.mp4"] = [*a_only(2), *OPUS, "-strict", "-2", "-movflags", "+faststart"]
    # Longer files for seeking: 10 s, a key frame every second.
    e["mp4-h264-aac-long.mp4"] = [*av(10), *H264_HIGH, *AAC, "-movflags", "+faststart"]
    # A flat picture, so that the pixels behind the default controls are the
    # same at every time (M14 feature 9: the pixel tests of the controls).
    e["mp4-h264-aac-flat-long.mp4"] = [
        "-f",
        "lavfi",
        "-i",
        f"color=c=0x336699:size={W}x{H}:rate={FPS}:duration=10",
        "-filter_complex",
        audio_graph(10),
        "-map",
        "0:v",
        "-map",
        "[a]",
        *H264_HIGH,
        *AAC,
        "-movflags",
        "+faststart",
    ]
    e["mp3-long.mp3"] = [
        *a_only(10),
        "-c:a",
        "libmp3lame",
        "-b:a",
        "48k",
        "-ar",
        "44100",
        "-id3v2_version",
        "0",
        "-write_xing",
        "1",
    ]
    # Hostile limits (ADR 0047 section 11): one black VP9 frame wider than
    # 16384 pixels, one with more pixels than 8192 x 4352, and a Matroska
    # file with 17 audio streams (more than `max_streams`, 16).
    for width, height in ((16400, 16), (8192, 4416)):
        e[f"webm-vp9-{width}x{height}.webm"] = [
            "-f",
            "lavfi",
            "-i",
            f"color=c=black:size={width}x{height}:rate={FPS}:duration=0.04",
            "-frames:v",
            "1",
            *VP9,
            "-an",
        ]
    streams = 17
    e[f"mka-pcm-{streams}-streams.mka"] = [
        "-f",
        "lavfi",
        "-i",
        "sine=frequency=440:sample_rate=8000:duration=0.02",
        "-filter_complex",
        f"[0:a]asplit={streams}" + "".join(f"[a{i}]" for i in range(streams)),
        *[arg for i in range(streams) for arg in ("-map", f"[a{i}]")],
        "-c:a",
        "pcm_s16le",
    ]
    # Posters: one flat color (#336699, a color that `testsrc2` does not draw).
    for size in ("320x240", "200x100"):
        e[f"poster-{size}.png"] = [
            "-f",
            "lavfi",
            "-i",
            f"color=c=0x336699:size={size}",
            "-frames:v",
            "1",
            "-update",
            "1",
        ]
    return e


def damage(fraction: float) -> Callable[[bytes], bytes]:
    """The byte change that puts 600 bytes of a linear congruential
    generator (seed 16) at `fraction` of the file. The engine test
    `late_failures_follow_the_damage` makes the same bytes (also for the
    10 s file, whose damaged copy is too big to commit). An MP4 file has its
    index first, so the damage lies in the sample data and the file loads."""

    def change(data: bytes) -> bytes:
        x, noise = 16, bytearray()
        for _ in range(600):
            x = (x * 1103515245 + 12345) & 0x7FFFFFFF
            noise.append((x >> 16) & 0xFF)
        at = int(len(data) * fraction)
        return data[:at] + bytes(noise) + data[at + 600 :]

    return change


def derived() -> dict[str, tuple[str, Callable[[bytes], bytes]]]:
    """Hostile files: the name, the file that it comes from and the byte
    change. They are made after the files above and need no ffmpeg."""

    def cut(size: int) -> Callable[[bytes], bytes]:
        return lambda data: data[:size]

    def corrupt(data: bytes) -> bytes:
        # 2000 pseudo-random bytes (fixed seed) in the middle of the data.
        middle = len(data) // 2
        return data[:middle] + random.Random(14).randbytes(2000) + data[middle + 2000 :]

    return {
        # Damage at a fraction of the file (percent in the name): the file
        # loads and the playback fails at the damage.
        **{
            f"mp4-h264-high-aac-damaged-{p}.mp4": ("mp4-h264-high-aac.mp4", damage(p / 100))
            for p in (30, 60, 80, 90)
        },
        # The index is first, the data is cut: plays the start, then fails.
        "mp4-h264-high-aac-truncated.mp4": ("mp4-h264-high-aac.mp4", cut(30000)),
        # The index is last and cut off: the file has no index at all.
        "mp4-h264-high-aac-no-moov.mp4": ("mp4-h264-high-aac-moov-at-end.mp4", cut(40000)),
        "webm-vp8-vorbis-truncated.webm": ("webm-vp8-vorbis.webm", cut(40000)),
        "mp4-h264-high-aac-corrupt.mp4": ("mp4-h264-high-aac.mp4", corrupt),
        "webm-vp8-vorbis-corrupt.webm": ("webm-vp8-vorbis.webm", corrupt),
        "mp3-truncated.mp3": ("mp3.mp3", cut(8000)),
        # 4096 pseudo-random bytes with the name of an MP4 file.
        "garbage.mp4": ("mp4-h264-high-aac.mp4", lambda _: random.Random(15).randbytes(4096)),
    }


def run(ffmpeg: str, out: Path, name: str, args: list[str]) -> None:
    target = out / name
    cmd = [ffmpeg, "-hide_banner", "-loglevel", "error", "-y", *args, *BITEXACT, str(target)]
    print(" ".join(cmd), file=sys.stderr)
    subprocess.run(cmd, check=True)
    print(f"{name}  {target.stat().st_size} bytes")


def main() -> int:
    root = Path(__file__).resolve().parents[2]
    ap = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    ap.add_argument("--out", type=Path, default=root / "fixtures" / "media")
    ap.add_argument("--ffmpeg", default=shutil.which("ffmpeg") or "ffmpeg")
    ap.add_argument("names", nargs="*", help="only these files")
    ns = ap.parse_args()
    table = entries()
    hostile = derived()
    names = ns.names or [*table, *hostile]
    unknown = [n for n in names if n not in table and n not in hostile]
    if unknown:
        print("unknown file: " + ", ".join(unknown), file=sys.stderr)
        return 2
    ns.out.mkdir(parents=True, exist_ok=True)
    total = 0
    for name in names:
        if name in table:
            run(ns.ffmpeg, ns.out, name, table[name])
        else:
            source, change = hostile[name]
            (ns.out / name).write_bytes(change((ns.out / source).read_bytes()))
            print(f"{name}  {(ns.out / name).stat().st_size} bytes (from {source})")
        total += (ns.out / name).stat().st_size
    print(f"total {total} bytes in {len(names)} files")
    return 0


if __name__ == "__main__":
    sys.exit(main())
