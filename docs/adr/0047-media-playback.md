# ADR 0047: Media playback

- Status: accepted
- Date: 2026-11-24

## Context

M14 (roadmap, "M14: Media playback") adds audio and video playback. The
owner decided on 2026-11-20 and 2026-11-24 (ground rules): FFmpeg
(libavformat, libavcodec, libswscale, libswresample) demuxes, decodes and
converts; it is built from a pinned source release with only the needed
demuxers and decoders, without network code and GPL parts, and linked
statically (LGPL-2.1 or later, an exception to ADR 0003 for builds with
media); audio output uses PipeWire (MIT, linked dynamically); a Cargo
feature turns media on, and without it swb builds without FFmpeg and
PipeWire; CI builds and tests both; FFmpeg runs in the browser process
without a sandbox. swb writes the media element, the media clock, audio
and video synchronization and audio output.

The diagnosis ([media-diagnosis.md](../media-diagnosis.md)) measured
Chromium 148: the `canPlayType` answers, the states and events, the
autoplay rules, what Chromium fetches, the frames at paused times, the
poster rules, the controls and the budgets. Its section 11 lists the
questions for this ADR.

Before M14, swb fetches no media (ADR 0020): `<video>` shows its poster
and the controls of ADR 0020 drawn as before playback;
`HTMLMediaElement` has the reflected attributes only (ADR 0028 section
14). The loader (`swb-net`) keeps a whole response in one buffer of at
most 64 MiB (`MAX_BODY_SIZE`) and replay has no `Range`. The workspace
forbids `unsafe` code (`unsafe_code = "forbid"`); the code conventions
require an ADR before any `unsafe` code.

**The spike.** This ADR was written with a build spike that is not
committed (in the worktree of the step, `tmp/spike/`; the notes of the
step have the commands and the raw output). On the development machine
(20 cores, Debian 13, gcc 14.2, nasm 2.16.03):

- FFmpeg 9.0.2 (the newest stable release at least two weeks old: 9.1
  came out on 2026-11-20), built with the configure line of section 1:
  `configure` reports "License: LGPL version 2.1 or later". Build time
  24 s with `make -j 8` (2 min 38 s of CPU). Static libraries:
  `libavcodec.a` 6.19 MB, `libswscale.a` 2.76 MB, `libavutil.a` 1.44
  MB, `libavformat.a` 0.99 MB, `libswresample.a` 0.20 MB (11.5 MB; no
  debug symbols). A linked test binary, stripped, is 8.6 MB; FFmpeg's
  part is about 8.2 MB (text and data; `bss` 2.6 MB). Of `libavcodec`,
  by object files: VP9 1.76 MB, H.264 1.31 MB, shared code 0.64 MB, AAC
  0.31 MB, VP8 0.16 MB, Vorbis 0.07 MB, MP3 0.06 MB, Opus 0.05 MB, FLAC
  0.04 MB.
- A Rust test program opened the 35 media files of `fixtures/media/` through
  read and seek callbacks on a Rust byte source (FFmpeg never got a file
  name), decoded the first video frame, seeked to the middle and decoded
  the frame there, decoded 0.5 s of audio and resampled it to 48 kHz
  stereo `f32`, and converted frames to RGBA. All valid files work; the
  hostile ones fail where Chromium fails and play where Chromium plays
  (section 11). The same program with
  AddressSanitizer and UndefinedBehaviorSanitizer on the C boundary file
  found nothing (also no leaks).
- FFmpeg's values agree with Chromium's where the diagnosis has them:
  the start time of `mp4-h264-aac-start1500.mp4` is 1.476009 s and of
  `webm-vp8-vorbis-start1500.webm` 1.497 s (Chromium's `currentTime` at
  the start: 1.476009 and 1.497), the ADTS file's estimated duration is
  2.027269 s, VP8 + Vorbis 2.003, VP9 + Opus 2.008, Ogg Opus 2.0065. A
  seek to 60 % of the 4K file gives the frame at 21.5795 s (Chromium
  shows 21.58).
- Frames against Chromium's pixels (`tools/probes/media-frames.json`,
  seven cases, 13 frames, 756 points, a fresh Chromium run): FFmpeg's
  `swscale` with its BT.709 table differs by up to 12 levels (the
  diagnosis's 12 and 13). The cause: **Chromium's conversion caps the
  Cb-to-B coefficient at 2.0** (in 8-bit code values per Cb level;
  BT.709 limited range needs 2.112, BT.601 2.017, full range BT.709
  1.856). A fit on the blue and the green bar of the BT.709 file gives
  1.996 to 2.0; the R and G channels match the standard coefficients
  within 1 level. With the cap, a direct
  conversion (nearest chroma sample, the matrix of the stream) is within
  **1 level** of Chromium on all 756 points, and `swscale` with the cap in
  its coefficient table within **3 levels** (median 1 or 2), for BT.601,
  BT.709, full range and the untagged 1280 x 720 file. Chroma
  interpolation makes it worse at chroma edges (`SWS_FULL_CHR_H_INT`: up
  to 42 levels; `SWS_ACCURATE_RND`: up to 26; bilinear chroma in the
  direct conversion: up to 26), so at a scale of 1 Chromium's pixels fit
  one chroma sample for each 2 x 2 block of pixels.
- Decode cost (one decode thread plus FFmpeg's threads; CPU time per
  second of media, so 100 % is one core at real time):

  | File | 1 thread | 4 threads | 20 threads (auto) | throughput, auto |
  |---|---|---|---|---|
  | 1080p 30 fps H.264, 20 s | 11 % | 12 % | 16 % | 1644 fps |
  | Wikipedia VP8 2558 x 1626, 25 fps | 5 % | 6 % | 12 % | 1903 fps |
  | Mux 4K H.264, 24 fps | 52 % | 56 % | 72 % | 368 fps (1 thread: 46 fps) |

  The conversion to RGBA with `swscale` costs 0.6 ms per 1080p frame at
  the native size, 1.3 ms at 2558 x 1626 and 3.4 ms at 4K (1.4 ms for
  1080p to 1280 x 720); a scalar Rust conversion costs 3.5, 9.1 and
  14.2 ms. Opening the 1080p file (to the metadata) takes 11 to 15 ms,
  its first frame 31 to 45 ms; a seek to an exact frame (key frame, then
  decode forward) 17 to 28 ms with threads (31 to 77 ms with one); the
  4K file 160 to 310 ms with 20 threads, 340 to 780 ms with 4, 0.9 to 2.2
  s with one. So the CPU budget of the diagnosis (150 % of one core for
  1080p and the Wikipedia file) leaves more than 130 % for paint and
  composition, and the seek budget (250 ms for 1080p) holds.
- PipeWire 1.4.2: a `pw_stream` (F32, stereo, 48 kHz, `node.latency`
  1024/48000) opened from a C boundary file in 11.5 ms and consumed
  48,125 frames per second of zeros (a quantum of 512 frames, a delay of
  512 frames, 10.7 ms). Only silence was played. Without a server
  (`XDG_RUNTIME_DIR` unset) `pw_stream_connect` fails at once
  (`EHOSTDOWN`, 5.4 ms).

## Decision

### 1. The FFmpeg build

**Version.** FFmpeg 9.0.2, `ffmpeg-9.0.2.tar.xz` from
`https://ffmpeg.org/releases/`, SHA-256
`8c3850283eb25fa026482078a04051e0be17347b09ef81a0849bec15a96e002e`
(12,040,788 bytes), signed by the FFmpeg release signing key
`FCF9 86EA 15E6 E293 A564 4F10 B432 2F04 D676 58D8` (the spike checked
the signature; `ffmpeg.org` publishes no checksum file, so swb pins the
SHA-256 of the verified tarball).

**One place for the pins.** `tools/ffmpeg/` holds the version, the
SHA-256, the key fingerprint and the build script; the `just` recipe,
the build script of the Rust crate and CI read them from there.

**Configure line** (from an empty build directory):

```
configure --prefix=PREFIX \
  --enable-static --disable-shared --enable-pic \
  --disable-autodetect --disable-iconv \
  --disable-programs --disable-doc --disable-network \
  --disable-avdevice --disable-avfilter \
  --disable-debug \
  --disable-everything \
  --enable-demuxer=mov,matroska,ogg,mp3,flac,wav,aac \
  --enable-decoder=h264,vp8,vp9,aac,mp3float,opus,vorbis,flac \
  --enable-decoder=pcm_u8,pcm_s16le,pcm_s24le,pcm_s32le,pcm_f32le \
  --enable-parser=h264,vp8,vp9,aac,mpegaudio,opus,vorbis,flac
```

- `--disable-everything` leaves no protocols, muxers, encoders, filters,
  devices or hardware acceleration. `configure` adds the one bitstream
  filter that the VP9 decoder needs (`vp9_superframe_split`).
- `--disable-autodetect` turns off every system library (VA-API, VDPAU,
  Vulkan, X11, SDL, zlib, bzlib, lzma); `--disable-iconv` turns off
  iconv, which `--disable-autodetect` leaves on. No demuxer of the list
  needs zlib: without it the MOV demuxer cannot read a compressed index
  (`cmov`, from old QuickTime files) and the Matroska demuxer cannot read
  zlib- or bzip2-compressed tracks (rare; header stripping works). Both
  then fail to open, which is `MEDIA_ERR_SRC_NOT_SUPPORTED`.
- No `--enable-gpl`, no `--enable-nonfree`, no `--enable-version3`.
- The x86 assembly stays on (it needs `nasm`), and so do FFmpeg's
  threads (pthreads).
- The PCM decoders are the formats of WAV files that Chromium plays
  (`audio/wav; codecs=1`) and float WAV.
- New demuxers or decoders (AV1 through dav1d, later) need an update of
  this ADR.

**The recipe.** `just ffmpeg` runs `tools/ffmpeg/build.sh`: download the
tarball into `out/ffmpeg/` if it is missing (once; `curl` from
`ffmpeg.org`), check the SHA-256 (always) and the signature (when `gpg`
is installed, with a temporary key ring that holds only the pinned key),
extract, configure in `out/ffmpeg/build-9.0.2/`, `make -j` (the number
of cores, at most 8), install into `out/ffmpeg/9.0.2/` and write
`out/ffmpeg/9.0.2/swb-build.txt`: the version, the SHA-256 and the
configure line. `out/` is git-ignored. Build needs: a C compiler, `make`,
`nasm`; for PipeWire, `pkg-config` and `libpipewire-0.3-dev`.

**`cargo build --features media` without the build.** The build script
of `swb-media-ffi` (section 4) looks for `SWB_FFMPEG_DIR`, else for
`out/ffmpeg/9.0.2/` of the workspace. When it is missing, or when its
`swb-build.txt` does not name the pinned version, checksum and configure
line, the build fails with one message: "FFmpeg 9.0.2 is not built (or
was built with other options): run `just ffmpeg` (ADR 0047), or set
SWB_FFMPEG_DIR". It never downloads or builds FFmpeg itself (a build
script must not use the network). It links `avformat`, `avcodec`,
`swscale`, `swresample` and `avutil` statically and `m`, `pthread`
dynamically.

**CI.** A second job, `check-media`: installs `nasm`, `pkg-config` and
`libpipewire-0.3-dev`, restores `out/ffmpeg/9.0.2/` from the cache with
the key `ffmpeg-<runner OS and image>-<hash of the pins>-<hash of
configure.txt, build.sh and the key>` (the version, size, checksum, URL,
key fingerprint and the configure line, so any change makes a new key; not
the date of the signature check), runs `just ffmpeg` (a no-op on a cache
hit), saves the cache right after the build (also when a later step fails)
and runs `just check-media`. The first job (`check`) stays as it is: the default
build without media.

**Updates.** FFmpeg parses hostile input in the browser process without
a sandbox, so security fixes matter. FFmpeg publishes them in point
releases of each branch (ffmpeg.org "Security" lists the CVEs fixed per
release). Rules:

- At the start of each milestone, and when a CVE in an enabled demuxer,
  parser or decoder is published, check for a newer point release of the
  9.0 branch and take it.
- An update changes the files of `tools/ffmpeg/` only (version,
  checksum, the date of the signature check), then: `just ffmpeg`,
  `just check-media`, the media probe tests, the hostile-page set; one
  commit.
- A new branch (9.1, 10.0) is a decision of its own: the C boundary is
  compiled with warnings as errors, so removed or deprecated API shows at
  once; the probe tests check that frames and times did not change.
- A release is taken at least two weeks after its publication, unless it
  fixes a security problem in an enabled component.

### 2. The license

FFmpeg built as in section 1 is LGPL-2.1 or later (`configure`: "License:
LGPL version 2.1 or later"; FFmpeg's `LICENSE.md`: the GPL parts are
optional and need `--enable-gpl`). Static linking makes the `swb` binary
of a media build a "work that uses the Library" in the sense of LGPL-2.1
section 6.

**The source repository** contains no FFmpeg code: the recipe downloads
it. swb's own files stay MIT, including the C boundary files (they
include FFmpeg's headers; the binary that results is covered below). The
repository records the exception: `THIRD_PARTY_NOTICES.md` gets a section
"FFmpeg (LGPL-2.1-or-later), in builds with the `media` feature" (what,
which version, the configure line, where the source comes from, what a
distributor must do), and `licenses/LGPL-2.1.txt` holds the license text
(FFmpeg's `COPYING.LGPLv2.1`). PipeWire gets a section too: MIT, linked
dynamically; its SPA headers (MIT) are compiled into the boundary file.

**Whoever distributes a `swb` binary built with `media`** must (LGPL-2.1
sections 6 and 6a; FFmpeg's "License and Legal Considerations" checklist):

1. give a prominent notice that the binary contains FFmpeg under the
   LGPL-2.1 or later, with the license text (both are in
   `THIRD_PARTY_NOTICES.md` and `licenses/`);
2. give FFmpeg's complete source as used: the exact tarball (SHA-256
   above), the configure line, and any patches (swb applies none), next
   to the binary (the same download page), not only a link to
   `ffmpeg.org`;
3. allow relinking with a modified FFmpeg: swb's complete source (MIT)
   at the same commit, with the build instructions (`just ffmpeg`,
   `SWB_FFMPEG_DIR`, `cargo build --release --features media`), satisfies
   this, because anyone can rebuild with another FFmpeg;
4. not restrict reverse engineering for debugging such modifications
   (MIT does not).

swb publishes no binaries today. A future packaging recipe must put the
three items (notices, FFmpeg's tarball and configure line, swb's source
at the commit) into every archive of a media build. CI does not upload
binaries of the media build.

**The binary tells it.** `swb --version` prints, in a media build, one
line per component: "FFmpeg 9.0.2 (LGPL-2.1-or-later; configure:
...)" from `av_version_info()` and `avcodec_configuration()`, and
"PipeWire 1.4.2 (MIT)" from `pw_get_library_version()`. `swb --licenses`
prints `THIRD_PARTY_NOTICES.md` and the license text (embedded with
`include_str!`), so a binary carries its notices.

**Checks.** A test of `swb-media-ffi` asserts that `avcodec_license()`,
`avformat_license()`, `avutil_license()`, `swscale_license()` and
`swresample_license()` are "LGPL version 2.1 or later" and that
`avcodec_configuration()` contains neither `--enable-gpl` nor
`--enable-nonfree` and equals the pinned configure line. `cargo deny`
checks only Cargo's graph (`deny.toml` has `all-features = true`): the
new crates are swb's own (MIT) and their build dependencies `cc` and
`pkg-config` are MIT OR Apache-2.0, so `cargo deny` stays green and the
default build stays permissive. FFmpeg and PipeWire are not crates; the
build script and the test above are their guard.

**Exception to ADR 0003.** The owner allowed FFmpeg (LGPL-2.1-or-later,
static) and PipeWire (MIT, dynamic) for builds with the `media` feature
(ground rules, 2026-11-20). Like ADR 0014, this is not a precedent for
other copyleft code. ADR 0003 has an update section.

**Patents** are not a copyright matter and this ADR does not decide them:
the owner put the non-free formats (H.264, AAC) in scope; FFmpeg's own
decoders are used, and a distributor checks the patent situation of its
country.

### 3. The Cargo feature `media`

- `swb` has the feature `media` = `swb-engine/media`; `swb-engine` has
  `media` = `swb-media/ffmpeg` + `swb-media/pipewire`; `swb-media` has
  `ffmpeg` = `swb-media-ffi/ffmpeg` and `pipewire` =
  `swb-media-ffi/pipewire`. `swb-automation` passes it through for its
  tests. Default: off. `cargo build` builds without FFmpeg and PipeWire
  and needs neither.
- `swb-media-ffi` without features compiles to an empty crate, and its
  build script does nothing. So `cargo build --workspace` and
  `cargo clippy --workspace --all-targets` (which build every member)
  work without FFmpeg.
- `just` recipes: `check`, `lint`, `test`, `build`, `run` stay without
  media. New: `ffmpeg` (section 1), `build-media`, `run-media`,
  `test-media` (the workspace tests with `--features swb/media`),
  `lint-media`, `check-media` (`ffmpeg` + `lint-media` + `test-media` +
  `deny`), `media-asan` (section 4). The probe tools pass
  `--features media` when they build swb for the media probes.
- CI: the job `check` (default) and the job `check-media` (section 1).
  Locally, a commit that changes media code (the media crates, or code
  behind the feature in other crates) passes `just check-media` as well
  as `just check`. Feature 3 decides whether `test-media` runs the whole
  workspace or only the crates with the feature, because each feature
  set needs its own build of the dependent crates (disk and time).
- **Without the feature** swb behaves like a Chromium build without
  codecs, with JavaScript on and off: the element API exists in full
  (section 10); `canPlayType()` answers `""` for every type, so
  `<source type>` children are skipped with an `error` event on the
  source and no request; a resource without a matching type (`src`, or a
  `<source>` without `type`) is not fetched and fails as a failed request
  does: `loadstart`, then `error` with `MediaError` code 4
  (`MEDIA_ERR_SRC_NOT_SUPPORTED`) and the message `MEDIA_ELEMENT_ERROR:
  Format error`, `networkState` 3, `readyState` 0; `play()` rejects with
  `NotSupportedError: The element has no supported sources.`; a video
  shows its poster (or the default poster of ADR 0020). The controls draw
  as in ADR 0020; a click on play does nothing visible.

### 4. The boundary and `unsafe`

**A small C file per library, compiled by the `cc` crate, behind a
narrow C API.** Rust sees opaque handles (`SwbDemuxer`, `SwbPacket`,
`SwbDecoder`, `SwbFrame`, `SwbScaler`, `SwbResampler`, `SwbAudio`),
structs of plain integers (stream and frame information, times in
microseconds) and a few callbacks (read, seek, log, audio fill). No
FFmpeg or PipeWire struct crosses the boundary. About 30 functions for
FFmpeg (the spike's `swb_ffmpeg.h` is the draft) and 4 for PipeWire.

Reasons, from the spike: FFmpeg's structs (`AVFrame`, `AVCodecContext`,
`AVFormatContext`) change their layout between major versions and are
large; C code that uses them by name gets the layout from the headers of
the very build it links, and the compiler checks every use. A generated
binding (`bindgen`) needs libclang at build time, generates thousands of
items, and puts field access into `unsafe` Rust; hand-written
declarations of those structs would be undefined behavior after any
layout change. PipeWire's API is mostly inline functions and macros in
the SPA headers (`spa_format_audio_raw_build`, `SPA_POD_BUILDER_INIT`),
which no binding can call; C can.

**The one crate with `unsafe`: `swb-media-ffi`** (`crates/media-ffi`).
It holds `csrc/ffmpeg.c`, `csrc/ffmpeg.h`, `csrc/pipewire.c`, the
`extern "C"` declarations and the safe Rust types over them, and nothing
else. Everything else, the pipeline included, is in `swb-media`
(`crates/media`), safe Rust under the workspace lints.

- **Lints.** `swb-media-ffi` cannot use `[lints] workspace = true`
  (Cargo cannot override one lint of the workspace table), so its
  `Cargo.toml` has its own `[lints]` tables: a copy of the workspace
  tables with `unsafe_code = "allow"`, plus `unsafe_op_in_unsafe_fn =
  "deny"`, `clippy::undocumented_unsafe_blocks = "deny"`,
  `clippy::multiple_unsafe_ops_per_block = "deny"` and
  `clippy::missing_safety_doc = "deny"`. A unit test of the crate
  compares its tables with the workspace tables and fails on any other
  difference, so the copy follows the workspace.
- **Rules for the Rust side.** Every `unsafe` block has a `// SAFETY:`
  comment that names the invariant; one unsafe operation per block. Each
  C handle is owned by exactly one Rust value (`NonNull` inside) whose
  `Drop` frees it; no handle is `Clone`. `Send` is implemented (with a
  `SAFETY` comment) only for types that FFmpeg lets one thread use at a
  time; none is `Sync`. Methods that use a handle take `&mut self`. No
  raw pointer and no handle is public: the crate's API is safe types and
  slices that borrow from their owner (the planes of a frame live as long
  as the frame). Callback state (the byte source, the audio fill
  closure) is boxed by the owner and outlives the C object; callbacks
  catch panics (`catch_unwind`) and return an error or silence.
- **Rules for the C side.** C11; only the public API of FFmpeg and
  PipeWire; no global state except the log callback (set once) and
  PipeWire's `pw_init`; every function checks its arguments that come
  from Rust (stream indices, sizes, strides). The build script compiles
  it with `-Wall -Wextra -Werror` (and `-Wno-unused-parameter`;
  `-Wno-missing-field-initializers` for PipeWire's macros).
- **Sanitizers.** `just media-asan` builds the crate's decode tests with
  `-fsanitize=address,undefined` on the C files (FFmpeg itself is not
  instrumented) and runs them over `fixtures/media/` and the hostile
  media of section 11 (the spike ran this: no finding, no leak). It runs
  in `check-media` (it costs seconds). No fuzzing (CLAUDE.md).

### 5. Input: the media byte source

FFmpeg never opens a file or a connection: the demuxer gets read and seek
callbacks (`avio_alloc_context` with a 64 KiB buffer,
`AVFMT_FLAG_CUSTOM_IO`), and its `io_open` callback (used for a second
resource, such as MOV references) always refuses. Only the demuxers of
section 1 exist, so the format comes from the bytes (FFmpeg's probe), not
from `Content-Type`, as in Chromium (a WebM served as `video/mp4`
plays).

**The interface** (feature 4, in `swb-net`): `Loader::open_media(request)`
returns a `MediaResource`, a handle that the demux thread reads with
blocking calls: `read(buf)`, `seek(pos)`, `size()` (`None` when the
server cannot serve ranges), and `cancel()` from any thread. Behind it:

- **Requests.** The first request is `GET` with `Range: bytes=0-` (as
  Chromium; the header is CORS-safelisted for one range with a start)
  and `Accept-Encoding: identity`. A 206 with `Content-Range` gives the
  size and makes the resource seekable; a 200 makes it a stream of
  unknown seekability: `size()` is `None`, FFmpeg gets no seek callback,
  `seekable` is empty, and a seek ends at time 0 (Chromium, diagnosis
  section 5). Redirects are followed once; later range requests go to
  the final URL.
- **Chunks.** A fetch thread per resource reads the open response in
  chunks of 64 KiB into a block cache (64 KiB blocks at 64 KiB offsets).
  `read` blocks on a condition variable until the block at its position
  is there, or the end, an error or a cancellation. A read outside the
  cached blocks that is more than 1 MiB ahead of the running request, or
  behind it, cancels that request and opens `bytes=N-` at the block
  boundary. There is no 64 MiB limit on the size of a media resource.
- **Read-ahead per `preload`** (Chromium: about 15 s buffered at `auto`,
  3 to 6 s at `metadata`, nothing at `none`): `none` sends nothing until
  `play()` or a load is needed; `metadata` reads until `loadeddata` and
  then at most 3 s of media ahead of the current position; `auto` and
  playback keep 15 s ahead. Seconds become bytes with the average bit
  rate (the size over the duration, else 1 MiB per second). When the
  cache holds enough, the fetch thread cancels its request (as Chromium
  does) and opens a new one when the reader comes within half of the
  target.
- **Byte budget.** At most 64 MiB of cache per element; blocks behind
  the read position go first (the newest 8 MiB behind it stay, for short
  seeks back). All media resources of a page share one `ByteBudget`
  (`swb-net`, ADR 0028 section 16) of 512 MiB; a resource that cannot
  reserve a block waits as if the network were slow (`stalled`).
- **Threads.** Media fetches run on a pool of their own (at most 16
  threads; more resources wait), so a slow media server cannot hold the
  page's loads or the script's fetches.
- **Cancellation.** A new load, `load()`, a removed source, the end of
  the document or a suspended pipeline (section 6) calls `cancel()`: the
  running request is dropped, a blocked `read` returns an error, FFmpeg
  returns `AVERROR(EIO)`, and the pipeline stops without an error event
  (the element's algorithm already moved on).
- **`file:` URLs** read the file directly (seekable, no cache, no
  read-ahead: the OS cache does it), with the `file:` rules that images
  have. `data:` and `blob:` URLs are in memory.
- **Replay.** A media file is recorded once as a whole response (status
  200, the full body). The replay fetcher answers a `Range` request from
  it: 206, `Content-Range`, `Accept-Ranges: bytes` (416 outside), so a
  test does not depend on the sequence of requests. A media body is read
  from the fixture file at an offset, never loaded whole (the 4K file is
  161 MB). The tests' local server sends `Accept-Ranges: bytes` and 206.
- **CORS and credentials** (HTML "potentially CORS-enabled request" with
  the `crossorigin` attribute): without the attribute the request is
  `no-cors` with credentials `include`; with `anonymous` it is `cors`
  with `same-origin` credentials, with `use-credentials` `cors` with
  `include`; a failed CORS check is a network error (code 4,
  `MEDIA_ELEMENT_ERROR: Format error`). The destination is `video` or
  `audio`; cookies, the referrer policy and status 404 and 500 are as for
  images.

### 6. Threads and memory

**Per element with an open pipeline:**

- the fetch thread of section 5 (from the media pool; none for `file:`);
- a **demux thread**: reads packets (it blocks in the byte source),
  decodes the audio stream (audio decoding is cheap) and resamples it to
  48 kHz stereo `f32` (`swresample`), and sends video packets to the
  video thread;
- a **video thread**: decodes and converts frames to RGBA (section 9),
  with FFmpeg's own frame and slice threads: one thread below 1280 x 720,
  else `min(available cores, 8)`. FFmpeg's automatic count (all 20
  cores) costs 16 % against 11 % for 1080p and is not needed for real
  time.

Only the first video stream and the first audio stream are decoded (the
selected tracks of HTML); the demuxer discards the others
(`AVDISCARD_ALL`), so they cost nothing.

**Queues, bounded in bytes and in time:**

- video packets: at most 2 s of media and 16 MiB;
- decoded frames: at most 6 and at most 96 MiB (at least 2; a 4K frame
  is 33 MB in RGBA, so the 4K file keeps 2); the pixmaps are pooled and
  reused, so playback does not allocate per frame; the current frame, the
  frame being converted and the image that paint holds come on top of the
  queue (about 3 pixmaps; see the implementation notes of feature 5);
- samples: a ring of 1 s per element (384 KB).

**A bound on pipelines.** At most 16 open pipelines per process (an open
demuxer and decoders). An element that needs one while 16 are open waits
in a FIFO queue (`networkState` LOADING, `readyState` unchanged); a
pipeline that suspends gives its place to the next. Suspend: an element
that is paused (or ended) and has its current frame, or that has left
the document, closes its pipeline after 15 s (at once when it has left
the document and another element waits), keeping its metadata, its
current frame (as an image), its byte cache within the budget and its
position. It opens the pipeline again and seeks to `currentTime` when it
needs data (`play()`, a seek). Chromium suspends idle players too; the
15 s is swb's choice. So a page with 1,000 `<video preload=metadata>`
loads them all, 16 at a time, with bounded threads.

**Hidden elements.** A playing element that is not painted (`display:
none`, `visibility: hidden`, skipped contents, outside the viewport and
its margin, in a hidden window; the rules are in "Implementation notes
(M14 feature 7b)") keeps decoding (the decoder state and the clock need
it) but does not convert frames. An
element removed from the document pauses (measured: `timeupdate`,
`pause`) and suspends as above. Paused, ended and suspended elements use
no CPU: no thread runs and no timer is scheduled (the diagnosis budget:
0.0 % in the 3 s after a pause).

### 7. The media clock

Each element has a `MediaClock`: media time = anchor media time + (clock
now − anchor clock time) while playing (rate 1; `playbackRate` is in the
backlog). The clock source:

- **audio**, when the element has an audio track and is in the mixer of the
  PipeWire output: the frames of this element that the mixer consumed,
  divided by 48,000, minus the output delay that `pw_stream_get_time_n`
  reports (at least one cycle: a cycle's frames play after its callback),
  with an interpolation between the callbacks. The rules for the join, the
  leave, a seek, an underrun and the end of the audio are in "Implementation
  notes (M14 feature 8)";
- **monotonic** (`Instant`), in the GUI for an element without an audio
  track, and for all elements while no PipeWire output runs (a muted
  element with audio feeds the mixer at gain 0, so its clock stays the
  audio clock);
- **page time** (`PageClock`), in headless mode and tests, in real or
  virtual time.

**Virtual time.** The page clock jumps only when no work is pending
(ADR 0028 section 7). A playing pipeline that has not decoded what the
next due time needs (the frame and the samples up to it, or the end) is
pending work, like a request in flight; the jump waits for it. So the
media clock follows virtual time without sleeping, and results do not
depend on the speed of the machine. The silent output consumes samples
on page time.

**Events in time.** `timeupdate` every 250 ms of media time while
playing (Chromium), and at the steps that HTML names (seek, pause,
`ended`); `ended` when the media time reaches the duration and both
streams have ended; the media due times join `Page::frame_time()`
(section 9) and the event loop's next due time, so the GUI waits with
`WaitUntil` and headless loops jump to them.

**Synchronization.** Audio is the master; video follows. A frame is
shown from its presentation time (pts) until the next frame's; at a
paint, the frame with the greatest pts at or before the media time is
drawn. A frame whose successor is also due at the paint is dropped
(counted for `getVideoPlaybackQuality()`); the video thread decodes ahead
and is never waited for while audio plays. When the queue runs dry (no
frame or no samples for the current time), the element goes to
`HAVE_CURRENT_DATA`, fires `waiting`, and the clock stops (the anchor is
set); it starts again at `HAVE_FUTURE_DATA`. Works item 4 (at most one
frame period between the frame on screen and the audio clock) is
measured with a trace of (pts, audio time) at each present.

**Seeking.** Setting `currentTime` sets the official playback position
at once (reads give the new value; `seeking` true, a `seeking` event).
The pipeline flushes its queues, seeks the demuxer to the key frame at
or before the target (`AVSEEK_FLAG_BACKWARD`), flushes the decoders,
decodes forward and drops frames before the target except the last one
at or before it, which becomes the current frame; audio drops the
samples before the target (also inside a frame). `currentTime` stays the
target exactly (Chromium: 21.58 for 60 % of the 4K file). Several
assignments in a row run only the last seek (three `seeking`, one
`seeked`). A seek before the metadata is kept and runs after
`loadedmetadata`. Without seekable ranges (section 5) the position goes
to the start.

**The origin.** The timeline is the stream's: `currentTime` at the start
is the start time of the presentation and playback ends at the duration
(diagnosis section 1). FFmpeg's start time equals Chromium's for the MP4
and WebM files (1.476009, 1.497); for MP3 FFmpeg reports the encoder
delay (0.025057 s) where Chromium starts at 0, and for the MP4 file with
a start time FFmpeg's start plus duration is 3.5000 where Chromium's
`duration` is 3.496145. Feature 5 takes the rule per container from the
probe rows (`format-metadata-table`).

### 8. Audio output

- **One PipeWire connection per process**, opened when the first element
  starts to play with audio: a `pw_thread_loop` and one `pw_stream`
  (`PW_DIRECTION_OUTPUT`, `AUTOCONNECT | MAP_BUFFERS | RT_PROCESS`), F32
  interleaved, stereo, 48 kHz, `node.latency` 1024/48000, `media.role`
  `Movie`, application name `swb`. PipeWire converts to the device's
  format. Measured: 512 frames per cycle, 10.7 ms delay. The stream is
  inactive (`pw_stream_set_active(false)`: no callback runs) while no
  element is in the mixer, and it closes 30 s after the last element
  stopped. The connect does not wait for `STREAMING`: the elements play on
  the silent output until the stream streams and join the mixer then.
- **The mix** runs in the stream's process callback (PipeWire's real-time
  thread), in Rust, called through the C boundary: at most 32 sources
  (elements with audio that play), each a single-producer
  single-consumer ring of samples with an atomic gain; the callback sums
  `sample × gain`, clamps to [−1, 1], and counts the frames it took from
  each source (the audio clock of section 7). The callback does not
  block, allocate, free or log. Sources are added and removed through a
  fixed table of 32 slots (an atomic state and a mutex per slot, which the
  callback only `try_lock`s: the protocol and its proof are in the notes of
  feature 8). A 33rd playing element is not mixed: it plays on the silent
  output, and its clock is the page's (the monotonic clock in the GUI).
- **`volume` and `muted`**: the gain is `volume` (linear) or 0 when
  muted, applied with a 10 ms linear ramp to avoid clicks (swb's choice;
  not measured). `volumechange` fires on a change only (section 10).
- **The silent output** stands in for PipeWire: in headless mode and
  tests always (swb never connects to PipeWire there), and in the GUI
  when the connection fails or the stream does not reach `STREAMING`
  within 2 s (no server: `EHOSTDOWN` at once, measured; no output
  device: the stream does not reach `STREAMING`, not measured). It consumes each element's samples at the rate of its
  clock (page time, or monotonic), so `timeupdate` and `ended` come at
  the right times. One `warn!` per process. The GUI tries PipeWire again
  at the next `play()` after 30 s (measured: no server fails in 1 ms, a
  stream without a link in 2.0 s).
- **Errors while playing** (the server goes away, the stream goes to
  `ERROR`): the elements continue on the monotonic clock from their
  current media time (no jump), with the silent output.
- **PipeWire version**: the code uses `pw_stream_get_time_n`, which needs
  0.3.50 or later; the build script asks `pkg-config` for
  `libpipewire-0.3 >= 0.3.50`. Tested with 1.4.2 (Debian 13). CI's
  `ubuntu-latest` (24.04) has 1.0.5; Ubuntu 22.04 (0.3.48) is too old.

### 9. Video in paint

- **Conversion on the video thread**, with `swscale` through the C
  boundary, into RGBA at the frame's native size, written directly into
  a pooled `tiny_skia::Pixmap` (opaque pixels, so straight and
  premultiplied alpha are the same; no extra copy) **only when the width is
  a multiple of 64 pixels and the frame needs no padding; otherwise
  `swscale` writes into a buffer of the scaler (width rounded up to 64, one
  slack row) and the visible part is copied**: `swscale`'s SIMD paths write
  up to a vector past the last row of the destination (see the notes of
  feature 7a, fix round 2). The frame becomes the
  `DecodedImage` of the element; paint and raster draw it as any image.
  Measured: 0.6 ms per 1080p frame, 3.4 ms per 4K frame.
- **Colors as Chromium shows them** (section "The spike"): the matrix is
  the stream's tag as FFmpeg's codec parameters give it (from the
  container or the bitstream: `AVCOL_SPC_BT709` → BT.709; BT.470BG and
  SMPTE 170M → BT.601), else the frame's tag, else the untagged rule: an
  untagged stream is BT.601 below 720 lines and BT.709 from 720 (the
  height of the stream alone decides (the cropped height: 1280 x 718 is 718
  lines): measured in feature 7a, see "Implementation
  notes (M14 feature 7a)"). The stream's tag wins over the frame's: the
  Wikipedia VP8 file has BT.709 in its container while FFmpeg's VP8
  decoder tags every frame BT.601, and Chromium shows BT.709 (measured in
  feature 7a). The range is the stream's
  (`color_range`, else the frame's); for limited range, the
  Cb-to-B coefficient of the table is capped at 2.0 (`swscale`'s
  coefficient table, `sws_setColorspaceDetails`). Flags: `SWS_BILINEAR`
  without `SWS_ACCURATE_RND` and `SWS_FULL_CHR_H_INT`. **Tolerance for
  the tests: 3 levels for all matrices and ranges** (measured maximum 3,
  median 1 or 2), not 13. Other pixel formats (10-bit VP9, 4:4:4) go
  through the same call; no probe covers them yet.
- **Scale.** Paint draws the native-size frame with `object-fit`,
  `object-position`, clips and transforms like an image (bilinear, as
  Chromium's `object-fit: fill`). The decode thread does not scale.
  Feature 10 may add a downscale on the video thread (to the device size
  of the destination rectangle, when it is smaller and axis-aligned) if
  the budget needs it; `swscale` does 1080p to 1280 x 720 in 1.4 ms.
- **The display list** does not change when a frame changes: the video's
  `Image` item (`ImageRef::Node`) stays, the `Images` of the document
  return the current frame for the node, and the new frame bumps the
  node's content generation (`ImageSource::generation`, prepared in ADR
  0044 section 9). The damage is the item's bounds; the region raster
  draws it with its clips (ADR 0044 sections 9, 18, 19). The natural size
  changes only at `resize` (a new layout then).
- **The poster** (diagnosis section 8): each element has a "show poster"
  flag. It is set at each load; it is cleared by `play()` and by a seek
  after the metadata (also a seek to 0); while it is set and the element
  has a `poster` attribute, the poster shows (nothing while it loads or
  after it failed), else the current frame; without a poster
  attribute the first frame shows as soon as it is decoded; `load()`
  sets it again; `ended` keeps the last frame. With `preload=none` and no
  data, the poster or nothing (no #333 with JavaScript on and without
  controls, as now). The default poster (#333, a video with controls and
  without a `poster` attribute) shows only while the video has no frame:
  with a frame the letterbox is transparent (measured in feature 7a).
- **The frame loop** (corrected in feature 7b: the frame times are not in
  `Page::frame_time()`; see "Implementation notes (M14 feature 7b)").
  The presentation time of the next frame of a playing video, mapped to
  page time through the media clock, is one of the media due times that
  `Page::next_task_time()` takes (`media_next_due`). The GUI wakes at that
  time (`ControlFlow::WaitUntil`), the pump polls the pipelines, the frame
  pacing of ADR 0040 applies (Wayland frame callbacks), and
  `frame_outdated()` is true when a painted video has a converted frame
  due that it does not show (`Pipeline::frame_due`: it asks, and takes
  nothing) or when the new frame bumped the generation of its image. A
  paint that comes after the next frame's time shows the newest due frame
  (dropped frames are counted, never queued). A paused, ended or stalled
  video schedules nothing.
- **Headless screenshots** draw the frame at the media time of the page
  clock; in virtual time the pipeline has settled before the screenshot
  (section 7), so screenshots are deterministic.
- **The natural size** is the stream's display size (`videoWidth`,
  `videoHeight`; the sample aspect ratio applied, measured in feature 7a:
  160 x 120 with 2:1 is 320 x 120 and with 1:2 is 160 x 240; 321 x 241
  stays exact), except while the element represents its poster and the
  poster is loaded: then it is the poster's (measured: a 200 x 100 poster
  on a 320 x 240 video gives a box of 200 x 100 until `play()`, 320 x 240
  after it, and 200 x 100 again after `load()`).

### 10. The element

- **State lives in the engine**: `engine/src/media/`, one `MediaElement`
  record per `<video>` and `<audio>` node in `DocumentState` (and one per
  `new Audio()` element, which has no document position): the HTML
  algorithms (resource selection with `<source>` `type` and `media`, the
  load algorithm, `networkState` and `readyState`, `paused`, the seek
  algorithm, `ended`, `played` ranges, `volume`, `muted`, `error`, the
  pending `play()` promises by id, the autoplay flag, the show-poster
  flag) and the handle of its pipeline (`swb-media`). These algorithms
  run with JavaScript off too (autoplay, the controls of feature 9).
- **The pipeline reports** through a channel that the engine drains in
  `Page::pump`, as network completions (its notify wakes the GUI loop):
  metadata, the first frame, buffered ranges, underrun, the end, errors.
  The engine turns them into state changes and queues **media element
  tasks** (HTML's media element event task source) that fire the events
  in Chromium's order (diagnosis section 3), each a task with its
  microtask checkpoint, delivered as the image events are
  (`ScriptRuntime::media_events`, a new entry like `element_events`).
- **The bindings** (`web`): the IDL of `HTMLMediaElement`,
  `HTMLVideoElement` and `HTMLAudioElement` (the members of the diagnosis
  section 2 that M14 needs; not `captureStream`, `setSinkId`, the
  `remote` and key members), `MediaError`, `TimeRanges`, the legacy
  factory `Audio`; they call new `EngineHost` methods (`media_state`,
  `media_command`, with no-op defaults). The `play()` promises live in
  the web crate's state, keyed by node and id; the engine settles them
  through the same delivery as the events.
- **User activation for autoplay** (diagnosis section 4): the frame's
  sticky and transient activation (existing: `note_user_activation`).
  Rules: muted `<video>` plays without activation; unmuted needs sticky
  activation; `<audio>` and `new Audio()` need it also when muted. Every
  `--eval` expression, every `eval` step and every automation
  `page.evaluate` gives the document sticky and transient activation
  before it runs, as Playwright's `evaluate` does, so tests that compare
  with the probe rows see the same rule. Risk for feature 6b: existing
  tests read the activation state (the popover and dialog cases of
  `popover_input.rs` and `probe_net/top_layer.rs`, the `xo-act-*` rows
  that test which inputs give activation, `message_channel.rs`). If the
  new activation changes their results, it becomes an option of the
  automation call (on for the media tests) instead of a rule for every
  evaluation; feature 6b measures this first. A `play()` that the policy
  blocks rejects with `NotAllowedError: play() failed because the user
  didn't interact with the document first. https://goo.gl/xX8pDD`,
  also in the "attached, unmuted, created by script" case where Chromium
  leaves the promise pending (a quirk that swb does not copy).
- **`error.message`**: Chromium's strings where the condition is the same
  (swb uses FFmpeg too): `MEDIA_ELEMENT_ERROR: Format error` (a failed
  request, and every load without the feature),
  `MEDIA_ELEMENT_ERROR: Empty src attribute`,
  `PipelineStatus::DEMUXER_ERROR_COULD_NOT_OPEN: FFmpegDemuxer: open
  context failed` (no demuxer opens the bytes),
  `PipelineStatus::PIPELINE_ERROR_DECODE: video decode error!` (and
  `audio decode error!` for audio). Other failures get a short text of
  swb's own with the same prefix style. Exception messages (`TypeError`,
  `IndexSizeError`, `AbortError`, `NotSupportedError`) are Chromium's,
  measured.
- **Automation** (diagnosis section 11, point 6): activation for
  `--eval`, `eval` and `page.evaluate` (above); the `wait` step and
  `page.waitForIdle` run the media clock in virtual time (it is part of
  the page's due times); an `eval` that returns a promise waits for it
  in virtual time, which gives "wait for `seeked`" and sampling at media
  times without new step kinds; the `screenshot` step shows frames.
- **ADR 0028 section 14** ("Media playback is out of scope", `play()`
  rejects with `NotSupportedError`) is replaced by this section for
  builds with the feature; without it, section 3 applies.

### 11. Limits for hostile media

- **Demuxer**: `probesize` 5 MB and `max_analyze_duration` 5 s (FFmpeg's
  defaults, set explicitly), at most 16 streams (`max_streams`; FFmpeg's
  default is 1,000); the rest are discarded.
- **Frames**: at most 16,384 px in each dimension and at most 8192 x
  4352 pixels in all (`max_pixels` of the decoder); a larger stream fails
  at the metadata with code 4. Swb's choice; feature 10 measures
  Chromium with a hostile file.
- **Audio**: at most 8 channels (mixed down to stereo by
  `swresample`), 8 kHz to 384 kHz.
- **Allocation**: `av_max_alloc` 512 MiB per allocation (FFmpeg's default
  is `INT_MAX`); a larger one fails, and the element gets an error.
- **Memory** in total: the queues and budgets of sections 5 and 6, the
  pipeline bound (16), so that the 4K file stays below 1.5 GiB and many
  elements stay bounded.
- **Duration**: no limit; a stream without a known duration has
  `duration` `Infinity` and plays until its end (the byte budget bounds
  its memory).
- **Errors**: an error of a decoder (`avcodec_send_packet`,
  `avcodec_receive_frame`) is fatal for the element: `MEDIA_ERR_DECODE`
  (code 3), `error`, `pause` (the spike: the corrupt and the truncated
  MP4 fail in the H.264 decoder, which is Chromium's code 3). An error of
  the demuxer after the open ends the stream (`ended`; the spike: the
  truncated and the corrupt WebM end early, which Chromium plays to
  `ended`). An open that fails is code 4 (`garbage.mp4`, the MP4 without
  `moov`, an image).
- **Time**: FFmpeg's calls cannot be interrupted, but they run on the
  element's threads, never on the page thread. A pipeline that has made
  no progress for 10 s while the element needs data gets code 3, and its
  threads are detached (at most 16 detached threads per process; beyond
  that, new media loads fail with code 3). A blocked read returns at
  cancellation.
- The hostile-page set gets the hostile files of `fixtures/media/` plus
  new ones made by the generator (feature 10): a frame size above the
  limit, an endless stream (a server that never ends the body), 1,000
  media elements, a file with 100 streams.

### 12. Updates of ADR 0003 and ADR 0020

Both have an update section with this ADR's date: ADR 0003 for the
license exception (section 2), ADR 0020 for playback (sections 3 and
9).

## Alternatives

- **Dynamic linking of a system FFmpeg** (`libavcodec-dev`): no static
  LGPL obligations, but the decoders, their versions and their security
  fixes would be whatever the system has (also GPL builds), and the
  owner decided for a pinned static build.
- **Crates that bind FFmpeg** (`ffmpeg-next`, `ffmpeg-sys-next`):
  `bindgen` over all of FFmpeg's API, struct access in `unsafe` Rust,
  versions that lag FFmpeg's releases, and a build that finds FFmpeg its
  own way. The narrow C API is smaller and checked by the C compiler.
- **Rust decoders** (`symphonia` for audio, `openh264` for H.264): no
  VP8/VP9 decoder in Rust, Cisco's OpenH264 binary license, and two
  stacks instead of one; the owner chose FFmpeg.
- **Conversion with swb's own Rust code**: matches Chromium within 1
  level instead of 3, but the scalar version is 6 times slower than
  `swscale` (3.5 ms against 0.6 ms per 1080p frame). It stays an option
  if a test needs the last two levels.
- **Conversion in the raster** (a YUV image kind in `paint`): the raster
  would convert every visible frame at every paint, also for repaints of
  other damage, and `paint` would need the color rules; the decode thread
  converts once per frame and off the page thread.
- **The audio clock from wall time** only (no PipeWire timing): drifts
  against the device clock over a long playback (works item 4 measures
  sync over a long playback).
- **One PipeWire stream per element**: PipeWire mixes, but every stream
  is a node with its own real-time thread, and volume and clock would
  need one connection each; one stream with swb's mixer keeps one
  connection per process, as the ground rules ask.
- **Fetch the media resource without the feature** (as Chromium without
  codecs does, then fail in the demuxer): costs the bandwidth of every
  video on a page for an error; the request log of such a build is not
  compared with Chromium's.

## Consequences

- Media builds need `just ffmpeg` once (24 s here) and `nasm`,
  `pkg-config` and `libpipewire-0.3-dev`; default builds need nothing
  new. A media build is about 8.2 MB larger.
- `swb-media-ffi` is the only crate with `unsafe` code; it is small and
  has its own lint tables, tied to the workspace's by a test.
- swb's process parses hostile media with FFmpeg without a sandbox; the
  small set of enabled components, the refused `io_open`, the limits of
  section 11 and the update rule of section 1 reduce the risk; they do
  not remove it.
- Distributing a media build has LGPL obligations (section 2); swb
  publishes no binaries now.
- `swb-net` gets a second response model (a resource read in ranges)
  besides the whole-body `Response`.
- Frames match Chromium within 3 levels; the probe tests use that
  tolerance.
- Not decided here: Media Source Extensions (M15; its byte stream formats
  reuse the demuxer through the same byte source idea), AV1, native HLS,
  `playbackRate` other than 1 (time stretching), Web Audio,
  `captureStream()`, `requestVideoFrameCallback()`, hardware decoding.

## Implementation notes (M14 feature 3)

Decisions that the sections above left open, and findings of the
implementation:

- **The pins** (section 1) are four files in `tools/ffmpeg/`:
  `ffmpeg.env` (version, SHA-256, size, URL, key fingerprint, the date of
  the signature check; `KEY=VALUE` lines that the shell and the build
  script read), `configure.txt` (one option per line), `release-key.asc`
  (FFmpeg's public release signing key; `build.sh` imports only it into a
  temporary key ring in `out/ffmpeg/` and checks its fingerprint against
  the pin), and `build.sh`. `swb-build.txt` holds `version=`, `sha256=` and
  `configure=` lines; the build script of `swb-media-ffi` compares them
  with the pins. `build.sh` removes the source and build directories after
  the install (the logs stay) and keeps all temporary files in
  `out/ffmpeg/`.
- **Linking**: FFmpeg's pkg-config files ask for `-lm -latomic -pthread`;
  the build script links `m`, `atomic` and `pthread` dynamically.
- **The feature `pipewire`** of section 3 was not defined until feature 8
  added the PipeWire C file (it exists now: "Implementation notes (M14
  feature 8)"). `swb`'s feature `media` was `swb-media/ffmpeg` at feature
  3 (`swb` depends on `swb-media` for `--version` and `--licenses`);
  features 5 and 6 made it `swb-engine/media`, and feature 8 added
  `swb-media/pipewire` to it.
- **`just test-media` runs the whole workspace** with
  `--features swb/media`. Feature unification changes only the crates that
  depend on `swb-media` (today `swb-media-ffi`, `swb-media` and `swb`), so
  the other crates reuse the artifacts of `just test`. When the engine
  depends on `swb-media` (feature 5), the engine and the crates above it
  build twice; measure then.
- **`swb --version`** prints the configure options without `--prefix`
  (the path of the build) and without the quotes that `configure` adds;
  `-V` prints only swb's version.
- **Limits** (section 11): FFmpeg's `max_streams` makes the Matroska
  demuxer keep the first 16 streams of a file with more (the test file
  `mka-pcm-17-streams.mka`); the other demuxers may fail instead (not
  measured; feature 10's file with 100 streams). `max_pixels` makes
  `avcodec_open2` fail for a stream over the limit, or the decode of the
  first frame; swb checks the selected video track against both size
  limits at the open (`Unsupported`, code 4) and every decoded frame
  (decode error, code 3), and the audio track's channels and rate at the
  open.
- **Error kinds** of `swb-media`: `NotMedia` (no demuxer opens the bytes)
  and `Unsupported` (no decodable stream, a limit, a decoder that does not
  open) are code 4, `Decode` code 3, `Source` (the byte source failed,
  also during a call that FFmpeg then reports as invalid data, and a
  source that panics) code 2, `Seek` none. A read error that FFmpeg
  tolerates during the open is cleared when the open succeeds, so the first
  read does not report it again. None of the test files ends
  with a demuxer error after the open: the cut and the corrupt WebM end at
  the end of their bytes (FFmpeg logs the damage), so `error_at_end` is
  not exercised by them.
- **Seeking audio alone** starts the demuxer 200 ms before the target
  (MP3's bit reservoir and Opus's pre-skip need the packets before it;
  without the preroll the first MP3 frame after a seek is silent) and
  drops the samples before the target. With video, the demuxer seeks to
  the video's key frame, which is earlier. **The seek time is never below
  0** (the MP3 and ADTS demuxers refuse a negative time, so a loop to the
  start and a seek under 200 ms failed and left the demuxer at its end: a
  looping MP3 or ADTS file was silent after its first round; fixed in the
  fix round of feature 8). **A failed seek is not silent:** `seek_demux`
  logs it at `warn!` and tries once at the start of the stream (the
  decoder still drops what is before the target); if that fails as well, the
  pipeline logs `warn!` and decodes on from where the demuxer is (a source
  error still fails the pipeline).
- **Colors**: matrices other than BT.601 and BT.709 (BT.2020, SMPTE 240M)
  take the untagged rule; a pixel format with alpha is shown opaque (the C
  side sets alpha to 255). Both are in the backlog.
- **The natural size** applies the sample aspect ratio by widening (ratio
  above 1) or heightening (below 1); not measured in Chromium (the test
  files have square pixels).
- **Measured** (the test data of `crates/media/tests/`): all 40 test files
  open or fail as the spike found; the 25 rows of `format-metadata-table`
  match Chromium's size, duration and start except the departures of
  section 7 ("The origin"); 1,292 points of 43 frames are within 3 levels
  of Chromium (the worst point is 3); ASan and UBSan on the C boundary
  find nothing over the tests.

### Review fixes of feature 3

- **The refused `io_open`** (sections 5 and 11) has a self-check, not a
  test through FFmpeg: the MOV demuxer follows an external data reference
  only with its option `enable_drefs` (default off; swb passes no
  options), and no other enabled demuxer opens a second resource. So no
  test file can make FFmpeg call `io_open`. `swb_demuxer_try_io_open` calls
  the open context's `io_open` and `boundary.rs` expects swb's own error
  (`EPERM`). The build has no protocols either (`--disable-everything`):
  FFmpeg's default `io_open` would fail too, with another
  code, so the test checks the code and removing the refusal fails it
  (measured: with the refusal removed, a test that only expects an error
  still passed).
- **Seeks and the audio content**: the test compares the samples after a
  seek with the same span decoded in order. WebM times its blocks in whole
  milliseconds, so after a seek its first sample can be up to 0.5 ms
  (24 samples) from its time in the in-order decode (measured: `webm-vp8-
  vorbis.webm` is off by up to 24 samples); the other containers match
  within the resampler's phase (half a sample for 44.1 kHz).
- **The scaler** keeps the source and destination size and format and the
  matrix and range that it was set up with, and sets the coefficients again
  when any of them changes (it no longer compares the pointer that
  `sws_getCachedContext` returns).
- **The resampler** drops the samples that its old context holds when the
  input format changes inside a stream (a few milliseconds; broken or
  hostile files only). `Resampler::convert` refuses a frame that would
  need more than 4,194,304 samples per channel, and a frame's sample rate
  must be within the limits of section 11 (as the track's, which must not
  be 0).
- **`build.sh`** checks the size (`FFMPEG_SIZE`) before the SHA-256 and
  accepts a signature by a subkey of the pinned key (the last field of
  `VALIDSIG` is the primary key). The CI cache key does not include the
  date of the signature check, and the cache is saved right after the
  build (`actions/cache/save` with `if: always()`).

## Implementation notes (M14 feature 4)

Decisions that section 5 left open, and findings of the implementation
(`crates/net/src/media/`, `range.rs`, `test_server.rs`):

- **The interface.** `Loader::open_media(request)` and
  `open_media_with(request, MediaOptions)` return a `MediaResource` at
  once (an HTTP resource opens in the background; its calls block until the
  first response is there; a failure to open is the error of the first
  call, not of `open_media`). `MediaResource` has `read`, `seek`, `size`
  and `cancel`; `MediaResource::handle()` gives a cloneable `MediaHandle`
  for the owner: `cancel`, `resume` (undo a cancel: a suspended pipeline
  keeps its cache), `reader()` (a new reader over the same cache),
  `set_read_ahead(ReadAhead::Bytes | Seconds)`, `set_duration`, `stats()`
  (`MediaStats`: bytes cached now and at the peak, requests, `stalled`,
  `seekable`, `size`), `error()` and `wait_open(timeout)`. Dropping the
  last handle and reader cancels and frees the cache at once (the page
  budget gets its bytes back even when a job thread still waits for the
  network); a `cancel` alone keeps the cache for a `resume`, which also
  restarts the first request of a resource that was cancelled while it was
  still opening. `MediaResource::from_bytes` is the
  resource for `blob:` URLs (the engine holds the blobs). The element
  sets `Destination::Video` or `Audio` (new variants); `open_media`
  forces one of them.
- **The adapter** is `swb_media::NetSource` (module `net_source.rs`), behind
  the feature `net` of `swb-media` (`swb/media` turns it on): `swb-media`
  depends on `swb-net` only with it, and the engine adds nothing in 6a
  but the feature. Without the feature the sanitizer build (`just
  media-asan`, which builds `swb-media` without `net`) is unchanged: with
  `swb-net`'s dependencies (`ring`, through the `libc` crate) in the link,
  the linker put `libc` before the AddressSanitizer runtime in the
  `DT_NEEDED` list and the runtime crashed at start (`InitTlsSize`, a null
  call). So `tests/net.rs` runs under `just test-media`, not under the
  sanitizers.
- **Request path.** `Fetcher::fetch_stream(request)` (new, default: the
  buffered `fetch`) returns a `StreamResponse` (head and a `Read`); the
  network fetcher streams, the replay streams from the file. The
  redirect, CORS and tainting loop (`fetch_request_with`) is shared with
  `fetch_request`. `Accept-Encoding: identity` is the default of
  destinations `video` and `audio` in the HTTP layer, not a request
  header: a header of the request would be CORS-unsafe and cause a
  preflight (Chromium does not preflight it).
- **Statuses.** 206 needs a valid `Content-Range` that starts at the
  requested byte; a 206 with a total makes the resource seekable, with
  `*` it is a stream. 200 makes a stream (a 200 to a later request is an
  error unless it is for byte 0). Any other status, also 416 for the
  first request (an empty file) and 404, fails the resource with
  `NetError::Status`; a 416 later cannot happen (no request starts at
  or past the size). A changed total or start is `NetError::Range`.
- **Defaults.** Read-ahead `AUTO` (15 s) with at least 1 MiB and at most the
  cache limit minus the kept 8 MiB; seconds become bytes by size over
  duration, else 1 MiB per second. The job stops when the cache ahead of
  the reader reaches the target and starts again below half of it. A
  stream (no `Range`) cannot reopen, so its job waits with the
  connection open (TCP back pressure) and keeps a place of the pool.
- **Failures mid-body.** A body that ends or fails before the end of the
  file is requested again at the first block that was not complete; three
  failures in a row without a completed block fail the resource
  (`NetError::Range`). A failed resource still serves the blocks it has.
  A request for a later range (`bytes=N-`, N above 0) has 60 s for its
  whole body: it ends with an error and is reopened where it stopped (see
  **Time limits** below).
- **Eviction** (`cache.rs`): behind the read position oldest first except the
  newest 8 MiB; then blocks ahead past the first gap, farthest first; then
  the rest behind; never the blocks that run without a gap from the read
  position. A job without room waits (100 ms steps) and `stalled` is set.
- **The pool** (`pool.rs`): at most 16 jobs run; a cancelled or replaced job
  that waits for the network gives its place away (`release`) and its
  thread ends when the network returns (at most 16 such threads). A job
  marks its slot done under the pool lock before it changes the counters,
  and `release` ignores a slot that is done or released, so a `cancel`
  that races with the end of the job cannot count the place twice.
  There is no timeout shorter than the request's 60 s for a server that
  never answers the head: the cancelled thread stays until then.
- **Not interruptible**: the read of a blocked connection (ureq has no way to
  close it from another thread), so cancel frees the reader and the pool
  place at once but not the socket. The budget is freed at once only when
  the last owner is dropped (the cache closes: blocks and the block that
  a thread fills now are given back, later calls of the thread do
  nothing); after a `cancel` that keeps the cache the bytes stay charged.
- **Time limits** (`network/http.rs`, `fetch_stream`). ureq 3.4 has, on its
  stable API, a total time for the whole call (`timeout_global`), a total
  for the response head (`timeout_recv_response`) and a total for the body
  from the head on (`timeout_recv_body`); each is a budget that is not
  restarted by a read, and the socket's read timeout is the time that is
  left of the budget. A limit for one read (a stall limit) exists only in
  `ureq::unversioned`, which has no stability promise and is not used.
  The rule: the connect (15 s) and the head (60 s) keep their limits for
  all requests. The body of a request for the first byte (no `Range`, or
  `bytes=0-`: the server may answer 200 with a stream that cannot reopen)
  has no limit on its total time, so a long file or a paused video does
  not fail after 60 s. The body of a request for a later range has 60 s in
  total; the job reopens where it stopped. Open: a server that stalls
  without closing the connection holds the thread of a body without a
  limit until it closes (the reader returns on `cancel`, and the place in
  the pool is released, but the thread stays: at most 16 such threads
  count against the pool; others wait). A stall limit needs a transport
  that can time out one read (backlog).
- **Page budget floor** (`cache.rs`): a resource that holds less than 2 MiB
  may reserve past the page budget, so a resource with an empty cache
  still progresses when the other resources hold the whole 512 MiB (at
  most 16 resources run, so the excess is below 32 MiB). Above the floor
  the budget limits as before.
- **Hostile servers.** A 206 whose `Content-Range` starts at another byte
  than requested, a total that changes on a later response, and a 200 to
  a request for a later range each fail the resource with
  `NetError::Range`; the blocks already read stay readable (tests with
  `test_server::Lie`). The far-seek check exists in two places (`plan`
  and the job); each alone gives the right result.
- **Replay** (`fixture/replay.rs`): a body file over 8 MiB stays in its
  file and is read at an offset (`File::seek` and `take`); smaller bodies
  are in memory as before. A `GET` with a single `bytes` range to an
  entry of status 200 gets 206, `Content-Range`, `Accept-Ranges: bytes`;
  a range outside the body gets 416 with `Content-Range: bytes */LEN`; an
  invalid or multiple range is ignored (200, with `Accept-Ranges`). A 206
  or 416 drops the stored `Content-Length`, `Accept-Ranges` and
  `Content-Range` before its own `Content-Range` and `Accept-Ranges` are
  added.
  A request without `Range` is answered as before. `swbtools
  media-fixture FIXTURE URL FILE` records a media file as one whole
  response.
- **Measured** (local server and replay, 4K file `mux-highest.mp4`,
  161,597,070 bytes): the whole file reads through one request when the
  reader is fast, with a peak of 67,108,864 bytes (the 64 MiB limit) and
  the page budget at most that; the demuxer opens it and seeks to 60 %
  with 2 requests, 90.5 MB sent.

## Implementation notes (M14 feature 5)

Decisions that sections 6 and 7 left open, and findings of the
implementation (`crates/media/src/pipeline/`, `clock.rs`, `ring.rs`,
`origin.rs`):

- **The API.** `swb_media::pipeline::Pipeline::open(SourceOpener,
  PipelineOptions)`; the owner calls `update(now)` (returns `Report`s),
  `play(now)`, `pause(now)`, `seek(target)`, `suspend(now)`, `resume()`,
  `next_due()`, `has_pending_work()`, `current_frame()`, `position()`,
  `buffered()`, `quality()`, `set_painted()`, `set_gain()`, `samples()`.
  `play` on a pipeline that ended does nothing: the element seeks to the
  start first (HTML's `play()` at the end). Options besides the tracks, the
  bound and the notify: `output` (`AudioOutput::Silent`, or `External`
  when another consumer reads the ring: the ring has one consumer, so the
  pipeline then consumes nothing, and `update` and `has_pending_work` wake
  the demux thread when the ring has room again), `packet_bytes` (the
  video packet bound, 16 MiB), `stall_limit` (the no-progress limit,
  10 s; tests set small values of these two) and `painted` (the video is
  painted from the open, default true; a hidden element sets false, so no
  frame is converted before its first `set_painted`).
  Times are microseconds: media times on the element's timeline, clock
  times on the owner's clock. Reports: `Metadata`, `FirstFrame` (the first
  data at the position: the first frame, or the first samples for audio
  alone), `Buffered`, `Underrun`, `Resumed`, `SeekDone`, `TimeUpdate`,
  `Ended`, `Error`. The owner's notify is called from the threads when a
  report is ready or data that the owner waits for was decoded (pending
  work, a seek, an underrun, or a playing pipeline without a queued frame,
  whose next due time changes with the next frame).
- **`SourceOpener`**: a function that opens a new byte source at position
  0 (on the demux thread; at the first open, at each reopen, and for a
  seek in a stream) and a cancel function (called at close and suspend).
  `SourceOpener::net(MediaHandle)` resumes the resource and reads it with
  a new reader over the same cache.
- **Runs and epochs.** Each start of the threads is a run; closing,
  suspending and a fatal error end it, and its threads stop at their next
  check (a read that blocks in the source returns at the cancel). Each
  seek is an epoch; frames carry it, and the owner uses only data of the
  current epoch, so only the last of several seeks shows. Everything that
  the owner and the threads share is under one mutex with two condition
  variables; the sample ring is outside it.
- **Wake-ups** (second review of feature 5). The two queues notify by
  themselves, under the lock: each push to the video packet queue wakes
  the video thread, each removal from it the demux thread, and each
  removal from the frame queue the video thread (`PacketQueue`,
  `FrameQueue`; their items are private, so no code path can take a frame
  or a packet without the notify). Before, the owner notified only when
  it took frames at the media time; taking the first frame or a seek's
  frame from a full queue left the video thread waiting with room in the
  queue, and virtual time hung (a test of the late-audio file failed 3 of
  3 runs under load). The notifies go to all waiters, so a thread of an
  older run cannot take the only one. The other changes that a thread
  waits for notify explicitly: a new run (close, suspend, failure), a
  seek, room in the ring (the silent output in `update`; for an external
  consumer `update` and `has_pending_work`).
- **The clock source** is the owner's: it passes the clock time to each
  call (`ClockSource::Page`: page time in real or virtual time).
  Feature 8 adds `ClockSource::Audio` (the frames that the mixer took from
  the ring, `SampleRing::read_position`, minus the output delay).
- **The sample ring** (`SampleRing`): `AtomicU32` cells with `f32` bits,
  atomic frame positions, an atomic gain; one producer (the demux thread,
  which writes under the pipeline's lock, so a stale run never writes) and
  one consumer that takes no lock and allocates nothing. A seek discards
  what was written; the consumer checks the discard position again after
  it copied and replaces frames that were discarded meanwhile with
  silence. Each run makes its own ring (only with an audio track), so a
  suspended pipeline holds none; feature 8's mixer takes `samples()` again
  after a reopen. Samples are placed at their time: a gap up to 10 s
  becomes silence, an overlap is cut, a larger jump starts a new base.
  The producer publishes each base (a ring position, its media time and
  the seek epoch) in the ring before the first frame of that base, through
  a sequence counter (`SampleRing::base`), so the consumer maps the
  positions that it read to media times without the pipeline's lock
  (feature 8's audio clock). Orderings: a release fence after the store of
  a discard and an acquire fence before the consumer loads it again make
  a consumer that copied a frame written after the discard see the
  discard; x86 orders these stores anyway, so no test on the development
  machine can show a missing fence (ARM can).
- **The silent output** is in `update`: it consumes the ring up to the
  media time. A full ring wakes the demux thread only when the output
  consumes (the owner notifies it). With `AudioOutput::External`,
  `update` and `has_pending_work` wake the demux thread when it waits for
  room that is there again (also when the consumer read between the
  owner's `update` and its `has_pending_work`), so feature 8's mixer
  needs the owner to call one of them at least every 0.75 s while it
  reads (the ring holds 1 s), and must not notify from its real-time
  callback. `SampleRing::base` gives `None` also after 64 reads that
  found the producer writing a base; the consumer keeps its last base.
- **Queues** (section 6): video packets at most 16 MiB, 4,096 packets
  and 2 s (an `End` or a seek's flush always goes in; the span is the
  first and the last packet's time, kept as packets come and go, without a
  scan), but the 2 s bound holds only while the ring has 250 ms or more
  (a file whose video runs far ahead of its audio cannot block the
  audio); the queue of decoded frames 2 to 6 frames within 96 MiB (by the
  coded size); the ring 1 s. While the ring is full, the audio packets
  that the demuxer reads wait in a backlog (a quarter of the packet bytes,
  4 MiB, and 4,096 packets) and the demuxer reads on, so that the video
  packets behind them in the file reach the video thread.
  Pixmaps come from a pool (`Arc<Pixmap>`; reused when nobody else holds
  them): 7 for the 1080p file, 4 for the 4K file, at most 9 for the
  250-frame test file. A suspended pipeline drops the pool. **The frame
  memory of a pipeline** is not the 96 MiB of the queue: the current
  frame, the frame being converted and the image that paint holds add
  about 3 pixmaps, so a pipeline holds about the queue's frames plus 3.
  At the frame limit of section 11 (8192 x 4352, 142.6 MB in RGBA; the
  queue keeps its minimum of 2) that is about 5 pixmaps, 713 MB per
  pipeline and 11.4 GB for 16 pipelines (plus FFmpeg's own frames).
  A process-wide frame budget is in the backlog (feature 10).
- **Virtual time.** `next_due()` is the clock time of the next queued
  frame, the next `timeupdate` point or the end; `has_pending_work()` is
  true while the pipeline opens (until the metadata and the first data),
  while a seek has not its frame and samples, and while playing when the
  data for the next due time is not decoded (a queued frame at or after
  it, or the queue full, or the video ended; the samples up to it, or the
  ring full, or the audio ended; at the end, every track ended or decoded
  past the end, which is also the condition of `Ended`, so the end does
  not depend on when the decoder reports its drain). A pipeline that
  waits for a place is not
  pending work (a page with 1,000 elements must reach the 15 s suspend
  time). In virtual time no underrun occurs, and frames are dropped only
  when the pipeline is stalled on its owner (below).
- **Tracks far apart in the file** (review of feature 5). The demuxer
  reads in file order; when the data that the owner needs lies beyond the
  queue bounds (the audio after 16 MiB of video, or the last video
  packets behind more audio than the ring and the backlog hold), every
  thread waits for room that only the owner frees. Each thread records
  what it waits for in the shared state, under the lock, before it waits
  (the demux thread: room in the packet queue, room in the ring, nothing
  left to read; the video thread: packets, or room in the frame queue),
  and the state is checked again each time: when both wait and what they
  wait for still holds, the pipeline is stalled on its owner. Then it has
  no pending work, a seek or the first data ends with what is there, and
  the clock runs on, never an underrun: silence where the ring has no
  samples for the media time (an audio gap; also at the start, when the
  ring's first samples are later than the position, or when no audio has
  come yet), the last frame where no frame is decoded; frames that come
  late are dropped as usual. A thread that starts to wait while the owner
  waits for data and so stalls the pipeline wakes the owner. While the
  clock runs past the audio this way (an audio gap), only the video stops
  the clock, until the audio covers the media time again (so real time
  does not stop at every frame). The rule depends only on what the
  threads have done, and every change that a thread waits for wakes it
  (above), so the threads always reach the same stall and virtual time
  stays the same on every run (checked under load). With
  the default bounds the backlog makes the video flow while the audio is
  far ahead; the test file `mp4-h264-aac-late-audio.mp4` (video 0 to 2 s,
  audio 3 to 5 s, every video packet before the audio) shows both cases
  with a 4 KiB packet bound: the gap at the start, and two frames held in
  the decoder at the end of the video (dropped late). The reviewer's
  files (1280 x 720 40 Mbit/s 4 s with audio from 5 s, 19.8 MB; 640 x 360
  20 s with audio from 22 s) play to their end in virtual and in real
  time, with no frame that comes late (the second one with an underrun
  and a resume at 0 in real time, before the threads stall).
- **The no-progress limit** (section 11). A pipeline whose owner needs
  data (pending work, an underrun while playing) and whose threads made
  no progress (bytes read from the source, a packet, a frame or samples
  decoded) for 10 s fails with a decode error (code 3), and its run is
  detached: the run's place goes to the next pipeline at once, the
  source is cancelled, and its threads end at their next check when the
  blocked call returns; they count as detached until then. While 16
  detached threads run in the process, a new run fails at once with code
  3. One watchdog thread per process (`swb-media-watchdog`, started at
  the first use) holds the pipelines whose owners need data and sleeps on
  a condition variable until the next check (no wake-up while none
  needs data). The owner registers its pipeline when it starts to need
  data (`has_pending_work` and `update`, so also in real time); the
  failure calls the owner's notify, so it works also in virtual time,
  where nothing else would wake the owner. The limit counts from the
  moment the owner starts to need data (also after a long pause). A
  pipeline whose threads all wait for room that only the owner frees
  (stalled on the owner) is not stalled: the owner's flag may be older
  than its last look. When the threads all wait but what one waits for no
  longer holds, a wake-up was lost: the watchdog notifies both threads
  (they check their conditions again) and fails the run when the same
  holds a limit later without progress (second review of feature 5). The
  place is counted per run: the last thread of a run gives it back (the
  video thread too, so a panic of the demux thread no longer frees the
  place while the video thread runs).
- **Synchronization.** Audio is the master: with an audio track only the
  audio's data stops the clock (underrun); without one, the video's. The
  video thread drops a frame without converting it when its end is at or
  before the owner's last media time (counted with the dropped frames).
  `timeupdate` points: 250 ms of media time after `play`, after a seek
  ends, and after each point (a point that comes late does not repeat).
  After an underrun, playback resumes when 250 ms of audio past the media
  time are decoded (or the audio ended, or the ring is full), or for video
  alone when a frame is queued; the clock goes on from where it stopped.
- **The end** is the duration. When the streams ended more than 1 s
  before it (a cut file), or the duration is unknown, the end is the end
  of the data (swb's choice; not measured). Opus and VP9 + Opus: FFmpeg's
  duration (2.0065, 2.008 s) is a few milliseconds past the last sample;
  playback goes on to the duration, so `ended` comes at Chromium's
  `duration`.
- **Seeks.** A seek before the metadata is kept and runs after the open;
  the first frame after a seek is the seek's frame (the last at or before
  the target, else the first after it); a suspended pipeline that seeks
  reopens with that seek; a source that cannot seek opens again (when it
  has read packets) and plays from the start: `SeekDone` gives the start.
  The seek code is `MediaFile`'s (`seek_demux`, `SeekFilter`). A demuxer
  whose seek fails decodes on from where it is.
- **The origin** (section 7): `swb_media::timeline` per container. MP3:
  the stream's times are shifted by FFmpeg's start time (the encoder
  delay); the start is 0. WAV and ADTS: start 0. Matroska/WebM: FFmpeg's
  duration is the end. MP4 with a start time above 0: the start plus the
  longer of the video track and the AAC track in whole frames of 1,024
  samples (3.496145 s for the test file, Chromium's value). One probe row
  fits this rule; it is swb's reading of the row, not a measured
  mechanism. The 25 rows of `format-metadata-table` now match without
  departures. A negative start time (Ogg or Matroska with packets before
  0, an MP4 edit list that starts before the first sample) gives the
  start 0; the media times stay the stream's, so what lies before 0 does
  not play (swb's choice; no test file has one; not measured).
- **The bound** (section 6): `PipelineSlots` (16 per process,
  `PipelineSlots::global()`; tests make their own). A pipeline without a
  place has no thread; the place goes to the first waiter when a run's
  threads have ended (or at once when the run is detached). A run that
  ends while it waits leaves the queue, so a pipeline that suspends and
  resumes many times does not grow it. Measured: 1,000 audio pipelines
  open through 16 places with at most 16 threads.
- **Idle.** A paused, ended or suspended pipeline schedules nothing
  (`next_due` is `None`), and its threads block on condition variables
  once the queues are full: no wake-ups in 500 ms (test counters), 0 ms of
  CPU in 3 s paused for each real file (release build).
- **Measured** (release build, development machine, `tests/
  pipeline_bench.rs`, the thread counts of section 6): virtual time as
  fast as it goes, decode and convert: 1080p 30 fps 1,058 fps (1,341
  without conversion), the Wikimedia file 641 fps (2,180), the 4K file
  230 fps (285); CPU per second of media 15 %, 9 % and 68 % of one core.
  Real time on a monotonic clock, 5 s: no dropped frame and no underrun
  for the three files (the frame shown at most 0.3 ms after its time);
  CPU 14 %, 8 % and 63 % of one core (silent output, no paint).
- **Small races closed** (review of feature 5): the demux thread adds
  buffered ranges and pushes the end of the stream only for its own run;
  when the source opened after the run ended (the opener may undo a
  cancel), the thread cancels the source again and ends.

## Implementation notes (M14 feature 6a)

Decisions that sections 3, 5, 6, 7 and 10 left open, and findings of the
implementation (`crates/engine/src/media/`, `document/media.rs`,
`page/media.rs`, `crates/web/src/media_element.rs`,
`crates/media/src/can_play.rs`).

- **Where the state lives.** `engine/src/media/` has `Media` (a field of
  `DocumentState`: a `BTreeMap` of `Element` by node, the queue of events,
  the next error number), `select.rs` (resource selection), `load.rs` (the
  load algorithm and failures), `step.rs` (what the pump does with a
  fetch), `backend.rs` (`Fetch`, with and without the feature). An element
  has a record from its first load (a `src`, a `source` child, `load()`).
  `engine` depends on `swb-media` always (`can_play_type`, the types) and
  turns on `swb-media/ffmpeg` and `swb-media/net` with its feature `media`;
  `swb`'s `media` is `swb-engine/media`. The bound on open pipelines is
  the process's (`PipelineSlots::global()`, section 6, also the bound on
  detached threads), shared by all pages through each page's
  `MediaContext` (handed to its documents). Tests that run pages in
  parallel in one test binary give each page places of its own
  (`Page::isolate_media`, called by the pages of the engine's and the
  automation's test helpers), so that they do not wait for each other's
  places (review S4). The table has at most 16,384 elements; at the limit
  the records of elements outside the document go.
- **Without the feature** `Fetch::start` fails with code 4 and
  `MEDIA_ELEMENT_ERROR: Format error` one step later (so that `loadstart`
  comes first, with `networkState` 2 at its dispatch); no request. A
  `<source type>` is skipped by `can_play_type`, which answers `""`.
- **Stable state.** The load algorithm's synchronous part runs when the
  drain of the DOM changes processes the `src` (or at a read of a script:
  `EngineHost::media_state` drains first): `abort`, `emptied`,
  `timeupdate` (also when the position is 0, as Chromium), the reset,
  `networkState` 3. The part that HTML runs "in a stable state" (the mode,
  `NETWORK_LOADING`, `loadstart`, the fetch) is the next unit of the pump.
  `Page::pump_media` delivers the queued events *before* it steps, so
  `abort` is dispatched in the state of the synchronous part (rs 0, ns 3),
  as Chromium shows. A `src` that is set several times in one task loads
  once (the drain coalesces them; HTML would run two load algorithms).
- **Lockstep.** The state that scripts read changes only in `Media::step`,
  one unit for each element, and the page delivers the events of a unit
  before the next. So `readyState` and `networkState` at an event are the
  same on every run. In virtual time (`strict`) one unit goes at a time, by
  the first element in node order that can do one, and an element that
  waits for its threads holds the later ones back: the events of different
  elements come in the same order on every run (a test: 36 pages at once
  and a busy thread per core). In real time every element goes on as soon
  as it can.
- **Reports become events** (probe `media-states`): the metadata note waits
  until the byte source is idle or has read 1 MiB (a small file is read
  whole before the first event: `progress` and `suspend` come before
  `durationchange`, as Chromium); then `progress`, `suspend` (the network
  is idle), `durationchange`, `resize` (a video with a size),
  `loadedmetadata`; the first-data note waits for
  `Pipeline::has_enough_data()`, and then `loadeddata`, `canplay` and
  `canplaythrough` come together with `readyState` 4 (Chromium goes from
  `HAVE_METADATA` to `HAVE_ENOUGH_DATA`: `loadeddata` is dispatched with
  rs 4). `HAVE_CURRENT_DATA` and `HAVE_FUTURE_DATA` are never visible. A
  failure before the metadata is `error` (progress and suspend first when
  bytes arrived), at the element in attribute mode, at the `source` in
  children mode (then the next candidate). The two waits (the byte source
  settled for the metadata, `has_enough_data` for the first data) apply
  only in virtual time, where they make the state at the events the same
  on every run. In real time each note is handled when it comes, as
  Chromium fires `loadedmetadata` at the parse and `loadeddata` at the
  first frame, also on a slow link (review S3; tests: a slow link gives the
  events of a fast one in virtual time, and in real time `loadedmetadata`
  with ns 2 long before `suspend`).
- **`has_enough_data`** (`HAVE_ENOUGH_DATA`, `canplaythrough`): the
  proposal of feature 5 (queues full) is "the threads have settled": the
  first data was reported, no seek runs, no underrun, and every track is
  decoded to its end or each thread waits for room that only the owner
  frees, or for nothing more to read (`stalled_on_owner`). A small file is
  settled when it is read whole; a large one when the decoded queues are
  full, so the 4K file at `metadata` has rs 4 at `loadeddata` with the
  frames and samples that fit the queues (Chromium: 3 to 6 s buffered). It
  depends only on what the threads decoded, so the buffered ranges are
  final when it holds. While it is false the owner needs data: the
  no-progress limit (10 s) applies, so a source that stalls after the first
  frame fails the pipeline.
- **`Pipeline::has_unread_reports()`** (new): while a thread's report
  waits for `update`, `has_pending_work` is false. An owner that waits for
  the pipeline asks this too; without it, virtual time jumped (or the
  strict order moved on to another element) in the moment between the
  report and the update (the test of the events failed 1 in 20 without
  it).
- **The handover of a place** (review S1). A run that ends (the pipeline
  suspends, closes or fails) keeps its place until its threads have ended,
  and the last of them hands it to the first waiter. Before, a waiter had
  no pending work until it had the place, so virtual time could jump while
  the threads of a suspended run ended, and the waiter's events came at a
  page time that depended on the machine (the 1,000-element test failed
  about 2 in 47 runs under load). Now the place is *in transit* from the
  end of the run until the waiter has it (`PipelineSlots::in_transit`):
  `RunPlace::end` marks it in the same critical section that ends the run
  (`Pipeline::end_run`, `Shared::fail`, the watchdog's detach, and a run
  that ended before its thread started), and the transit ends after the
  next pipeline has its place, so there is no moment in which the run has
  ended and no one has pending work. A pipeline that waits while a place is
  in transit has pending work (`Pipeline::awaits_handover`, part of the
  element's `Fetch::pending`); every waiter's owner is notified when a
  transit ends. Nothing here depends on how fast a thread is: the owner
  never waits for a thread (`suspend` returns at once), and virtual time
  waits for the transit, which only the threads end. The other design, a
  `suspend` that hands the place on at once, lets the old threads run on
  beside the new ones without a bound on their number (or counts them as
  detached, so that a burst of suspends fails new pipelines). A thread that
  stays blocked (in FFmpeg) would hold the transit for ever: the watchdog
  checks each ended place one no-progress limit (10 s) after its end and
  detaches it if it is still in transit (its threads then count as
  detached, section 11). Test hook: `PipelineSlots::set_handover_delay`
  makes the last thread wait before it hands the place on
  (`Page::isolate_media(delay)`); both exist for tests only, and
  `set_handover_delay` does nothing on the process's slots
  (`PipelineSlots::global`). The engine test with 300 ms per handover
  fails without the rule (one run in one) and passes with it.
  Second fix round: `Fetch::pending` reads `awaits_handover` before
  `has_pending_work`. A waiting pipeline has no pending work, so if the
  handover ended between the two reads (the other order), both were false
  although the pipeline then had the place and work to do, and virtual time
  could move; in this order a handover that ends after the first read is
  seen by the second. No test can hit that window; the order has a
  comment. The end paths of a run each have a test that fails without the
  call of `end` there: `Pipeline::end_run` and `Shared::fail` (a waiter's
  `awaits_handover` stays true until it has the place, with a handover
  delay of 1 s), a run that ended before its thread started
  (`demux::spawn`, a unit test), and the watchdog (`check_progress`: the
  place goes on at once, so the test checks that every waiter's owner is
  woken, also the one that does not get the place). A notify of a
  transit's end sees `in_transit() == 0` (a unit test of the slots).
  `check_progress` marks the place in transit without a watchdog entry
  (`RunPlace::end_before_detach`), as it detaches it at once.
- **Late failures.** A pipeline failure after the metadata (the damage lies
  later in the file) is kept, and the load goes on as if the first data had
  come: Chromium loads such a file (rs 4) and fails when it plays there
  (`err-corrupt-mp4` and the others: code 3 during the play). Feature 6b
  surfaces it when the playback reaches the place of the failure (see
  "Review fixes of feature 6b"); until then the element
  does not report it. Without this rule the events would depend on when the
  threads met the damage.
- **`buffered`** is the whole timeline once the byte source holds all of the
  resource (a truncated or damaged file that arrived whole is `0-duration`,
  Chromium), else the ranges that the demuxer read, clamped to the
  duration. `seekable` is `0-duration` for a resource that can seek, `0-0`
  for a stream, empty before the metadata. `currentTime` is the pipeline's
  position (the start time after the metadata). `paused` is true, `seeking`
  false until feature 6b.
- **`preload`**: a missing attribute and `metadata` read ahead `METADATA`
  (3 s; Chromium's default is `metadata`: the IDL value, and the cases
  `real-preload-*` of `media-real-files.json`, three equal runs: after 1.5 s
  the 4K file has under 10 s buffered without the attribute and with
  `metadata`, over 10 s with `auto`; test data
  `tests/data/media-preload-default.json`; a 10 s WAV file of 1 MiB per
  second in the engine test tells the two apart: `metadata` buffers 5.06 s,
  `auto` all), `auto` and the empty
  string `AUTO`; `none` fetches nothing (`loadstart` with rs 0 and ns 1,
  then `suspend`) until `load()` (measured: the diagnosis says `load()`
  starts it) or a change of the attribute to another value.
  `load()` ignores `preload=none` for that load only. `crossorigin`,
  cookies and credentials follow HTML's "potentially CORS-enabled fetch"
  (`backend::request_for`); the request carries no referrer (as images:
  `Request` has none). A `blob:` URL fails (the engine holds no blobs;
  backlog).
- **The 15 s suspend** (`SUSPEND_AFTER_MS`): an element with rs 4 and a
  running pipeline is idle from the step where it became so; at 15 s of
  page time (`Page::next_task_time` includes the due time, also without
  JavaScript) its pipeline suspends. An element removed from the document
  with rs 4 suspends at once; one that still loads goes on (HTML pauses a
  removed element, feature 6b, and does not stop its load; before review S2
  the pipeline suspended with no data, and the element stayed at ns 2, rs 0
  for ever), and the 15 s rule suspends it later. The record, the metadata
  and `buffered` stay. Re-insertion does not resume a suspended pipeline
  (feature 6b).
  1,000 `<video preload=metadata>` load in virtual time, 16 pipelines at a
  time, in 0.5 s of real time (63 rounds of 15 s of virtual time).
- **The wake-up.** The pipeline's notify and the byte source's (a new
  `MediaHandle::set_notify`, called from the jobs when a block arrives, a
  job ends or the resource fails) are `Loader::waker()`: a completion that
  no request owns (`id` 0, client `WAKE_CLIENT`), which `recv_timeout`
  returns and `Page::accept_completion` drops, so the loops that wait for
  completions (the GUI's wake-up, `run_until`) wake for media.
  `MediaStats::idle` (no job runs or waits) is the network's `suspend`.
- **A server that answers 200 with `Accept-Ranges: bytes` and a
  `Content-Length`** (probe `fetch-ignore-range-accept-ranges-*`, three
  runs): Chromium treats the resource as seekable (`seekable` `0-10` for
  the 10 s file), sends `bytes=N-` at a seek, gets the whole body again
  (status 200) and plays from the right place (`currentTime` 6, `buffered`
  `0-10` after it). The byte source does the same: such a first response
  makes the resource seekable with the length as its size
  (`State::ignores_range`); a later request that gets a 200 reads the body
  from byte 0 and discards the blocks before the one that the reader needs
  (`Conn::sequential`); a far block ahead of a running body of this kind
  does not replace the job (without that rule each read reopened the
  body). A 200 whose length differs from the size fails the resource. A
  200 without `Accept-Ranges` stays a stream (a seek ends at 0, 14 rows).
  The bytes that a job reads and discards so are bounded per seek (review
  N3, changed in the second fix round): the count restarts when a seek opens
  a new request (the reopening after a failed read goes on counting, so a
  server that cuts its bodies short cannot be read again and again), and
  one seek may discard at most `MAX_DISCARDED_BYTES` (256 MiB), also for a
  resource without a known size. A seek that needs more fails the resource
  (`NetError::Range`). Chromium has no such bound and reads on; without it
  a server that claims a length that it does not have (1 GiB of endless
  bytes in the test) cost the bandwidth of a read to the target, 900 MiB
  for a seek to 900 MiB. The first version of the bound was cumulative per
  resource (four times the size): the 8th far seek in a 100 MB file failed.
  Now scrubbing in such a file keeps working (a test with 60 far seeks that
  discard 300 MiB together); the cost: on such a server a seek that needs
  more than 256 MiB of discarded bytes (a file of over 256 MiB, a seek
  past that point) fails, where Chromium reads on.
- **Rows for the timeline rules** (probe `format-timeline-table`, three
  equal runs, new files `mp4-h264-aac-fragmented.mp4`,
  `mp4-h264-aac-bframes-no-edit-list.mp4`, `mp4-opus.mp4`): a fragmented
  MP4 (duration 2.08, start 0), an MP4 with B-frames and without an edit
  list (video starts at 0.08, duration 2.08) and Opus in MP4 (2) agree with
  `swb_media::timeline` without a change. One departure: at `loadedmetadata`
  Chromium has `currentTime` 0 and the duration 2.023991 for the MP4 with a
  start time of 1.5 s, and 150 ms later 1.476009 and 3.496145 (the rule's
  values); swb has the later values at `loadedmetadata`. The MP4 rule rests
  on two rows now (one more would need a file with another start time).
- **`canPlayType`** (`swb_media::can_play_type`): Chromium's 149 rows with
  two departures (the 5 HLS types and the 3 `av01` types answer `""`),
  derived rules: the type is case-insensitive; `codecs` missing, empty or
  unterminated gives `maybe` (`probably` for a bare MP3, AAC or FLAC type);
  one unknown codec gives `""`; the codec strings are case-sensitive;
  `avc1.PPCCLL` is `probably` for the profiles 42, 4D and 64 and `maybe`
  for the others; `vp09.PP.LL.DD`. A `<source>` with `type=""` has no type.
- **Not matched** (departures, in the tests' lists): a second `suspend` after
  `canplaythrough` for the 78 KB WebM file at `preload` `metadata`
  (Chromium; not for the MP4 or MP3); Chromium's `buffered` end differs by
  0.01 to 0.02 s for some files (the tests compare within 0.02 s); MP3, WAV
  and Ogg served as a 200 without `Range` have no duration (`Infinity`;
  Chromium knows it from `Content-Length`: backlog); `sel-first-404-then-ok`
  had one Chromium run with a second request and a second `progress` (the
  data file has the other runs'); the `at loadedmetadata` values of the MP4
  with a start time.
- **videojs.org** (found in this feature). Its video gets the HLS playlist
  as `src` (no `MediaSource`); the load fails with code 4 (with the
  feature: the replay returns the playlist body and FFmpeg cannot open it,
  `PipelineStatus::DEMUXER_ERROR_COULD_NOT_OPEN: FFmpegDemuxer: open
  context failed`; without: no fetch, `MEDIA_ELEMENT_ERROR: Format
  error`), and Video.js opens its error dialog ("This media could not be
  loaded..."), which the Chromium recording of the M13 exit test (it has
  Media Source Extensions and plays the stream) does not have: 288 and 56
  differences in `values` and `events` of
  `crates/automation/tests/videojs.rs`. Chromium 148 without `MediaSource`
  shows the same dialog (review of this feature; the diagnosis, section
  10, is corrected): `loadstart` (ns 2, rs 0), then `error` (ns 3, rs 0,
  code 4, `PipelineStatus::DEMUXER_ERROR_COULD_NOT_PARSE`: its HLS player
  fails at the rendition playlist, which the fixture lacks). Video.js opens
  the dialog on any `error` of the element, so swb needs no change. The two
  recorded sequences load with `Page::set_media(false)` (media elements
  load nothing and fire no events, as before M14;
  `load_at_capture_without_media`), so that they still measure the page;
  `without_media_source_the_player_fails_and_shows_its_error_dialog` loads
  the page with media on (with and without the feature) against
  Chromium's records without `MediaSource` (the replay tool with an init
  script that deletes it, three equal runs, `data/videojs-no-mse-*`; swb's
  test inserts the same script at the start of the document). Departures:
  the message, `progress` and `suspend` before the `error` with the
  feature (bytes arrived), and `canPlayType` of HLS (`""`). A real visit
  to videojs.org shows the dialog until M15, as Chromium without
  `MediaSource` does.
- **The selection pointer** (review N1): the position of the resource
  selection in the `source` children is HTML's *pointer*, kept as the child
  before it (`select::Pointer`): the next candidate is found from there, not
  by a scan from the first child (O(n) for n skipped sources instead of
  O(n²): 20,000 took 515 ms before). A removal of the child before the
  pointer moves it to the child before the removed ones (from the
  `Change::Children` of the journal, in its order), so the pointer does not
  move relative to the children that stay; an insertion at the pointer
  comes after it.
- **Hostile cases** (`tools/swbtools/hostile_cases/media.py`, default build):
  20,000 `<source media='not all'>` (0.15 s), one million `load()` calls in
  one task (2 s, 149 MiB: the event queue is not bounded, backlog), 16,000
  `<video src>` (2.2 s, 131 MiB).
- **Measured**: the 65 cases of `media-states` (62 compared; 3 skipped: two
  play or seek, one has only a layout row), the 45 cases of `media-fetch`, the 149 types
  of `media-can-play-type`: all equal to Chromium's except the departures
  above. The whole `media_states` binary takes 0.2 s in virtual time.

## Implementation notes (M14 feature 6b)

The playback API of the element (`crates/engine/src/media/playback.rs`,
`step.rs`, `load.rs`; `crates/web/src/media_element.rs`). Decisions that
sections 3, 7 and 10 left open, and findings of the implementation.

- **The API between the engine and the bindings.** `EngineHost` has
  `media_play(node) -> PlayStart` (`Pending(id)`, `NotAllowed`,
  `NotSupported`, `TooMany`), `media_command(node, MediaCommand)` (`Load`,
  `Pause`, `Seek`, `SetVolume`, `SetMuted`, `SetPlaybackRate`,
  `SetDefaultPlaybackRate`) and `media_task_begin(owner, epoch, task)`.
  `MediaInfo` has `played`, `ended`, `volume`, `muted` and the rates. The
  engine queues `MediaTask`s (`Event(name)`, `ResolvePlay(upto)`,
  `RejectPlay(upto, why)`) and the page hands them to
  `ScriptRuntime::media_events` as `MediaDelivery`s. The `play()` promises
  live in the web crate (`DocState::plays`, by node and number); a settling
  task settles all promises of the element with a number up to `upto`,
  because HTML settles all pending promises together (so the engine keeps
  one number for each element, not a list). `volume` outside 0 to 1 is
  `IndexSizeError` in the bindings; a value that is no finite number is the
  glue's `TypeError` (Chromium's messages, measured). `new Audio(src)` is
  the legacy factory (`preload="auto"`, then `src`).
- **Tasks are dropped by a load (HTML "media element load algorithm", step
  6).** Each queued task carries the element and its epoch; a load increases
  the epoch, and the tasks of older epochs do not run, also those that the
  page took in a batch and has not run yet (`media_task_begin` checks at the
  run; the first version checked only at the take, and `canplay` after a
  `seeked` whose listener called `load()` still ran). Measured: `play()` then
  `load()` fires no `play` and no `waiting`; `currentTime = 1.5` then `load()`
  fires no `seeking`. The promises whose settling was dropped reject with
  `AbortError` ("interrupted by a new load request"): the engine knows the
  highest number that a delivered task settled.
- **The stable-state step is a marker in the queue** (`Entry::Select`).
  Chromium dispatches `play` and `volumechange` of a script that follows the
  creation of the element with `networkState` 2 (the selection ran) and
  `loadstart` after them, while `abort` and `emptied` of a `load()` see
  `networkState` 3. The marker is queued where HTML says "await a stable
  state": events before it are dispatched in the state of the synchronous
  part, `Media::step` runs the selection when the marker is at the front, and
  the events queued after it see its state; the step queues `loadstart` at the
  end. (A marker that blocked the delivery of other elements' events made the
  events depend on the threads: the page takes events up to the marker and
  `step` handles it, so the events of other elements before it go first.)
  A marker that a later load replaced is dropped where it is met, and counts
  as progress of the step: without that, the tasks behind it waited for a
  round that never came (a script's `load()` before the first step stalled
  the page; `media_without_feature.rs` has the test), and a million loads
  cost one round each (the hostile case `media-load-flood` took 30 s; it
  takes 0.75 s now, 2 s before 6b).
- **The policy** (measured, `media-autoplay.json`, 27 cases): `play()` is
  allowed when the document has sticky activation, or the element is a
  `video` that is muted or has the volume 0 (`<audio>` and `new Audio()`
  need the activation also when muted; a `video` without an audio track too).
  A blocked `play()` rejects with `NotAllowedError` at once and changes no
  state; Chromium checks the policy before the error of the element. An
  `autoplay` element that the policy allows plays from the first data. Chromium's
  order differs by the reason: with an activation `loadeddata`, `canplay`,
  `play`, `playing`, `canplaythrough`; as a muted `video` (no activation)
  `loadeddata`, `canplay`, `canplaythrough` while paused, then `play`,
  `playing` (swb makes the second start a unit of its own, so that the first
  three are dispatched with `paused` true). `autoplay` also overrides
  `preload=none`. A `video` that plays only because it is silent pauses (events
  `volumechange`, `timeupdate`, `pause`) when a script unmutes it before the
  document has an activation: this is Chromium's rule, which two of three
  runs of `attr-autoplay-muted-then-unmuted-by-script` show (the third had the
  activation of the probe's first evaluation before the unmute, and
  `unmute-by-script-without-gesture`, which unmutes 200 ms after `playing`,
  keeps playing for the same reason). Correction: the diagnosis (section 4)
  and section 10 say that Chromium leaves the promise of `play()` pending for
  an unmuted video that a script creates and attaches. The case
  `created-video-play-at-load` throws at `document.body.appendChild` (the
  script runs in the head, before `<body>`: `document.body` is null), so
  `play()` never ran; swb's result is the same, and there is no quirk.
- **User activation for evaluations** (the measurement that section 10
  asked for). `Page::start_evaluation` and `evaluate_in_frame` give the
  document the activation (`Page::note_activation`) before the script runs:
  every `--eval`, `eval` step and `page.evaluate` (`Page::evaluate*` is their
  one path). The tests that read the activation state were run with it on:
  `probe_net` (9 tests, with `top_layer` and the `activation-*` rows),
  `frames` (312, the `xo-act-*` rows), `frame_oracle`, `script_input`,
  `top_layer`, the automation tests `popover_input`, `frame_input`,
  `nested_activation`, `real_input`, and `swb-web`'s `input_events` (the web
  crate's tests, `message_channel` among them, use a host of their own that
  does not go through `Page::evaluate`; they do not measure this change).
  No result changed, so the activation is the rule for every evaluation, not an
  option of the media tests. `Page::set_evaluation_activation(false)` turns it
  off (a test of a page that must see none; `evaluation_activation.rs`). The
  scripts of the automation's own give none (review fix): the `focus` step
  calls `focus()` through `Page::evaluate_internal`, as Playwright's
  `locator.focus()` gives no activation; only the requests of the user or of
  the automation client (`--eval`, the `eval` and `record` steps,
  `page.evaluate`) go through `Page::evaluate` (`focus_activation.rs`: after
  a `focus` step `navigator.userActivation` is false, after an `eval` step
  true). The GUI, the shell, `javascript:` URLs and the engine's own scripts
  do not call `Page::evaluate*`. The
  probe tool's evaluations also carry it (rows `evaluate-carries-user-
  activation`). The tests that compare with the autoplay rows run the case's
  script at load without one, then give one before the first step, as the
  probe's wait for the fonts does.
- **Playing.** `play()` follows the HTML internal play steps: a selection for
  `NETWORK_EMPTY`, the deferred fetch of `preload=none`, the restart at the
  start when `ended` (the flag of the end: after a seek to the end Chromium's
  `ended` is false, and `play()` plays on and ends at once, measured),
  `paused` false, `play`, then `playing` and the promises if the data is
  there, else `waiting`. The pipeline plays at "notify about playing"
  (`playing`), pauses at the pause steps, seeks at a seek. `playing` after
  the data came (`loadeddata`, `canplay`, `playing`, `canplaythrough`), after
  a seek and after an underrun. The promises resolve after `playing`. `pause()`
  fires `timeupdate` and `pause` and rejects the pending promises with
  `AbortError` ("interrupted by a call to pause()"); a pause while nothing
  plays fires nothing; the end rejects them too. A pause and `autoplay`
  clear the "can autoplay" flag. A failure of the resource (review fix; rows
  `play-pending-src-empty`, `-bad-url`, `-404`, `-garbage`: `play()` before the
  failure) rejects pending promises with `NotSupportedError: Failed to load
  because no supported source was found.` and pauses an element that was
  playing or about to (`paused` true; `error`, `timeupdate`, `pause`): the
  first version left it unpaused and rejected without a pause. The `error` and
  the pause are a unit of their own after the events of the failing unit
  (`failed_pending`, `step_failed`): `play`, `waiting`, `loadstart`, `progress`
  and `suspend` see `paused` false, `error` sees it true, as Chromium's. A call
  of `play()` on an element that already failed rejects at the call with `The
  element has no supported sources.`. An element with no `src` and no `source`
  keeps its promises pending (measured). `playbackRate` and
  `defaultPlaybackRate` are stored values with `ratechange` (backlog: the
  rate does not change the playback). A `playbackRate` outside Chromium's
  range (0, or 1/16 to 16: 0.0625 and 16 are in, 0.0624 and 16.1 are out)
  throws `NotSupportedError: ...The provided playback rate (N) is not in the
  supported playback range.`, with `N` in six significant digits (`0.06`,
  `1.00000e-9`, `1.23457e+6`: the form of `number_text`); a `defaultPlaybackRate`
  outside it is ignored without a throw and without `ratechange`; a change of
  either fires `ratechange` (rows of `media-api-edges.json`).
- **Seeking.** `currentTime = t` before the metadata is the default start
  position (reads `t`, also a negative one: `-5` reads `-5`, and the playback
  starts at 0; no `seeking`, no load under `preload=none`); after it
  `seeking` is true at once, `readyState` goes to 1 (with `waiting` when the
  element plays), `seeking` is queued (once for each assignment, measured),
  and the pipeline seeks (only the last of several seeks ends). The end:
  `timeupdate`, `seeked`, and the data is there again (`canplay`,
  `playing` if it plays, `canplaythrough`), waited for in virtual time as the
  first data is (`has_enough_data`). The target clamps to 0 and the duration. A
  position that a script set before the metadata and the start of a media
  fragment `#t=` (`NPT`, `t=10`, `t=1:30.5`, `t=npt:...,end`; the start only)
  seek at the unit of `loadedmetadata`: the seek ends before the first data,
  and the unit handles both notes together (`timeupdate`, `seeked`,
  `loadeddata`, `canplay`, `canplaythrough` with `readyState` 4, as
  Chromium). `pause()` during a seek fires its events at once and the seek
  ends without `playing` (`pause-during-seek`). A seek of an element whose
  pipeline failed ends at once.
- **The end and `loop`.** `Report::Ended` becomes `timeupdate` and then
  `pause` and `ended` (`paused` true, `ended` true), or with `loop` a seek to
  the start in the next unit (`seeking`, `waiting`, `timeupdate`, `seeked`,
  `canplay`, `playing`, `canplaythrough`; no `pause` and no `ended`;
  `loop` is read at the end, so setting it near the end works). The unit
  after the end is a unit of its own so that `timeupdate` is dispatched at
  `currentTime` 2 (Chromium's), not 0. Feature 5's rule of the cut file (a
  file whose data ends more than 1 s before its duration ends with the data)
  is gone: Chromium plays the truncated WebM and MP3 of `media-states` to the
  duration (`ended` at `ct` 2), so a pipeline with a known duration ends at
  the duration (silence and the last frame where the data ends); only a
  stream without a duration ends with its data.
- **`played`** is kept in `Element::played` (ranges that merge when they
  touch) and the run that goes on. A run starts at `play()` (Chromium: a media
  with a start time of 1.5 s that `play()` started before the metadata has
  `played` 0-3.5), and goes into the ranges at a pause, a seek, the end, a
  removal and a load that clears it. A run without time leaves a range `0-0` at
  a pause (the removal of a playing element), and none at the end (`play()` at
  the end after a seek to the end). An element without metadata has no media
  to have played: `play()` then `pause()` of an element without a source
  leaves `played` empty (review fix; the first version left `0-0`).
- **Removal and re-insertion.** An element that leaves the document runs the
  internal pause steps (`timeupdate`, `pause`; measured, also in the probe
  row of the removal), and its pipeline suspends at once when it has all
  its data. Re-insertion starts nothing (Chromium does not resume
  playback); `play()` or a seek opens the suspended pipeline again
  (`Fetch::play` and `Fetch::seek` clear the flag; `Pipeline::play`
  resumes). Every media element has a state from its insertion, for
  `muted` (its content attribute at the creation, a value of its own after)
  and `volume`; the table is bounded as before, and the elements that go
  first at the bound are those outside the document that do not play.
- **`new Audio()`** is an element without a position in the document.
  Its state lives in the same table; it takes a pipeline place as the others.
  A state lives while its element plays (the purge at the bound keeps them) or
  is in the document; swb does not know whether script can reach a paused
  detached element (backlog: tell the engine when the wrapper is collected),
  so such states stay until the bound drops them.
- **The late failure** (rows `err-corrupt-mp4`, `err-truncated-mp4`,
  `late-failure-damage-30`, `-60`, `-80`, `-90`; test `late_failures_follow_the_
  damage`; the rule is the one of the review fix, see "Review fixes of feature
  6b" below). The pipeline that meets damage after the metadata ends its run
  and cannot play any more; Chromium loads the file and plays on, and fails
  when the playback comes to the damaged packet (code 3, `error`,
  `timeupdate`, `pause`). `Fetch::late` is kept as `Element::damage`; the
  element counts the media time on the page clock (`Emulation`, whole
  microseconds; the `timeupdate` points continue the grid of the pipeline,
  `Pipeline::next_timeupdate`) and fails where the failing thread stopped
  (below). The message is swb's own (`PipelineStatus::PIPELINE_ERROR_DECODE:
  video decode error!`; Chromium names the packet).
- **Virtual time.** The page time goes to a task as `set_media_time` (at the
  lending of the document), so that `play()` and `currentTime` read the time
  of the task, not of the last pump; `currentTime` while playing is
  `Pipeline::time_at(now)` (the last position, moved on by the clock). The
  pipeline's due times come back as page time (ms) and go in as microseconds:
  `ms_to_us` rounds (truncating made a due point one microsecond away, so that
  `advance_to` of virtual time jumped to it again and again). The suspend rule
  and the emulation's due times are whole microseconds (`idle_since`,
  `SUSPEND_AFTER_US`, `Emulation`), so they compare without a margin; the first
  version had a margin of a nanosecond for the same one-ULP shortfall. Both
  made the 9th of nine media played one after the other hang (`many_media_play_one_
  after_the_other_in_one_page`); the 6a tests had whole milliseconds and did
  not show it. A playing element is `pending` while the pipeline needs data
  and while one of its own units waits (the end of a looping media, the start of
  an autoplay); its due times (`Pipeline::next_due`, the emulation's) join
  `Media::next_due`, so `Page::next_task_time` has them with JavaScript off too
  and a page with a playing video is not idle (`wait_until_idle`).
- **Bounds** (hostile): the queue holds at most 65,536 undelivered events,
  and a settling of promises joins the one before it (a flood of `play()`
  makes one entry); at most 65,536 `play()` promises of an element wait for
  their settling, the others reject with `AbortError` ("Too many play()
  requests are waiting"); a flood of seeks queues `seeking` up to the bound
  (the last seek ends with the last value). Tests: 30,000 pairs of `play()`
  and `pause()` settle every promise; 70,000 promises: 65,536 reject at the
  `pause()`, the others at once; 200,000 seeks; 1,000 muted `autoplay`
  videos through 16 places end (2.3 s of real time for the five tests of the
  floods and the 1,000 videos). Hostile cases `media-play-pause-flood`,
  `media-many-play-promises`, `media-seek-and-audio-flood`.
- **Measured** (`media_playback.rs`, `media_autoplay.rs`; three runs of
  each probe in Chromium 148): the 30 cases of `media-playback`, the 27 of
  `media-autoplay`, and 5 new cases of `media-playback-2` (play after a
  seek to the end, removal and `play()` again, `autoplay` cancelled by
  `pause()` at `loadedmetadata` and at `canplay`, `pause()` during a seek):
  equal to Chromium's within the rounding of the probe (0.25 s; Chromium's own
  `ct` after a pause varies by 0.04 s between runs) except these departures.
  `media-playback`: `new-audio` reads `readyState` 1 at `progress` and
  `suspend` where Chromium reads 0 (the unit of the metadata, 6a).
  `media-states` now also compares the `play` parts of the `err-*` rows,
  `sel-fragment-time` (`progress` and `suspend` read `ct` 1 where Chromium
  reads 0: the seek is part of the unit of the metadata), `sel-currentSrc-url`
  and `format-play-to-end-table` (the M4A's `buffered` starts at -0.02 in
  Chromium); `media-fetch` compares the seek of the two `ignore-range`
  cases. Not compared: the events of two elements in one page in Chromium
  interleave differently from swb's (the probe case that showed it was split
  in two); `attr-autoplay-muted-then-unmuted-by-script` varies in Chromium
  (above).

### Review fixes of feature 6b

- **`timeupdate` at the end.** `Pipeline::advance` emitted a `TimeUpdate` at
  the end when the clock reached it before the decoders drained, and
  `note_ended` queued another: 9 `timeupdate` events instead of 8 in 2 of
  about 130 runs of `media_playback` (case `timeupdate-rate`). The end owns
  the last one now: no `TimeUpdate` at `m >= end`. A unit test puts the
  pipeline in that state (clock at the end, video decoder not drained:
  `the_end_owns_the_last_timeupdate`), and 300 runs of `media_playback` (half
  of them with two busy loops per core) pass. The other
  places where the presence or the count of an event could depend on whether a
  thread had finished were read: `Underrun`, `Resumed` and the audio gap
  happen only in real time (in virtual time the owner waits for the data);
  `Ended` waits for `end_reached`, which only grows with decoding; the first
  frame and the end of a seek wait for the data (`step_note`, `strict`);
  `note_time_update` drops a tick after a pause by the order of the notes,
  which the lockstep fixes; the late failure is the next item. `suspend` and
  `progress` depend on the byte source's state, which the 6a tests
  (`the_events_are_the_same_on_every_run_also_under_load`) cover.
- **The order of `loadeddata` and `seeked` after the seek of the metadata.**
  The load check (20 runs of the four media tests with two busy loops per
  core) failed `media_states` (`sel-fragment-time`) in 8 of 30 runs: when the
  first frame was decoded before the unit of the metadata, the note of the
  first data came before the end of the seek that the unit started (a `#t=`
  fragment, a position that a script set), and `loadeddata` was dispatched
  before `seeked`; Chromium has `seeked` first. `Fetch::seek` now moves a
  note of the first data that waits behind the end of the seek
  (`first_data_held`), the order that the pipeline gives when the first frame
  comes after the seek. 0 failures in 30 runs after. The delta review found
  that a pipeline that fails during that seek never reports its end, so the
  held note never came and the element stayed at `HAVE_METADATA` (a `#t=1`
  fragment of the file damaged at 60 %: 4 of 16 pages under load). A late
  failure now gives the held note too; the test
  `a_failure_during_the_start_seek_still_gives_the_first_data` (32 pages
  under load) covers it. With a late failure and a `#t=` fragment the order
  of `loadeddata` and `seeked` still varies between runs, and a failed start
  seek fires no `seeked` (Chromium not measured; backlog).
- **The late failure comes where the decoder stopped** (the first version
  failed 0.5 s after the start of the playback whatever the damage: damage at
  0.3 and 0.5 of the 2 s file gave `ct` 0.5, 0.6 gave 0.92, 0.8 gave 0.48, 0.9
  gave 1.68; 0.8 failed before 0.9, and the time depended on the threads).
  Measured in Chromium 148 (three runs; `late-failure-damage-*` of
  `media-playback-2.json`, the files `mp4-h264-high-aac-damaged-N.mp4`, 600
  bytes of a fixed generator at N percent of the file, made by `tools/media/
  generate.py`; `late-failure-long-60` for the 10 s file `mp4-h264-aac-long.mp4`
  damaged at 60 percent, whose damaged copy (300 KB) is made by the test):

  | damage | error `ct` in Chromium | place of swb's decoder | swb's `error` |
  |--------|------------------------|------------------------|---------------|
  | 30 %   | 0.12                   | 0.37                   | 0.17          |
  | 60 %   | 0.80, 0.81, 0.81       | 0.96                   | 0.76          |
  | 80 %   | 1.22                   | 1.46                   | 1.26          |
  | 90 %   | 1.44                   | 1.69                   | 1.49          |
  | long 60 % | 5.64                | 5.90                   | 5.70 (5.72)   |
  | long 70 % | 6.64                | 6.90                   | 6.70          |

  The rule: the thread that fails says where it stopped (`Pipeline::failed_at`:
  the end of the last frame that came out of the video decoder, or of the
  audio that the demux thread decoded; the value is the same on every run, 12
  runs of each file in `a_decode_failure_has_the_same_place_on_every_run`), the
  pipeline lowers it by `READY_AHEAD_US` (0.2 s), and the element plays on to
  that time and fails there. Chromium decodes about 0.2 s ahead of its playback,
  so its error comes 0.15 to 0.26 s before the place of swb's decoder (the six
  rows above); with 0.2 s swb's error is within 0.06 s of Chromium's in every
  row. The same 0.2 s makes the time of the arrival harmless in virtual time:
  the owner goes on to a media time `t` only when the data up to `t +
  READY_AHEAD_US` is decoded (`ready_for`), so when a decoder fails at `F` the
  playback has not passed `F - READY_AHEAD_US`, and the element's playback from
  where it is to the place has the same events whatever the speed of the
  threads (the first try subtracted the 0.2 s in the element only, and the
  playback passed the place before the failure arrived in some runs: 0.92
  instead of 0.76). The `timeupdate` points continue the pipeline's grid (`Pipeline::next_timeupdate`). The
  requirements are met by the rule: a later damage never fails earlier (the
  place only grows with the position of the damage; the rows above and
  `late_failures_follow_the_damage`), `currentTime` does not pass the place
  before the `error` (a test of the page and one of the pipeline), and the
  events are the same on every run in virtual time (twelve pages at once,
  three times, with a busy thread per core:
  `a_late_failure_while_playing_has_the_same_events_on_every_run_also_under_
  load`, with the 10 s file damaged at 70 %). Departures: (1) when the video
  decoder and the audio decoder fail at different places, the thread that
  reports first decides. Measured: the 10 s file damaged at 40, 60 or 90 %
  hits both at nearly the same time, and 20 runs under a busy machine gave
  two places 0.02 s apart (5.6975 and 5.72 s at 60 %; 19 against 1). The
  delta review found the same for the 2 s file at 80 % (1.2625 s from the
  video, 1.28 s from the audio; 3 of 40 runs under load), so
  `a_decode_failure_has_the_same_place_on_every_run` allows a spread of
  25 ms. The 10 s file at 70 % hits one decoder and has one place. A rule
  that waits for
  both decoders (each track ends at its damage and the pipeline reports the
  failure at the minimum) is backlog; (2) in real time the clock runs by itself and
  may pass the place before the failure arrives, so the error comes at the
  position of the playback then; (3) a failure without a place (the source, the
  demuxer of a file without audio, the no-progress limit) fails at the position
  of the playback; (4) `err-corrupt-mp4`'s Chromium error names an audio packet
  at 0.859 s, `ct` 0.5 (swb: 0.7); the 0.26 s tolerance of the probe test holds.
  A seek of the element after the failure ends at once, and playing again from a
  position past the place fails at once, at that position.
- **`play()` before a failed source** (rows `play-pending-src-*`, the review's
  mutation M15): see "Playing" above. The test of the rows is the probe test of
  `media-playback-2.json`; with the rejection or the pause removed, four rows
  fail.
- **The `focus` step gives no activation** (`Page::evaluate_internal`, test
  `focus_activation.rs`): see "User activation for evaluations".
- **API edges** (`tools/probes/media-api-edges.json`, 99 rows of one query;
  `media_api_edges.rs`; three equal runs in Chromium): `currentTime = -5` before
  the metadata reads -5, `playbackRate` outside 0 and 1/16 to 16 throws,
  `defaultPlaybackRate` outside it is ignored, `play()` then `pause()` without a
  source leaves `played` empty (see "Playing", "Seeking", "`played`" above).
- **The comparison with Chromium is exact except for times**
  (`common/media_probe.rs`): the tolerance of 0.26 s applies to the numbers under
  the keys of `TIME_KEYS` and to the numbers of a text that are not `rs=`,
  `ns=` or the value of an error message; `volume`, `readyState`,
  `networkState`, counts and rates are equal. A bias of 0.2 in `volume` now
  fails the rows `volume-muted`, `load-after-ended-and-paused` and the edges.
- **Whole microseconds** (see "Virtual time"): the 1 ns margins are gone; the
  15 s suspend and `many_media_play_one_after_the_other_in_one_page` pass as
  before.
- **Fast failure of a loop that does not restart**
  (`a_looping_media_seeks_to_the_start_at_its_end`): the evaluation has a 5 s
  limit and checks `seeking` at `currentTime` 0 and no `ended`; without the
  restart the case of the probe test spun in virtual time for the 60 s of its
  limit.
- **Feature 5's hostile-file test** now checks where the playback of a cut or
  damaged file ends: at the duration.
- **The epoch check in `take_events`** stays: `task_begin` drops the same
  tasks at the delivery, so the check changes no result; it saves the budget
  of a round for tasks that a load dropped (a comment says so).
- **`media_autoplay.rs`**: the data pins swb to one of the two outcomes of
  Chromium for `attr-autoplay-muted-then-unmuted-by-script`; `DEPARTURES` is
  empty because swb does not differ from Chromium.

## Implementation notes (M14 feature 7a)

Video frames in paint, headless: what a screenshot and the display list show
(`crates/engine/src/media/frames.rs`, `resources.rs`, `document/media.rs`,
`page/media.rs`; `crates/paint`; `crates/media`). The GUI frame loop is feature
7b. Decisions that section 9 left open, and findings of the implementation.

- **The color rule, measured** (probe cases `frames-color-untagged-bt709-WxH`
  of `tools/probes/media-frames.json`, three equal runs; 11 new files made by
  `tools/media/generate.py`, 0.6 s each, content converted with BT.709 and not
  tagged). Chromium converts an untagged stream as BT.709 when its height is
  720 lines or more, else as BT.601: 1280 x 544, 1280 x 480, 1024 x 576, 960 x
  540, 1280 x 600, 1024 x 640, 1280 x 704 and 1280 x 718 are BT.601 (the
  conversion within 3 levels; BT.709 would be off by 21), 720 x 720, 960 x 720
  and 1920 x 1080 are BT.709 (601 would be off by 23). Width and area do not
  decide (a wide, low frame is 601; a narrow, tall one is 709): the rule of
  `swb_media::info::conversion` (`UNTAGGED_BT709_LINES` = 720, the height of
  the stream: the cropped height, so 1280 x 718 is 718 and BT.601) was right
  and did not change; its comment and tests record the
  numbers. `frames.rs` compares 1,556 points of 54 frames (22 cases; the 11 new
  files and the 3 of the diagnosis among them) within 3 levels (worst 3), and
  `the_untagged_files_follow_the_height_rule` checks the matrix of each file.
- **The Wikipedia VP8 file** (container BT.709, FFmpeg's decoder tags every
  frame BT.601): the probe case `real-frames-color-wikimedia-vp8` (in
  `media-real-files.json`; 20 points on saturated areas of the frame at
  16.02 s, which is frame 398 of the file: its frame times are not on a grid)
  shows Chromium on BT.709 (at most 5 levels from the BT.709 conversion, the
  points at an edge; 601 differs by 24 and more). So the stream's tag wins over
  the frame's, as `conversion` already had it; the test
  `the_wikimedia_file_converts_by_its_container_tag` reads the local file and
  passes without it (a message). Without a stream tag, the frame's tag still
  applies (a stream that FFmpeg tags only on its frames).
- **The frame as the image** (`Images::videos`). For each video with a media
  state the document keeps the natural size of the stream, whether the element
  represents its poster, and the current frame: the pipeline's pooled pixmap
  (`Arc<Pixmap>`) under a `DecodedImage` that shares it
  (`DecodedImage::from_shared_pixmap`, `ImageKind::Raster(Arc<Pixmap>)`: no
  copy; the pool takes a pixmap back when the image goes). `Images::get` for
  `ImageRef::Node` of a video returns the frame, or the poster in poster mode.
  `DocumentState::sync_media_images` brings it up to date after each step of
  the media, after a command or `play()` of a script (so a listener sees the
  layout), and before a paint (the pipelines first take the frames due at the
  page time, `Page::sync_media_frames`; in virtual time only when no media has
  pending work). A new frame bumps the node's content generation
  (`ImageSource::generation`), so the frame is outdated and the damage is the
  item's bounds; the layout and the display list stay (`FlushCounters::
  layout_runs` does not move). The layout is outdated only when the natural
  size changes or whether a frame shows changes (the first frame, `load()`).
- **The poster flag** (`Element::show_poster`): set at each load, cleared by
  `play()` (the internal play steps), by an `autoplay` start and by a seek after
  the metadata (also a seek to 0; a `currentTime` before the metadata only sets
  the start position, so `preload=none` and `currentTime = 1` keep the poster).
  Poster mode is the flag with a non-blank `poster` attribute: the poster shows
  when it is loaded, nothing while it loads and after it failed (the page
  background, as Chromium: row `poster-missing-file`); `removeAttribute('poster')`
  shows the first frame at once. **A loaded poster also shows with the flag
  clear while the current load has no frame** (review of 7a, finding 1;
  `set_video_image`: poster mode is `poster && (flag || no frame)`): a video
  with a poster and no `src` shows the poster after `play()` (Chromium row
  `poster-play-no-src`, three equal runs), and so does one whose file fails to
  open after `play()` (`poster-play-garbage-file`); the gap between `play()` and
  the first frame of a slow load is the same rule, tested in real time over a
  slow link (`a_loaded_poster_shows_after_play_until_the_video_has_a_frame`; the
  probe tool's server has no delay, so Chromium has no row for the gap).
  The 24 cases of `media-poster.json` (21, `poster-failed-file-with-frame` and
  the two above) match Chromium on every `pixels` row; two rows
  (`poster-play-pause-at-start`, `poster-play-and-run`) differ between runs of
  Chromium by the frame that its real-time clock shows, so a row passes when swb
  equals one of the three recorded runs.
- **The natural size** (`ImageSizes::size`): the poster's while the element
  represents its poster and the poster is loaded, else the stream's display size
  (`videoWidth` by `videoHeight`). Measured (`frames-natural-size-*`): a 200 x
  100 poster on a 320 x 240 video gives a box of 200 x 100, 320 x 240 after
  `play()` and 200 x 100 after `load()`; without a poster and with `preload=none`
  the box is 300 x 150 until the load; a stream with the sample aspect ratio 2:1
  (160 x 120) is 320 x 120 and with 1:2 is 160 x 240 (as `natural_size` already
  had it; two new files `mp4-h264-sar-2x1.mp4`, `-1x2.mp4`); 321 x 241 is exact.
  A change gives a new layout in the step that brought it, before the events of
  that step are delivered (`resize`, `loadedmetadata` listeners read the new
  size). No fixture has a stream whose frame size changes: swb draws the frames
  into the natural size of the metadata and fires no `resize` for a change
  (backlog).
- **Placement** (the 42 cases of the engine's data `tests/data/media-frames.json`:
  30 placement cases, the first frame and the end, `frames-dense-contain-320`,
  `frames-dense-contain-odd`, the natural-size, controls and `display` cases;
  all `pixels` rows within 3 levels except the points of the departures below).
  Four findings:
  1. Chromium draws the frame into a rectangle whose origin and size are
     rounded each: a 320 x 240 video that `object-fit: contain` puts at 66.67 to
     333.33 of a 400 x 200 box covers the pixels 67 to 333 (267 wide), and the
     321 x 241 video 67 to 332 (266). An image snaps by its edges (67 to 332).
     `ImageSource::rounds_size` is true for a frame (`snap_by_size` in
     `raster.rs`); the dense row `frames-dense-contain-320` (560 points) matches.
  2. `border-radius` clips the video's content (the user-agent style sheet gives
     it `overflow: clip`); the paint of the box drew the content before the
     overflow clip, so the content now has its own rounded clip of the padding
     edge (as the frame of an `iframe`). `<img>` has the same gap (backlog).
  3. The default poster (#333; a video with controls and without a `poster`
     attribute) is under the frame only until the first frame: with a frame the
     letterbox is transparent (`frames-controls-letterbox`: page background at
     the sides, with the controls' gradient over the bottom). The layout gives the
     fragment `default_poster` only while no frame shows (`ReplacedSizes::
     shows_video_frame`), and the display list pushes the poster fill and the
     image both (it pushed only the fill).
  4. Poster and frame are drawn by the same item; a frame at a transform, scale
     or filter draws as an image (the transform, scale, opacity, filter, clip-path
     and overlap cases pass).
  Departures (`DEPARTURES` of `media_frames.rs`, 2 points): the anti-aliased
  edge of a circle clip (6 levels); one point of the frame drawn at 0.37 of its
  size (14 levels: swb draws it from an averaged level, `swb_paint::reduce`).
  **Odd frames** (review of 7a, finding 4; the 321 x 241 video): the first
  version allowed 10 points of up to 140 levels and left the dense row
  `frames-dense-contain-odd` (560 points, three equal Chromium runs, now in the
  data) uncompared; it differed at 223 points, up to 190 levels. The cause is
  in the conversion, not in the placement: the chroma planes of an odd frame
  have ceil(size / 2) samples (161 x 121), and swscale scales a chroma plane by
  the ratio of the plane sizes to the output size (161 / 321, not 1 / 2), so
  the chroma drifts against the luma by up to half a chroma sample across the
  frame (Chromium upsamples exactly twice). `swb_scaler_rgba` (`ffmpeg.c`) now
  converts such a frame from a copy padded to a multiple of 16 in width and to
  an even height (the last column and row repeat) and crops the result
  (planar YUV with subsampled chroma only; a conversion that also scales keeps
  the old path; fix round 2 below changed the multiple to 64 and the cases to
  every width that is not a multiple of 64). A padding to the
  next even width alone left the last column wrong (swscale's tail of a width
  that is not a multiple of 16: the last column came out as a mix); the multiple
  of 16 gives the exact data. All 560 points of the dense row and every odd-frame
  point now match within 3 levels, so those departures are gone.
  **Fix round 2: a heap overflow in the conversion** (delta review of the fixes
  of 7a, finding 1; it exists since feature 3). For 8-bit 4:2:0 and 4:2:2 frames
  (`yuva420p`, `yuvj*` too) whose width is not a multiple of 16, `swscale`
  wrote up to 15 pixels past the end of the last row of `dst`: 8 bytes for
  1918 x 1078, 24 for 426 x 240, 56 for 2 x 2 (0 for 4:4:4, 10-bit and NV12).
  `Decoder::convert` passes the exact `Pixmap` buffer, and the size is the
  media's, so the content controlled a write past a heap block (the sanitizers
  do not see it: FFmpeg is not instrumented). The same SIMD tail made the last 2
  to 6 columns of even widths with `w % 16` of 2, 6 or 10 (322, 854, 1366) differ
  from the padded conversion by up to 84 levels (finding 2). The rule now:
  - `swscale` writes into the caller's buffer only if the width is a multiple of
    64 pixels (1920, 1280, 3840, 640 and 320 are) and the frame needs no
    padding. The vector width depends on the CPU, so "a multiple of 16" is not
    enough on a machine with wider vector units; 64 pixels is a 256-byte row.
  - Otherwise it converts into the scaler's scratch buffer (stride rounded up to
    64 pixels, `rows + 1` rows, so a write past the last row stays inside) and
    copies the visible part row by row. The scratch and the padded copy of the
    source are kept between frames and freed with the scaler (no allocation at a
    steady size). The size of the scratch is computed in 64 bits and refused above
    `INT_MAX` (section 11 allows 16,384 x 4,352 pixels: 285 MB at most;
    `dst_w > INT_MAX / 4 - 64` is refused before).
  - Planar YUV with subsampled chroma converts from the source padded to a
    multiple of 64 in width and an even height (when the chroma is subsampled
    vertically), so `swscale` converts a plane of the padded size to the same
    size and has no tail; this fixes finding 2. `needs_padding` excludes packed
    YUV (`uyvy422`, `yuyv422`), RGB, palette and bitstream formats and formats
    with fewer than 3 components: the build's decoders (H.264, VP8, VP9;
    `tools/ffmpeg/configure.txt`) output planar YUV and `gbrp` (RGB: scratch,
    no padding).
  Tests: `the_conversion_stays_in_its_buffer` (`boundary.rs`) has an exact-size
  destination with a 256-byte sentinel for 12 files (new fixtures `webm-vp8-
  321x240`, `webm-vp8-320x241`, `mp4-h264-854x480`, `mp4-h264-322x242` and its
  4:4:4, 4:2:2 and 10-bit variants; H.264 4:2:0 cannot have an odd size); with the
  conversion of the first fixes (padding for odd sizes only, direct otherwise)
  it fails on the first new file; `frames_match_chromium_within_3_levels` has two
  new cases, `frames-color-odd-columns-854x480` and `-322x242` (smooth
  gradients, 3 equal Chromium runs), whose points cover the last 8 and 9
  columns (they also fail with that conversion). The cost (release build,
  `-O2`, 4:2:0, best of 5 runs of 50 frames, before to after): a frame that is
  not a multiple of 64 wide costs one more copy of the RGBA. 1920 x 1080 0.42 to
  0.42 ms (direct), 1280 x 720 0.18 to 0.18, 3840 x 2160 1.94 to 1.94, 1919 x 1079
  0.90 to 0.90 (padded before as well), 854 x 480 0.08 to 0.18, 2558 x 1626
  (the Wikipedia file) 0.86 to 2.8. The last is 17 % of a 60 fps frame; most of
  it is the copy of 16 MB out of the scratch.
  The retained frames have no byte budget yet (review of 7a, finding 3):
  every element that decoded a frame keeps one converted frame
  (`Images::videos`), also a suspended or detached one (300 paused
  `preload=auto` videos of 320 x 240 hold 300 frames after 300 s of virtual
  time, +257 MiB; 16,384 videos would hold 5 GB at 320 x 240 and 136 GB at
  1080p). A total pixel budget in `Images` (drop the frame of a suspended or
  not visible element and convert it again at `play()`, a seek or a box) and a
  hostile case of many 1080p `preload=auto` videos are feature 10, with the
  viewport rule of 7b.
- **Screenshots** (`Page::screenshot`, which the automation `screenshot` step,
  `page.screenshot` and the probe tool's swb side call). In virtual time the
  screenshot first lays the page out (a video's box says whether it is painted),
  then settles the media (`Page::settle_media`: so a hidden video that got a
  box has decoded its frame; the page time does not move), then takes the frame
  at the media time of the page clock (`Pipeline::update` at that time). **The
  settle is bounded and runs no page task** (review of 7a, finding 2; it ran
  `pump` for up to 30 s, so a `loadstart` listener that reloads the video from
  a timer made a screenshot take 30 s and ran timers and scripts in it): it
  steps only the media (`pump_media`: a step, then the delivery of the events
  that it queued, which the lockstep of `crate::media` needs, so the listeners
  of those events run) and waits for the pipelines' threads on the loader's
  wake-up, for at most 256 steps and 2 s of real time; then it draws what is
  there and logs a warning. No timer, animation frame or fetch of the page
  runs. A listener that calls `v.load()` itself from its media event is bounded
  by the steps and the 2 s. Tests: `a_screenshot_runs_no_page_task_and_ends_in_
  bounded_time`, hostile case `media-reload-loop-screenshot`. The media due times already stop virtual time at every frame, so the
  frame is the one that a playback shows at that page time: `a_screenshot_shows_
  the_frame_at_the_media_time_of_the_page_clock` (playing for 600 ms equals a
  pause at the same `currentTime` and a seek there), and 8 pages at once with a
  busy thread per core, 3 rounds, give the same pixels as one page
  (`screenshots_of_playing_videos_are_the_same_on_every_run_also_under_load`). In
  real time a render polls the pipelines and shows what is due (the GUI's frame
  loop is 7b): `a_render_in_real_time_shows_the_frame_that_the_clock_reached`
  (renders only, no step in between, until a newer frame shows; a bound of 10 s,
  not a fixed wait; it fails without `sync_media_frames` in `render_frame`).
- **The painted flag** (`Media::set_painted`, `Pipeline::set_painted`). The
  pipeline opens painted. After each layout (`run_layout`) the document tells the
  elements which videos have a box (a `BoxContent::Media` fragment): a video
  without one (`display: none`, not in the document) converts no frame (its
  pipeline decodes: the clock needs it), and the flag is kept across loads. A
  paused or ended pipeline that gets a box again and whose current frame has no
  pixels seeks quietly to its position (no `SeekDone`, position and `ended`
  unchanged: `Seeking::ended`): the backlog item of feature 5, tests
  `a_paused_video_that_becomes_painted_gets_the_pixels_of_its_frame`,
  `an_ended_video_that_becomes_painted_stays_ended`, and in the page
  `a_hidden_video_converts_no_frame_and_shows_its_frame_when_it_gets_a_box`. A
  playing video shows its next frame, a suspended one nothing until `play()` or a
  seek (backlog). The first layout may come after the first frames (a hidden video
  converts the frames that were decoded before it, at most the queue); the viewport
  and the hidden window are 7b.
- **Measured** (`just probe --with-swb` on `media-frames.json` (66 cases) and
  `media-poster.json` (22) with the release build: the probe's own comparison,
  exact for colors, so a point that differs by 1 level counts). Rows equal to
  Chromium's, of 242: **32 before** (the build without the feature, which draws
  no frame, as swb did before 7a: 15 of 127 `pixels` rows, 11 of 109 `js`
  rows, 6 of 6 `boxes` rows) and **183 after** (with the feature: 71 `pixels`
  rows, 106 `js` rows, 6 `boxes` rows). The `pixels` rows that differ in the exact
  comparison are within 3 levels in `media_frames.rs` (conversion and
  interpolation differ by 1 to 3 levels; the Rust tests list the points that
  differ more). Three equal Chromium runs for each file, except two rows of
  `media-poster.json` (above) and two cases of `media-frames.json` whose rows
  varied in full runs and not alone (`frames-color-untagged-bt709-1280x704`
  and `-1280x718`, 3 points each: four runs of the two cases alone were equal and
  are the data).
- **A regression that the hostile case found** (`media-16000-videos` in a media
  build, 60 s against 1.9 s): the first version of `sync_media_images` looked at
  every video after each unit of a step, which costs the square of the number of
  elements in virtual time (one unit at a time). It looks now at the elements
  that a unit or a script touched (`Media::touched`), and at all of them only
  for a paint or after the table was purged.
- **Hostile cases** (`hostile_cases/media.py`, a media build decodes):
  `media-100-autoplay-videos` (100 muted `autoplay loop` videos on screen) and
  `media-huge-object-fit-none-box` (a box of 10^6 px square with `object-fit:
  none` and a video scaled 1,000 times) and `media-reload-loop-screenshot` (a
  `loadstart` listener that reloads the video from a timer: the screenshot at the
  end is bounded). No fixture has a stream whose frame size changes: that case is
  not run.

## Implementation notes (M14 feature 7b)

The GUI frame loop for video: what makes the page's frame outdated at a video
frame's time, which videos convert frames (the painted flag), and what a
window costs. Code: `crates/media/src/pipeline/mod.rs` (`frame_due`),
`crates/engine/src/media/frames.rs`, `document/media.rs`, `page/media.rs`,
`page/pipeline.rs`, `crates/swb/src/gui/`. Playback is silent (audio is
feature 8).

- **The frame decision.** `Page::frame_outdated()` is true when a painted
  video has a converted frame due that it does not show
  (`Page::media_frame_due`, then `Media::frame_due`, `Fetch::frame_due`,
  `Pipeline::frame_due`). `Pipeline::frame_due(now)` is false at once unless
  the pipeline plays with a running clock, no seek and no underrun (a paused,
  ended, stalled, suspended or failed pipeline costs no lock). Else it takes
  the pipeline's lock, walks the frame queue (at most 6 frames) and tells
  whether the last frame due at the media time has pixels. It polls nothing,
  converts nothing and takes nothing: the frame stays in the queue for the
  next poll. A due frame without pixels (decoded while the video was not
  painted) does not count: paint would show the same picture. With 1,000
  playing videos on a page (16 pipelines run), the slowest decision in 6 s of
  scrolling was 0.02 ms (hostile case `media-1000-playing-videos-frame-loop`).
  Before 7b the GUI already drew each frame: the pump polled the pipelines at
  each wake-up (`step_element` polls every fetching element), the poll bumped
  the generation of the image, and the generation made the frame outdated.
  The new test means the decision no longer depends on a poll that something
  else ran (`the_frame_decision_sees_a_due_frame_without_a_poll`: reads only,
  no `pump`), and the frame is outdated at the time the frame is due, not at
  the next batch.
- **Where the frame time goes.** Not into `Page::frame_time()` (the answer to
  the question of 7b; section 9 said so and is corrected). `frame_time()` is
  the time of the next rendering update: it runs the `requestAnimationFrame`
  callbacks, `ResizeObserver` and the animation steps, it lies on the 60 Hz
  grid and at least one frame after the last update. Frame times of a video
  there would run a rendering update (and the callbacks of any page that
  waits for one) at 25 or 30 Hz for nothing, and would move each frame to the
  grid (25 fps is 2.4 grid steps: judder). A video frame changes no script
  state and needs no style or layout. So the frame time stays where it was
  since 6a: in `next_task_time()` through `media_next_due` (the GUI's
  `WaitUntil`), at the exact time of the frame, and the frame decision reads
  the queue.
- **No busy loop.** The pipeline's next due time is the first queued frame, the
  next `timeupdate` point or the end, and only while the clock runs (for a
  video that is not painted, in real time, the frame in the middle of the
  queue: "No frame times" below). A video
  that is paused, ended, seeking, stalled (an underrun stops the clock) or
  suspended has none: measured, 0 wake-ups and 0.0 % CPU in 10 s for the three
  files, paused and ended. A frame that the video thread has not queued yet
  is not due: while the queue is empty the next due time is a `timeupdate`
  point or the end, and the pipeline arms a notify (`arm_frame_notify`), so
  the loop wakes when the thread queues a frame, once, not repeatedly at the
  late time. A due time that nothing consumes does not spin the GUI either:
  `wake_deadline` makes a wake-up that ran no task and requested no frame wait
  2 ms before the next (`MIN_IDLE_WAKE`; a frame that is due within 2 ms
  after a wake-up that changed nothing waits up to 2 ms). Tests (`media_frame_loop.rs`,
  real time, the GUI's loop without a window): a paused video and an ended one
  give no render, no wake-up and no spin in 1 s, and their next due time is
  the 15 s suspend; a video on a slow link (2 KB every 250 ms, it stalls)
  neither spins nor draws more than its frames; the loop of a playing video
  has at most its frames and its `timeupdate` points as wake-ups.
- **A freeze at the wrap of a loop** (found while testing, a bug since feature
  5). A seek while playing (the wrap of `loop`) left `media_now` at the old
  position until the owner's next update, which waits for the seek. The video
  thread drops a frame as late when its successor is due at `media_now`, so
  every frame after the target frame of the seek counted as late and was
  dropped: 31 of 50 frames of the 2 s file, and the picture stood still for
  1.3 s at each wrap. `request_seek` now sets `media_now` to the target.
  Tests: `a_seek_while_playing_does_not_drop_the_frames_after_the_target_as_late`
  (44 frames dropped without the fix), `a_looping_video_goes_on_after_its_wrap`.
- **The painted rules** (`DocumentState::update_media_visibility`, replacing
  `sync_media_painted`). A video is painted when (1) the fragment walk that
  paint uses (`FragmentTree::walk_painted`) visits its box: it has a box, and
  it is not in skipped contents (`content-visibility: hidden`, a closed
  `details`); (2) its computed `visibility` is `visible`; (3) its document is
  shown: the window is not hidden, and a frame document lies in its parent's
  window (below); and (4) with painted frames (`Page::set_paints_frames(true)`,
  the GUI), its box, after the transforms and the scroll offsets of the walk,
  meets the viewport grown by one viewport on each side (`VIDEO_MARGIN`).
  Headless pages have no viewport rule: a full-page screenshot draws the
  videos outside the viewport; they keep the 7a rule plus (1) and (2), which
  a screenshot shows too. The flag changes only the conversion: decoding,
  the clock, the events and what scripts read do not change. It is computed
  after a layout and after a display list or scroll (the key is the layout
  serial, the list serial, the viewport scroll offset, the viewport size and
  the inputs of the page), so a page without a change does not walk; a page
  without media elements does not look at all. A video that is painted again
  shows its frame: a paused one through the quiet seek of 7a; a playing one
  with the next frames that the video thread converts.
- **The margin, measured.** After the flag turns on while the video plays,
  the frames in the queue were decoded without pixels (at most 6: 240 ms at 25
  fps; in a test the first converted frame was current 241 ms after the
  flag, at the 6th render of 41 ms). A video that becomes visible within that
  time has no current frame; the last converted frame shows in its place (a
  hole is worse than an old picture: `Fetch::frame_image` falls back to the
  last converted frame). A margin of one viewport makes the flag turn on a
  viewport before the video is visible: for a window of 720 px that is a
  scroll of 3,000 px/s for 240 ms, and a page-down (the flag is on before the
  jump). The test `a_video_that_scrolls_in_shows_its_frame_when_it_comes_into_view`
  scrolls 60 px per render at 25 renders per second (1,500 px/s; 600 px
  window, video 1,800 px down): the video is visible at render 20 and its
  current frame (3 or more frames converted after the flag) shows in render
  20: "in the first frame where it is visible". With a margin of 0 the frame
  comes at render 27, 7 renders (280 ms) after the video is visible, and the
  test fails (checked). A jump of more than a viewport (`End`, the
  scrollbar) shows the old frame for up to 240 ms.
- **Frame documents.** The page tells each frame document whether its parent
  shows it (`Page::update_frames_shown`, at each render of a page with
  frames, parents first): the frame has a box that is painted and, with the
  viewport rule, within the margin of the parent's viewport, and the parent
  is shown itself. The videos of a frame that is not shown convert no
  frames; the frame's own viewport and scroll decide for the others. Only the
  frames that lead to a video are looked at (one walk of the parent's boxes
  each), and only when the key of the look changed (fix round 1, below).
- **The hidden window** (`Page::set_window_hidden`; the GUI calls it from
  `about_to_wait`). Rule (`window_hidden` in `gui/mod.rs`): the window has no
  pixels (`Resized` to 0 in a width or height; minimized on Windows), or
  `WindowEvent::Occluded(true)`, or `Window::is_minimized() == Some(true)`
  (read after `Resized`, `Occluded` and `Focused`). Per platform, from the
  documentation and the X11 and Wayland backends of winit 0.30.13: **X11**:
  `Occluded` comes from the visibility events (fully obscured; measured on
  Xvfb by mapping an opaque window over the swb window: no frame is converted
  or drawn), `is_minimized` reads `_NET_WM_STATE_HIDDEN` (the window manager's
  state is not testable on Xvfb, which has none); **Wayland**: `Occluded` is
  not sent and `is_minimized` is always `None`; the compositor stops the
  frame callbacks of a hidden surface, so no frame is drawn (ADR 0040
  section 3), but the page cannot tell and keeps converting; **Windows**: no
  `Occluded`, a minimized window has the size 0; **macOS and iOS**:
  `Occluded` and `is_minimized`. While the window is hidden no video
  converts; the clocks and events go on (test
  `a_hidden_window_converts_no_frames`: the position moves on); the frames
  come back when it is shown again, a paused video through the quiet seek
  (`a_paused_video_shows_its_frame_after_the_window_was_hidden`). The size
  comes from the last `Resized` event: reading it at each wake-up costs an X
  request (190 against 310 main-thread context switches per second while
  playing).
- **Damage.** A new frame repaints only the video: the generation of the
  image gives the damage of its box (the display list stays), and the region
  raster draws it (ADR 0044). Measured in the GUI (the `frame` lines of the
  frame log, 640 x 360 box at 100, 100 in a 1280 x 720 window): the 1080p file
  draws 232,404 px per frame, the 642 x 362 of the box, no full raster, 1
  rectangle; the 320 x 240 file in the same box draws 174,484 px (482 x 362:
  `object-fit: contain` puts the 4:3 picture in the middle of the 16:9
  box). The changed rows are the rows of the damage rectangles, in the
  window's whole width (`Composer::note_changed`); the toolbar is not drawn
  again. A video that fills the window damages more than half of the
  viewport, so each frame is a full raster of the page (870,400 px, the page
  area; the table below). Rows that are as wide as the window are a
  gap (the buffer write and the present damage of a narrow video cover the
  window's width): backlog.
- **Measured in a window** (release build with media, a private Xvfb, window
  1280 x 720 (viewport 1280 x 680 under the toolbar), `RUST_LOG=swb::gui=debug`;
  each file in a page of its own, `muted autoplay loop`, 4 s to settle, then
  10 s, one run each, one machine; the tool is `measure.py` of the 7b notes
  file, not in the repository). Columns: frames drawn per second (`frame` lines
  of the frame log) against the file's rate; dropped frames (the pipeline's
  counters, the new `media` line of the frame log); CPU of the whole process
  (`/proc/PID/stat` before and after); resident memory; the median damage of a
  frame in device pixels; wake-ups of the event loop per second (the `wakeups`
  of the `media` line: one per `new_events`). The video fills the window
  (`object-fit: contain`) except in the two "box" rows (640 x 360).

  This is the first run, before the fix round (below): its wake-up column
  is the one of the first build; the re-measured rows follow the table.

  | File and state | drawn/s (file) | dropped | CPU % | RSS MiB | damage px/frame | wake-ups/s |
  |---|---|---|---|---|---|---|
  | 1080p, 30 fps, playing | 30.0 (30) | 0 | 41.6 | 240 | 870,400 (full) | 95 |
  | paused (10 s) | 0 | 0 | 0.0 | 232 | 0 | 0 |
  | ended | 0 | 0 | 0.0 | 223 | 0 | 0 |
  | playing, below the viewport | 0 | 0 | 12.7 | 174 | 0 | 64 |
  | playing, window covered (`Occluded`) | 0 | 0 | 13.2 | 232 | 0 | 64 |
  | Wikimedia, 2558 x 1626, 25 fps, playing | 25.0 (25) | 0 | 49.9 | 317 | 870,400 (full) | 79 |
  | paused / ended | 0 | 0 | 0.0 / 0.0 | 296 / 302 | 0 | 0 / 0 |
  | playing, below the viewport | 0 | 0 | 5.8 | 161 | 0 | 55 |
  | `mux-highest.mp4`, 4K, 24.02 fps, playing | 24.0 (24.02) | 0 | 109.7 | 505 | 870,400 (full) | 76 |
  | paused / ended | 0 | 0 | 0.0 / 0.0 | 458 / 507 | 0 | 0 / 0 |
  | playing, below the viewport | 0 | 0 | 51.8 | 331 | 0 | 52 |
  | 320 x 240, 25 fps, playing | 25.0 (25) | 0 | 17.4 | 97 | 870,400 (full) | 56 |
  | paused / ended | 0 | 0 | 0.0 / 0.0 | 99 / 101 | 0 | 0 / 0 |
  | playing, below the viewport | 0 | 0 | 1.0 | 95 | 0 | 55 |
  | 1080p in a 640 x 360 box | 30.0 (30) | 0 | 33.5 | 248 | 232,404 (1 rect) | 96 |
  | 320 x 240 in a 640 x 360 box | 25.0 (25) | 0 | 6.2 | 101 | 174,484 (1 rect) | 61 |

  Before the change (the same build without `frame_due`, the viewport rule
  and the window rule): the three playing files drew the same frames, and the
  three below the viewport drew 30, 25 and 24 frames per second of nothing
  (damage 0) and converted every frame, with 14.9, 14.0 and 58.4 % CPU, 242,
  318 and 495 MiB, and 185, 77 and 326 wake-ups per second. The 4K file
  (`mux-highest.mp4`) is a 24.02 fps file (`avg_frame_rate` 77490000/3225877,
  861 frames in 35.8 s; the 47.95 of `r_frame_rate` is the stream's time base,
  not its rate): swb draws 24.0 frames a second and drops none, so it plays
  it in real time at 110 % CPU, like Chromium (240 frames in 10 s, 93.2 %).
  The first draft of these notes took 47.95 for the rate and called the file
  decode-bound; there is no gap (found in the review). Neither an underrun
  nor `waiting` was measured at 4K.
  **Against the budgets of the diagnosis (section 6):** 1080p: 300 frames in
  10 s, 0 dropped, 41.6 % CPU (budget 150 %, Chromium 24.9 %), 240 MiB
  (budget 600 MiB above the idle page); Wikimedia: 0 dropped, 49.9 % (150 %;
  Chromium 21.6 %), 317 MiB; 4K: all 24.0 frames a second, 0 dropped,
  109.7 % CPU (Chromium 93.2 %), 505 MiB (1.5 GiB); paused, ended: 0.0 %
  (budget 0.0 %) and no wake-up. Gap: the CPU of 1.7 and 2.3 times
  Chromium's for 1080p and Wikimedia (the full-window raster of each frame
  takes 8.5 and 13.5 ms of it). Not measured: a minimized window (Xvfb has no
  window manager; the covered window is the same rule through `Occluded`,
  which Xvfb reports).
- **The wake-ups (fix round 1 of 7b).** The first run had 95 wake-ups a
  second for a painted 30 fps video and 55 to 65 for a playing video that
  nobody sees. The cause is not "the redraw and a notify": `end_update` armed
  the owner's notify whenever `pending()` was true, and while playing
  `pending()` is true when less than `READY_AHEAD_US` (200 ms) is decoded
  ahead. The queue holds 6 frames (200 to 240 ms), so each frame that the
  owner takes makes it "not ready", and the video thread's next push woke the
  loop although the owner's next due time (the queued frame) had not changed.
  Per frame: the due time and two such notifies (a traced run: 204 timer and
  360 notify wake-ups in 6 s). Fix (`Pipeline::end_update`): no notify while
  the pipeline plays with a running clock, no seek, and a queued frame of
  the current epoch (then that frame is a due time). The review of the
  fixes found that the first version (playing and a queued frame) lost the
  end of a seek while playing: the clock stops during a seek, the frame at
  the target can be queued while the seek still waits for its audio, and
  only the notify tells a real-time owner that the seek can end (the
  late-audio file with a 4 KiB packet bound: no `SeekDone` in 12 of 12
  seeks; test `a_seek_while_playing_ends_for_an_owner_that_waits_for_the_notify`).
  An owner that
  waits for the pipeline (virtual time: `Media::pending`, `settle_media`,
  `run_until`) calls `has_pending_work`, which arms the notify itself, so
  virtual time is unaffected: the media suites of the engine
  (`media_playback`, `media_states`, `media_video_paint`, `media_frames`,
  `media_element`, `media_frame_loop`) pass with the same times (3 runs each,
  within 0.1 s of the times before). Measured (the same machine and window;
  one 10 s run each; "before" is the build of the first run):

  | Case | before: wake-ups/s | after item 2 | after item 3 | drawn/s | dropped | CPU % before / after |
  |---|---|---|---|---|---|---|
  | 1080p painted, full window | 95.4 | 64.5 | 64.6 | 30.0 | 0 | 42.2 / 42.9 |
  | 1080p painted, 640 x 360 box | 95.5 | 64.8 | 64.1 | 30.0 | 0 | 33.7 / 34.2 |
  | 320 x 240 painted, 640 x 360 box | 57.3 | 54.0 | 54.9 | 25.0 | 0 | 6.7 / 6.7 |
  | 1080p playing below the viewport | 65.5 | 34.9 | 11.6 | 0 | 0 | 12.8 / 13.3 |
  | 320 x 240 playing below the viewport | 60.2 | 30.9 | 13.4 | 0 | 0 | 1.0 / 0.7 |
  | 1080p paused | 0 | 0 | 0 | 0 | 0 | 0.0 / 0.0 |
  | 4K playing, full window | 75.6 | 52.0 | 51.8 | 23.9 | 0 | 115.4 / 111.6 |

  A painted video still wakes twice per frame (64 for 30 fps; 54 for 25 fps
  with the `timeupdate` points): the due time and, it seems, the redraw turn
  that winit delivers after `request_redraw` (not traced). A video that
  nobody sees woke once per frame after item 2 (the due time).
- **No frame times for a video that nobody sees (item 3 of the fix round).**
  In real time, the page's next media due time of a pipeline that is not
  painted leaves out its frame times (`Pipeline::next_wake`; the `timeupdate`
  points, the end and the suspend stay): it is the frame in the middle of the
  queue, so the owner takes the 6 queued frames in two or three batches.
  Never the last frame: with the queue taken to its end the video thread
  starts again from an empty queue, and an underrun is near (`covered` is
  the end of the current frame when the queue is empty). The frames that a
  batch passes over count as shown, not as dropped (an unpainted video drops
  nothing that anyone sees; `Pipeline::quality`). Virtual time keeps
  `next_due` (the first frame): its jumps and results do not depend on the
  paint (the page passes `coarse = !clock.is_virtual()`). A video that
  becomes painted has the exact frame time again at once (the owner reads the
  due time after each render), and shows the frame at its position as
  before; the scroll-in test is unchanged. Measured above: a playing video
  below the viewport wakes 11.6 and 13.4 times a second (one batch in three
  frames, 4 `timeupdate` points a second), 0 dropped, CPU unchanged.
- **The look at the frame containers is keyed.** `Page::update_frames_shown`
  and `DocumentState::element_in_media_window` walked the parent's fragment
  tree once per frame document that leads to a video at each render. The
  page keeps the key of the last look (for each frame: its parent, container,
  box, whether it has media, whether it was told "shown", whether its parent
  is shown, and the parent's layout serial, list serial, scroll offset,
  viewport and rule; and the window state), computed again after the look. A
  render without a change walks nothing (test
  `renders_without_a_change_walk_no_fragment_tree`: 25 renders of a playing
  video in a frame, 0 walks; unkeyed, 25 walks). The walk closure that
  `update_media_visibility` and `element_in_media_window` shared is one
  function (`walk_boxes_in_window`).
- **Chromium, one row** (`tools/probes/media-frame-loop.json`, case
  `autoplay-muted-scrolled-out`, three equal runs): a muted `autoplay` video
  that starts out of the viewport does not play (`paused` true, no `play`
  event) until it is scrolled into view; scrolled out again it pauses (a
  `pause` event) and plays again when scrolled back (`play`, `playing`).
  swb does none of it: its autoplay video plays wherever it is and keeps
  its state (backlog; nothing implemented).
- **Tests.** `crates/engine/tests/media_frame_loop.rs` (13, real time, the
  GUI's loop without a window), `frame_oracle.rs`
  `a_playing_video_asks_for_frames_when_its_pixels_change`,
  `crates/media/tests/pipeline.rs` (`frame_due_*`, the seek, and from fix
  round 1 `a_playing_pipeline_with_a_queued_frame_is_not_notified_at_each_frame`
  and `a_video_that_is_not_painted_wakes_its_owner_in_batches`), unit tests in
  `gui/mod.rs` (the hidden window, `wake_deadline`) and `framelog.rs`, and
  the hostile case `media-1000-playing-videos-frame-loop` (`swb --headless
  --js --frame-loop-ms 6000`: 1,000 playing 320 x 240 videos, the pump, the
  frame decision, the damage render and a scroll every 100 ms; slowest
  decision 0.02 ms, 308 MiB). Mutations that make a test fail (checked): no
  `media_frame_due` in `frame_outdated` (`the_frame_decision_sees_a_due_frame_
  without_a_poll` and the oracle), a margin of 0, no `visibility` test, skipped
  contents counted (`walk_painted_with_hidden`), no viewport rule, a window
  that is never hidden, frame documents always shown, no fallback to the last
  converted frame (`a_hidden_window_converts_no_frames`), the old `media_now`
  at a seek. Fix round 1, mutations that make a test fail (checked): the
  notify armed at each update (105 notifies for 50 frames against a bound of
  16; `a_playing_pipeline_with_a_queued_frame_...`), the frame look without
  the key (60 walks against 35: `renders_without_a_change_walk_no_fragment_tree`),
  the first queued frame as the wake-up of an unpainted video (`..._wakes_its_owner_in_batches`
  and 29 wake-ups against a bound of 18 in
  `a_video_outside_the_viewport_converts_no_frames`), the frames passed over
  counted as dropped (`..._wakes_its_owner_in_batches`).
- **Open (backlog).** A painted video wakes the loop twice per frame (the
  due time and, it seems, the redraw turn of winit; not traced). A video
  that nobody sees wakes 12 to 13 times a second at 25 to 30 fps (one batch of
  three frames and the `timeupdate` points): a longer queue for unpainted
  pipelines, or one wake-up per `timeupdate`, would go lower, but the queue
  is 6 frames and the owner must take it before it runs dry. The first converted frame of a video that
  becomes painted while it plays comes after the 6 queued frames (240 ms at
  25 fps; the old frame shows meanwhile). A clipped-away, covered or
  `opacity: 0` video counts as painted. The changed rows of the window are
  whole rows. The state of a minimized window is untested on a real window
  manager.

## Implementation notes (M14 feature 8)

Audio output through PipeWire: the C boundary, the mixer, the audio clock,
the refill without the page thread, the fallback, and the private daemon
that makes the measurements safe. Code: `crates/media-ffi/csrc/pipewire.{c,h}`,
`src/pipewire.rs`, `src/counting.rs`; `crates/media/src/output/` (`mix.rs`,
`control.rs`, `source.rs`, `fake.rs`, `pw.rs`); `pipeline/mod.rs`,
`pipeline/shared.rs`; `crates/engine/src/media/backend.rs`;
`crates/swb/src/gui/`; `tools/swbtools/pw_private.py`.

- **The C boundary** has three functions (`swb_audio_open`,
  `swb_audio_set_active`, `swb_audio_close`; section 4 said about four) and
  two callbacks: the fill callback on PipeWire's real-time thread (the buffer
  and the delay, in frames at 48 kHz, from `pw_stream_get_time_n`; 0 when
  PipeWire gives none) and the state callback on the loop thread. The open
  does not wait for `STREAMING` (the spike's did): it makes the stream,
  connects it and starts the thread loop (1 to 4 ms), and a connect without a
  server fails at once (`-EHOSTDOWN`, 1 ms). `csrc/pipewire.c` is C11 with
  `_GNU_SOURCE` (the SPA headers use `locale_t` and `ssize_t`) and compiles
  with `-Wall -Wextra -Werror` (also with `-fsanitize=address,undefined` in
  `just media-asan`, which now builds the feature `pipewire` too). The build
  script finds libpipewire with `pkg-config` (`>= 0.3.50`; the new build
  dependency `pkg-config` was already in the tree), compiles the C file, and
  prints the link flags after its archive. The Rust type `AudioStream` owns the
  handle, is `Send`, and has `set_active` and `Drop`; both callbacks catch
  panics (the fill then writes silence); each callback borrows only its own
  field of the callback state (leaked with `Box::into_raw` and freed with
  `Box::from_raw` after `swb_audio_close`, so the pointer that the C side keeps
  does not come from a moved box), so the two threads never hold references
  that overlap. `swb --version` prints the headers' version. The tests of the
  boundary against a real server are `#[ignore]`d
  (`crates/media-ffi/tests/pipewire.rs`) and refuse to run outside a
  `pw-private` runtime directory.
- **The mixer** (`output/mix.rs`) is a table of 32 slots; a slot has an atomic
  state (`FREE`, `ACTIVE`, `FADING`, `FADED`), an attachment number, the
  frames mixed for the source (`taken`), a starved flag, and
  `Mutex<Option<SlotData>>` (the element's ring, the ramp's gain, the silence
  to play first). **The protocol, and why the callback never frees or blocks:**
  1. The callback visits a slot only in the state `ACTIVE` or `FADING` (an
     acquire load) and takes its mutex with `try_lock`; when the lock is
     held it skips the slot for this cycle. It borrows the ring from the
     guard and never clones or drops the `Arc`. **The state that decides is
     read again under the lock** (the first load only says that the slot may
     be mixed): a callback that stalled between the load and the lock (the
     owner can free the slot and attach another source meanwhile) would
     otherwise apply the `FADING` of the old source to the new one, store
     `FADED` over its `ACTIVE`, and leave it silent in a slot that leaks.
     Proof that the state read under the lock agrees with the data: the owner
     changes the data only under the lock; it sets `FADED` before it takes the
     data out (so a callback that holds the lock and reads `ACTIVE` or
     `FADING` sees data that still belongs to that state), and sets `ACTIVE`
     (release) only after it put the new data in and left the lock; the only
     transition after the callback's read is `ACTIVE` to `FADING` (the new
     source's own fade), which the callback then applies to the right data.
     The unit test `a_stalled_callback_does_not_fade_the_source_that_took_the_slot`
     runs the interleaving with the stale first look as a parameter.
  2. The owner takes a slot's lock only with `try_lock` in a loop with
     `yield_now`, never with `lock()`. So the mutex never has a waiter, and
     its unlock in the callback is a store, not a futex wake (a system call).
  3. The owner puts the data into a `FREE` slot under the lock and then sets
     `ACTIVE` with a release store. It ends a source in two steps: `FADING`
     (the callback ramps the gain to 0 in 480 frames, then sets `FADED`), then
     it takes the data out under the lock, sets `FREE` and drops the data
     after the unlock, on its own thread. When the owner holds the lock the
     callback holds no reference to the data, and it cannot get one without
     the lock, so the last `Arc` of a ring is never dropped on the real-time
     thread. The owner frees a fading source that no callback fades (the
     device is stuck) after 100 ms.
  4. The mix is `sample × gain` summed per slot in chunks of 1,024 frames in
     one 8 KiB scratch buffer on the stack (zeroed once per cycle, not once
     per slot), then clamped to [-1, 1] (NaN becomes 0). The gain moves
     toward the ring's gain (`volume`, 0 when muted) by 1/480 per frame: a
     change takes at most 10 ms. A source starts at gain 0 (a fade-in).
  5. Per cycle the callback publishes, under a sequence counter, the time of
     the callback, the device's delay and the frames of the cycle, and per
     slot the frames mixed (`taken`: the frames of the ring and the frames of
     silence that align the source, not the frames of an empty ring) and
     whether the ring was empty; the owner reads one consistent snapshot
     (`Mix::reading`).
  Tests: `crates/media/tests/mix_realtime.rs` runs the callback on a thread
  with a counting global allocator (`swb-media-ffi`'s feature
  `counting-allocator`, for dev-dependencies: a `GlobalAlloc` needs `unsafe`,
  which only that crate has) while the owner adds, changes and fades 32
  sources and a producer fills the rings: 0 allocator calls on the callback
  thread. The unit tests of `mix.rs` run add and remove under a thread that
  loops the callback, and hold a slot's lock to show the skip.
- **The output** (`output/control.rs`): `Output` has the states `Closed`,
  `Connecting`, `Running`, `Parked`, `Waking` and `Failed`. `request()` (an
  element plays with audio) opens the device when there is none, or starts a
  parked one; it returns at once. A device that reports `Streaming` is
  `Running`; elements join it then (`attach`). While it is `Connecting` the
  elements play on the silent output, so a missing link or device freezes
  nothing. `Connecting` and `Waking` end with an error, or after 2 s without
  `Streaming` (no device or no link), in `Failed`: the device closes, every
  source is gone, one `warn!` for the process, and `request(…, retry)` tries
  again only for a `play()` 30 s after the failure (the steps of an element that
  already plays do not retry). A `Running` device without a mixed source (and
  not asked for in the last 100 ms, so an element that is about to join does
  not find it stopped, and in which no source left in the last 500 ms, so a
  loop's wrap or a seek, which drops the source and attaches a new one
  within moments, does not stop the stream and start it again) is stopped with
  `pw_stream_set_active(false)`: no callback runs, the process uses no CPU; it
  closes after 30 s. (A pause therefore costs 0.5 s of callbacks.) A stream that
  goes to `Paused` while `Running` (its link went away) is `Waking`: the
  elements leave the mixer until it streams or the 2 s end. A timer thread
  (`swb-audio-timer`, started at the first request) ticks at the deadlines
  (the 2 s, the 100 ms of a fade, the 30 s) and when the device reports a
  state, and calls the wakers that the elements registered (the page's
  wake-up), so no element polls. The state callback of the device only stores
  an atomic and wakes the timer thread: nothing in it takes the control lock,
  so the control lock can be held while the device is called (`set_active`, the
  drop). Time comes from an injected `MonoClock`; `Output::manual` has no
  thread and a test calls `tick`. `output/fake.rs` is the fake device: a test
  pulls N frames with a delay it gives and reports the stream's states.
- **The audio clock** (`output/source.rs`): `Source::clock_us()` is `taken`
  (the frames mixed for the element) minus the delay, over 48,000. **The
  delay used is at least one cycle**: the frames of a cycle are handed over
  at its callback and play during the cycle after it at the earliest, and a
  null sink reports 0 (measured: a card reports 512 frames, a null sink 0).
  **Interpolation:** between callbacks (every 512 frames, 10.7 ms) the clock
  adds the time since the last callback (read from `MonoClock`), up to the
  delay, and never more than `taken`; while the ring was empty in the last cycle
  it adds nothing. **Never back:** the value returned is the largest so far.
  The rules:
  - *Join and leave.* An element is in the mixer exactly while its pipeline's
    clock runs (playing, no seek, no underrun, not ended), its ring exists,
    the device streams and a slot is free. It joins at `play()` or at the
    first step after the clock runs, and leaves at a pause, a seek, an
    underrun, the end, a suspend, the end of its audio track, a failed device,
    and when the page takes the output away. A source that leaves fades out;
    its slot is freed by the next tick. A page that goes away drops its
    elements, and so their sources: no slot leaks.
  - *Alignment.* A source joins with `lead` frames: the media time of the
    ring's next frame (from `SampleRing::base`) minus the element's media
    time. A positive lead is silence that the mixer plays first, a negative
    lead frames that it skips, so the sound is at the clock's time after a
    pause, a seek and a switch from the silent output (which keeps the ring at
    the media time).
  - *Switches never jump.* `MediaClock::rebase` keeps the media time and
    moves the anchor to the other source's clock time: page clock to audio
    clock at the join, back at the leave (the pipeline passes `Report`s and
    asks `time_at` as before; `Fetch` converts the page time to the clock's
    on the way in and the due times back on the way out). After a device
    failure `Source::clock_us` goes on in real time from the last reading at
    the failure, so the element loses no time before its next step.
  - *Seek.* The element leaves at the seek (the ring is discarded), and joins
    when the seek ends and the clock runs. The old sound is cut without a
    fade (measured: the largest sample step at a seek is 3.9 times the
    sine's own; the pause and the volume changes have no step above the
    sine's).
  - *Underrun.* With the audio clock a ring that the mixer emptied is the
    underrun: the mixer plays silence for the element and counts no frame, so
    its clock stands still; the pipeline sees the empty ring
    (`Pipeline::audio_starved`) and reports `Underrun` at the clock's media
    time (also when the owner looks late), the element leaves the mixer, and
    on `Resumed` (250 ms decoded) it joins again with a lead.
  - *End of the audio before the video.* When the audio track is decoded to
    its end and the ring is empty (`Pipeline::audio_finished`) the element
    leaves the mixer and the clock goes on from there on the page (monotonic)
    clock, so the video and the end do not wait for a clock that stopped.
  - *Without audio, or without an output,* the clock is the page clock. A
    muted element (gain 0) stays in the mixer; its clock stays the audio
    clock.
  - *The wake-ups.* `Fetch::next_due` converts the pipeline's due time (audio
    clock time) to page time with the two clocks now. A source whose clock
    does not move (no callbacks) is polled again after the time that is left,
    so it does not spin.
- **The refill without the page thread.** `AudioOutput::External` (the
  pipeline does not consume the ring; the mixer does) used to need the
  owner's `update` or `has_pending_work` to wake the demux thread when the
  ring had room. Now `Pipeline::set_output(External)` sets a flag, and the
  demux thread that waits for room in the ring (or for room in the video
  packets while audio waits) uses a timed wait: from 20 ms to 500 ms, so that
  it wakes when the ring has fallen to about twice the level at which the
  2 s bound of the video packets lifts (250 ms). No condition variable or
  lock is in the real-time callback. Measured: 2.0 wake-ups a second of the
  demux thread for one audio element (3 s, release build); a page thread that
  a script holds for 2 s leaves the audio unbroken (no quiet run of 4 ms
  in the recording, the 440 Hz level unchanged, `currentTime` 4.0 s at the end
  of the 2 s, no `waiting`).
- **The engine and the GUI.** `Page::set_audio_output(Option<Arc<Output>>)`
  (feature `media`) gives the elements of a page their output. `Fetch`
  (`backend.rs`) holds the element's `Source` while it is mixed and passes
  the pipeline the audio clock's time instead of the page's; the changes of
  state are in `sync_audio` (at each poll, `play` and `pause`); a poll of an
  element that is not mixed looks at the output's phase first and does
  nothing when nothing can change (the output has failed and no `play()` asks
  for a new try; the table is full: the 33rd element), instead of `request`
  (a lock, a tick, a wake-up of the timer thread) and two moves of the
  pipeline between the outputs at every poll. The GUI turns
  the output on at the start (`Output::pipewire()`); `swb --no-audio-output`
  keeps it on the silent output, and the window with `--remote-port`, headless
  mode and `--bench` never turn it on (a unit test of `Args::audio_output`).
  The frame log has a new line `sync at=T video=S media=S audio=A` after each
  presented frame of a playing, painted video: the presentation time of the
  frame drawn and the element's media time (works item 4).
- **The private daemon** (`swbtools pw-private`, `tools/swbtools/pw_private.py`):
  a PipeWire daemon of its own with a runtime directory under `tmp/`, a
  configuration file of its own, no session manager and no device, one null
  sink (stereo, 48 kHz, with a monitor), for every run that plays sound. Its
  clients get the same two environment variables and a client configuration
  that makes their streams create ports (`adapter.auto-port-config`: without a
  session manager nothing does, and the stream stays at `Paused`). `start`
  proves the isolation: (a) with the daemon not running, `pw-cli info 0`
  under the same environment fails (`failed to connect: Host is down`: the
  client does not fall back to the owner's daemon); (b) `pw-dump` of the
  daemon shows no object with an `api.alsa`, `api.bluez5` or `api.v4l2`
  property or a `device.api` of these, no device object, and exactly one
  `Audio/Sink`. `watch` links swb's `output_FL/FR` to the sink within 20 ms of
  their appearance, and the sink's monitor to the recorder; `record` writes the
  monitor to a WAV file (32-bit float). The tool refuses a directory outside
  the checkout or the owner's runtime directory, and stops the daemon by its pid
  after it checked its command line.
- **Measured** (release build, development machine, GUI on a private Xvfb,
  the private daemon; the null sink reports a delay of 0):
  - *The stream.* Open 1.2 to 4 ms; `STREAMING` 18 to 42 ms after the connect
    (the watcher's 20 ms poll is in it); restart from a stopped stream within
    the tick; 48,000 to 48,075 frames a second (cycle 512).
  - *Levels* (`mp3-long.mp3`, 440 Hz left, 660 Hz right; the recorded null
    sink monitor against the file decoded with `ffmpeg`): `volume` 1 RMS
    1.0000 of the file's, 0.5 gives 0.5000, 0 and `muted` give 0.0 (digital
    silence); 440.00003 Hz and 660.00005 Hz; the amplitude of the 440 Hz
    component 0.118714 against the file's 0.118714. Two elements at once (330
    and 550 Hz at 0.25): each component keeps 0.25000 alone and together; two
    at 0.8 sum to 1.0 and clamp (56,544 samples at 1.0, none above). Pause and
    play: the silence lasts 1.003 s for a pause of 1.0 s, and the largest
    sample step is 0.93 and 0.97 of the sine's own (no click); volume changes
    0.93 and 0.46; a seek 3.86 and 3.07 (the cut).
  - *The page thread blocked for 2 s* (a script loop): no quiet run of 4 ms or
    more in the recording (`mp3-long.mp3`), audible length 5.993 s against 6.0
    expected, the 440 Hz amplitude in the busy window 0.11873 against 0.11871.
  - *CPU* (`mux-audio.m4a`, AAC): playing 0.3 to 0.4 % of a core (budget 3 %).
    After a pause: 0 ticks in the 3 s from 0.5 s after the pause in 2 of 3
    runs, one tick (10 ms) of the PipeWire loop thread in the third; the next
    3 s had 0 ticks (one run of 3 had one, in a different window); no periodic
    wake-up (the budget's 0.0 % is within the tick of the measurement).
  - *Fallback.* No server (an empty runtime directory): the `warn!` comes 10 ms
    after `play()`, one line, `currentTime` goes on (6.0 s at 6.0 s, worst step
    error 0.1 ms). A daemon without links: the `warn!` at 2.002 s, the element
    on the page clock from the start (6.0 s at 6.0 s). The daemon stopped while
    playing: the `warn!` at once (3.006 s after `play()`, the daemon was
    stopped at 3.005 s), `currentTime` goes on with a worst step error of 9.4
    ms (the audio clock's own jitter before the failure), the events still come;
    a new try at the next `play()` after 30 s (33 s: the connect opens in
    0.9 ms, streams after 24 ms). The stream closes 30 s after the last element
    stopped (`pw-dump`: present at 29.2 s, gone at 30.2 s after the pause).
  - *The sync trace* (`mux-1080p30-20s.mp4`, `loop`, muted, a 1280 x 720
    window): over 180 s, 5,422 presented frames (5,403 on the audio clock, the
    first 19 before the stream joined), `media - video` from 0.07 to 16.0 ms
    (mean 9.8, 99th percentile 14.1), 0 frames over the frame period (33.3 ms),
    0 dropped. After 10 seeks of 3 s each: 1,013 frames, 0.0 to 18.6 ms, 0 over
    the period; the first frame after a seek was presented 22 to 65 ms after
    the seek, 0 to 12 ms off the media time. (The difference has the mean of
    half a frame period: the frame shown is the last at or before the media
    time.)
- **Tests** (each new test failed when its fix was reverted: the list is in the
  notes of the task). `swb-media`: `output::mix` (sum, clamp, ramp, counts, 32
  and the 33rd, add and remove under a running callback, the skip), `output::
  control` (states, timers with the injected clock, fallback, retry only at
  `play()`, wakers, the 33rd source, the drop), `output::source` (delay,
  interpolation, never back, empty ring, seek, silence, failure),
  `tests/mix_realtime.rs`, `tests/output_pipeline.rs` (a real pipeline on the
  fake device: the media time follows the pulled frames, a busy page thread, an
  underrun, the end of the audio, a seek to the start, a looping file decodes
  as many samples in its second round as in its first, a seek to 0.1 s of an
  MP3 lands at 0.1 s). `swb-engine`: `tests/media_audio_output.rs` (12, a
  thread that pulls a fake device in real time: the clock of an element is the
  device, joining and leaving, gain, error, no server, retry, busy page, 42
  elements in 3 pages with 32 mixed, slots freed, no output, the alignment
  after a pause and a resume, the time while the device does not pull). `swb`:
  the options and the frame log's line. Tools: `tools/tests/test_pw_private.py`,
  and the hostile cases `media-100-playing-audio-elements` and
  `media-play-pause-volume-toggle-loop` (silent output; headless mode never
  opens the device, so the two cases do not reach the mixer: its churn is in
  the engine tests with the fake device).
- **Review fixes (round 1).** (1) The allocator counter is per thread (a
  thread-local count), so `the_counter_sees_the_calls_of_a_tracked_thread` does
  not add to the count of the real-time test: the flaky `mix_realtime` failed
  6 of 10 runs with a busy loop per core before, 0 of 10 after. (2) The MP3 and
  ADTS loop (see "Seeking audio alone" in section 7). (3) The slot re-check
  under the lock (point 1 of the protocol above). (4) and (5) The alignment
  and the clock are tested in `swb-engine`: with the device held between two
  cycles and a manual output clock, the first sound after a pause and a resume
  is at frame `512 + max(delay, 512)` of the join (fails without the lead, and
  without the one-cycle floor of the delay on a device that reports 0), and
  `currentTime` moves at most 40 ms while the device does not pull for 400 ms
  and follows the frames at half speed (fails with the page time instead of the
  audio clock). (6) The stream runs on for 0.5 s after the last source left.
  Measured on the private daemon (`wav-pcm-s16.wav`, 2 s of sines, `loop`, 9 s,
  the null sink's monitor recorded): before, the stream stopped and started at
  every wrap (4 restarts in 9 s, "ran 2008 ms") and the longest run of exact
  zero frames at a wrap was 16.0 to 18.0 ms; after, one run of 9,471 ms and no
  restart, the run of zeros 15.9 to 16.0 ms. So the gap is not the restart (the
  restart of a parked stream took 0 ms after `set_active`): it is the leave and
  the join (the lead of one cycle plus the 10 ms fade-in). The reviewer's
  estimate of 50 to 100 ms was too high; Chromium has no gap. (7) A poll of an
  element that is not mixed does nothing when the output has failed or the
  table is full (`Output::is_full`; `OutputStats::requests` counts the
  requests: 703 in 2 s of play with a failed output before, 2 or fewer after;
  6,838 before and fewer than 150 after with the 10 elements that have no
  slot). (8) The scratch buffer is one per cycle, the NaN test checks NaN and
  both infinities after the ramp, the callback state of the PipeWire stream
  comes from `Box::into_raw` and goes through `Box::from_raw`, the comment in
  `mix.rs` says "no blocking system call" (the clock read is a vDSO call), and
  the two hostile cases say that they do not reach the mixer.
- **Open (backlog).** (1) A seek cuts the old sound without a fade (a click
  of 3.9 times the sine's step). (2) The loop is not gapless: 16 ms of silence
  at each wrap (a gapless loop would decode the start of the file before the
  end and join the two in the ring). (3) The device's delay on a real card was
  measured only by the spike (512 frames); the private daemon's null sink
  reports 0, so the clock's lag on hardware is the 512 reported. (4) The
  sink's volume and mute of the desktop apply on top of the element's gain
  (PipeWire's; nothing to do). (5) `playbackRate` other than 1, Web Audio and
  `setSinkId` are not in feature 8.

## Implementation notes (M14 feature 9)

The default controls of `<video>` and `<audio>` respond: play and pause by
the button and by a click on the picture, seek by a click and by a drag on
the timeline, mute and the volume slider, the time during playback, the
auto-hide, and the states that Chromium draws (ADR 0020 "Update:
playback"). Code: `crates/engine/src/media/controls.rs` (state, key, view,
actions), `crates/engine/src/document/media_controls.rs` (the places of the
boxes, the views), the hooks in `crates/engine/src/page/` (`input.rs`,
`events.rs`, `frame_input.rs`, `media.rs`), `crates/layout/src/media.rs`
(geometry, hit test, time labels), `crates/paint/src/media.rs` (drawing).

- **What Chromium does** (probes `tools/probes/media-controls-input.json`
  (events, 31 cases, 29 of them recorded: `in-nodur` needs a local file and
  `in-click-fullscreen-and-menu` changed the page in Chromium), `-states.json`
  (22 pictures), `-fade.json`, `-hit.json`, `-sizes.json`; three runs each,
  equal apart from `ct` within 0.04 s). (1) A
  press, a release or a click on a control (play, mute, fullscreen, menu,
  the timeline, the volume slider; enabled or not) gives the page **no**
  `pointerdown`, `mousedown`, `pointerup`, `mouseup` or `click`, at the
  element or at the document (also not in the capture phase; the menu
  too: `in-menu-first`, a click on it pauses nothing and logs nothing). A
  press on a control **moves the focus to the media element** as a press on
  the picture does (`activeElement` is the `VIDEO` or `AUDIO`; `blur` and
  `focusout` go to the old element, then `focus` and `focusin` to the media
  element, before the action; `in-focus-video`, `in-focus-audio`,
  `in-focus-sequence`: the play button, the timeline, the slider, mute and
  the menu, with a text input focused before; a press on the element that
  has the focus gives no focus event). A click on
  the picture gives all of them at the video (trusted), and then toggles
  play and pause **in the default action**: a capturing `stopPropagation` on
  the window does not stop it, a `preventDefault()` of the `click` does, a
  canceled `pointerdown` (no `mousedown`) does not. (2) With no metadata
  (`preload=none`, an error, no source) the picture does nothing, the timeline
  and the mute button do nothing, and the play button works only with a
  source and no error (it starts a load that `preload=none` held back). The
  mute button also needs an audio track. (3) Events: a click on the
  timeline of a paused element is `seeking`, `seeked`, `canplay`; of a
  playing one `pause`, `seeking`, `play`, `waiting`, `seeked`, `canplay`,
  `playing` (pause, seek, play in one task). A drag: `pause` at the press of a
  playing element, one `seeking` and `seeked` for each move (a move that
  comes before the seek ends supersedes it), `play` and `playing` at the
  release. The drag follows the pointer outside the element and the window
  (clamped to 0 and the duration). A click after the end plays from 0
  (`seeking`, `play`, `waiting`, `seeked`, `canplay`, `playing`); a click on the
  timeline after the end seeks and stays paused. (4) The thumb's center maps 6
  px inside the bar's ends (a click at x = 100 on the 288 px bar of a 320
  px video seeks to 2.83 s of 10: (x - 22) / 276). The volume slider maps
  x = 130 to 0 and 170 to 1 (thumb centers inside the 52 px track from 124).
  (5) The volume slider opens over the mute button and stays open over its
  pill (x from 106 and the button's height), **and after a click on the mute
  button or the slider (it has the focus) until the next press elsewhere**.
  The timeline's thumb shows while the pointer is in the bottom 25 px of a
  video (the whole width), over the audio timeline, while it has the focus
  (a press on it) and while it is dragged. (6) Auto-hide: the controls of a
  playing video fade out 2.5 s after the last pointer move inside it or the
  start of the playback, in about 0.7 s (the opacity of the gradient at the
  bottom: 0.98 at 15 ms, 0.92 at 50 ms, 0.80 at 120 ms, 0.43 at 320 ms, 0.16 at
  540 ms, 0 at 1 s after the start of the fade; swb: linear in 0.7 s); a
  leave fades them at once; a move fades them in within 0.1 s. A paused or
  ended video keeps them. (7) The loading arc shows between about 0.1 s and
  1.1 s after a load starts (also for `preload=none` and for an error; not
  without a source). (8) The time label is `m:ss`, or `h:mm:ss` from an
  hour, for each number ("0:00 / 1:02:05", "1:01:40 / 1:02:05", "10:10 /
  1:02:05", "59:59 / 1:02:05"); a video shows "/ duration" when the label
  ends before the mute button (250 px wide: "0:00", 280 px: "0:00 /
  0:10"); audio always. (9) The audio buttons take the pointer in boxes of
  32 px (`hit-audio-mute-clicks`, `-play-clicks`, `-hover`, `-slider-hover` in
  `-hit.json`): play x 10 to 42, mute x 225.5 to 257.5, menu x 258 to 290, all
  y 11 to 43 of the 300 x 54 px panel (the boxes do not overlap; the 48 px
  boxes of the layout are the icons' places). The hover circle shows on play
  and the menu, the slider opens over mute only; a click at the menu's x
  range does not mute. Video buttons keep their 48 px boxes. (10) A press or
  drag on the volume slider of a **muted** element sets the volume and
  unmutes it: one `volumechange` for the volume if it changed and one for
  `muted = false` (both listeners read the final `muted` and `volume`, so the
  order of the two changes cannot be seen; swb sets the volume first). A press
  at the current volume gives only the unmute event; a press at 0 on a muted
  element does nothing (still muted, no event); a drag that goes on after
  the unmute gives one event per move; the same for `<audio>`
  (`in-slider-muted-click`, `-drag`, `-same-volume`, `-volume-zero`, `-audio`).
  An unmuted element dragged to 0 stays unmuted.
- **The state** is `Controls` in each media `Element` (the part under the
  pointer, whether it is inside, the time of the last move and of the leave,
  a drag, the focus of the timeline or the volume, the fade, the time of the
  load) and `PointerState` in `Media` (the hovered element, the press in
  progress, the shaped labels, the places of the boxes). It is the engine's, as the
  rest of the element's state; layout and paint hold nothing of it.
- **The key and the view.** `Element::controls_key(now)` gathers what decides
  the picture without shaping text: playing, enabled buttons, whole seconds
  of the position and the duration, the position in quarters of a second
  (the `timeupdate` points), the hover, the thumb, the volume, the slider, the
  opacity in 64 steps, the arc in 64 steps. Equal keys give the same
  picture. A key makes a `MediaControlsView` (`swb_layout::MediaControlsView`)
  with the shaped time labels (two strings, cached by text, shaped with the
  document's fonts in `update_display_list`). Hidden controls (opacity 0, no
  arc) have one key whatever the position: a playing video with hidden
  controls causes no repaint. An element without default controls (no
  `controls` attribute with scripting on) has the key of the state before
  playback.
- **The display list** builder reads the view of an element
  (`ImageSizes::media_controls`) and draws the controls from it; with none it
  draws the state before playback, as before. A change of a key makes
  `update_display_list` build the list again (`display_list = None`); the
  layout does not run (test `a_change_of_the_controls_does_not_lay_the_page_out_again`:
  0 layouts for hover, play, 40 renders of a playing video, thumb, mute). The
  damage is the list diff, which finds the controls' items. Not done
  (backlog): splicing the new items into the old list instead of building it
  again (a page with a 10,000-box tree pays its display list time for each
  change of the controls: once a second, at the hover and at the fade's 60 Hz).
- **Timers feed the page's due times.** `Page::media_next_due` takes the
  controls' due times too (`Element::controls_next_due`): the auto-hide's
  deadline while a playing video shows its controls, the fade's 16 ms ticks
  while it runs, the arc's steps in its window. Nothing else: a paused video
  with a still pointer, and a playing one with hidden controls, return `None`
  (test `the_controls_cause_no_wake_up_when_nothing_changes`;
  `Page::media_controls_due`). `frame_outdated` compares the keys with those
  of the views that the list was built with (`media_controls_stale`). The
  loading arc turns only on a page that paints frames (the GUI): a headless
  screenshot has none, so that the oracle of the damage (which renders twice
  at different times) and the references do not depend on a clock.
- **Input** (the hooks): hover through `update_hover_route` and
  `update_hover_in_document` (also in frame documents), press in
  `scripted_down` (JavaScript on) and `press_in_document` (off), moves in
  `move_top` (a drag), release in `scripted_up` and `release_in_document`,
  leave in `mouse_leave` and `clear_hover`. A press on a control returns
  "not allowed" to the callers (no selection) and fires no pointer event; it
  moves the focus to the media element first (`controls_press_focus`: the
  rule of a press on the picture, with the focus events when scripts run).
  The places come from one walk of the fragment tree for all media boxes
  (`refresh_controls_boxes`), kept while the layout serial, the scroll
  offsets of the scroll containers and the viewport scroll stay; the pointer
  maps through the inverse of the box's transform. The click of the picture
  runs after the `click` event (`scripted_release`, `release_in_document`):
  `ui_picture_click` toggles if no listener canceled it. A repeated hover at
  the same place is no move (it does not show hidden controls).
- **The actions** use the playback API: `play` (without a promise's use: the
  promise has no listener), `pause`, `seek`, `set_volume`, `set_muted`, so
  the events, the autoplay rules and the pending promises are the script's
  (the press is a user activation: `note_activation` ran before). A press on
  the timeline pauses a playing element, seeks and starts the drag; the
  moves seek again; the release plays what played. Fullscreen and the menu
  do nothing. A stream without a finite positive duration (`Infinity`, `NaN`)
  or that is not seekable does not seek (`Element::can_seek`; Chromium was
  measured only with a duration: its `nodur.webm` carries one).
- **The drawing.** Measured at scale 1: the pause icon (two bars: x 6 to 10
  and 14 to 18 of the 24 unit grid), the hover circle (radius 18, audio 16,
  #202124 at 70 %, audio black at 5.5 %) of play, mute, fullscreen and menu
  (not under the pill), the pill (112 x 36, audio 101 x 32, same fills; a
  track of 52 px, played white at 68 %, rest white at 30 %; audio black at
  63 % and 20 %), the bars (the played part is a fraction of the whole bar:
  at the end it reaches the end), the thumbs (12 px; none at volume 1 unless
  the pointer is over the slider), the muted icon (the speaker and a slash),
  the video timeline white at 68 % once the metadata is known (30 % before;
  audio black at 63 % after, 20 % before), the loading arc (a ring of about
  31 px radius, 270 degrees, black, turning once in 1.4 s), the opacity of the
  fade (all video controls; audio does not fade), the pill under the mute icon
  and over the time text, the audio timeline shortening to 15 px before the
  pill. The audio mute button moved from 55.5 to 58.5 px from the right edge
  (re-measured; the icon spans x 235 to 248 at 300 px).
- **Departures** (listed in `crates/engine/tests/media_controls.rs` with
  their reasons as **rectangles** (icon and text boxes), not pixel lists;
  every pixel outside them is within 12 levels of Chromium's, and inside
  each rectangle the count of pixels beyond 12 levels has a **cap per state**
  (`PIXEL_CAPS`: the measured count plus 10 % and 2 pixels), so an icon that
  moves by a pixel or two, a wrong icon or a wrong digit fails though it
  stays inside its rectangle): the anti-aliasing of the time glyphs (up to
  40 levels) and of
  the icons' edges; the round ends of the bars; the thumb's edge (Chromium
  centers it 0.5 px higher); Chromium plays in real time and starts after
  several hundred ms, so its position at a picture differs from swb's
  (the time digit and the played part's end); the loading arc is not drawn in
  headless pictures; the audio thumb (Chromium draws it smaller and lighter);
  the muted icon's slash is heavier than Chromium's; the fade's curve (linear
  against Chromium's slightly eased). **Found in the review fixes** (hidden by
  the whole-rectangle departure of the time text until the caps): in the
  states with the volume slider open (`st-muted*`, `st-volume-*`) Chromium
  draws the video's time as "0:00 0:10" (no slash, and the label is shorter,
  so it ends before the pill); swb draws "0:00 / 0:10" and the pill covers
  its end. Their cap (335 pixels against 141 to 201 elsewhere) holds this; it
  is a departure to fix (backlog).
- **Tests.** `tests/media_controls.rs`: the 29 event and state cases of
  `media-controls-input.json` against Chromium's rows (the page's event log
  for clicks, drags, mute, the slider, replay, `preventDefault` and capturing
  `stopPropagation`, audio, an error, `preload=none`, the menu, the focus
  events of a press on each control; the numbers in the log strings (`ct=`,
  `v=`, `vol`) may differ by 0.26, as in the other media rows, unless a
  string has `rs=` or `ns=`), and the 22 pictures of
  `media-controls-states.json` pixel by pixel against Chromium's screenshots
  (`tests/data/media-controls/`; 12 levels outside the departure rectangles,
  caps inside); `tests/media_controls_timers.rs`: the
  auto-hide, the leave, paused and ended controls, no wake-up, no layout,
  the drags (outside the video, 10,000 moves, an element that goes, a
  source that changes), an error, a stream that cannot seek, no metadata,
  JavaScript off, a canceled click, no event for a press on a control, the
  focus after a press on a control with JavaScript off, and in real time the
  time text without a layout, a playing audio that repaints its time and
  timeline, and no extra renders with hidden controls; unit tests in `layout/src/media.rs` (the time label with
  Chromium's hour strings, `Infinity` and `NaN`; the mapping of the timeline
  and the slider; the hit test) and in `controls.rs`. Hostile cases:
  `media-controls-click-flood` (2,000 clicks on the play button),
  `media-1000-videos-with-controls-under-a-moving-pointer` (300 moves; the
  runner cannot send 10,000: the engine test does).
- **Open (backlog).** The keyboard (Space, arrows), the overflow menu,
  fullscreen (M15), captions and picture-in-picture buttons, right-to-left
  controls, device scales other than 1, the focus ring, touch; a drag that
  starts in a frame document and leaves it; a cursor over the buttons; the
  fade's exact curve; the arc's turn and exact size (only its existence and
  window were measured); a stream with `duration` `Infinity` in Chromium
  (no source of one in the test files); splicing the controls' items into the
  display list; the video time label "0:00 0:10" with the slider open; the audio time text
  of a one-hour file (layout reserves room for "0:00 / 0:00"; Chromium not
  measured for audio); `controls.rs` (about 1,200 lines) split before the
  keyboard feature.
- **Review fixes** (round 1). (1) A press on a control did not move the
  focus: it does now (`Page::controls_press_focus`, for scripts and without;
  the rule of a press on the picture, `click_focus_target`, with `blur`,
  `focusout`, `focus`, `focusin`); the probe cases `in-focus-*` and a test
  with JavaScript off. (2) The departure rectangles had no bound inside:
  each has a cap per state now (`PIXEL_CAPS`, set from the counts of the
  run; mutations that fail: the audio mute center at 55.5 px, the play icon
  for the pause icon, the muted icon for the unmuted one, the time one
  second ahead). (3) The audio mute box (48 px) overlapped the menu box by
  15.5 px and mute won a click on the menu's left part: the audio buttons
  take the pointer in 32 px boxes now, as measured (point 9 above; the
  slider's hover area ends at the mute box). (4) The case count is 26 with
  24 recorded; `in-menu-first` is recorded. (5) The dead field
  `ControlsKey.play_enabled` (and the view's `play_enabled`) is gone,
  `Doc` is `PointerState`, the 25 px thumb band `THUMB_BAND` is in layout, a
  primary release with no target reports a repaint only after a press on a
  control. (6) A test checks that a playing audio repaints its time text and
  timeline in the frame loop.
- **Review fixes** (round 2). (1) The volume slider unmutes a muted element
  (point 10 above; `controls_set_volume` sets the volume, then `set_muted(false)`
  when the value is above 0, both through the playback API). Five probe cases
  `in-slider-muted-*` in the data (29 recorded cases); `in-focus-video` has its
  natural order again (mute, then slider). Mutation (no unmute): the probe
  rows and the JavaScript-off test `the_volume_slider_unmutes_a_muted_element_with_javascript_off`
  fail. (2) Measured, not changed: Chromium 148 does not autoplay
  `<audio muted autoplay>` (also without `controls`; `paused` stays true, as does
  `<audio controls autoplay>`), and swb does not either. (3) The video time
  label "0:00 0:10" with the slider open stays a listed departure (the glyph
  rectangle's reason and the caps).
