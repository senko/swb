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
the key `ffmpeg-<runner OS and image>-<hash of tools/ffmpeg/*>` (the
version, the checksum and the configure line are in those files, so any
change makes a new key), runs `just ffmpeg` (a no-op on a cache hit) and
`just check-media`. The first job (`check`) stays as it is: the default
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
  reused, so playback does not allocate per frame;
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
none`, outside the viewport, in a hidden window) keeps decoding (the
decoder state and the clock need it) but does not convert frames. An
element removed from the document pauses (measured: `timeupdate`,
`pause`) and suspends as above. Paused, ended and suspended elements use
no CPU: no thread runs and no timer is scheduled (the diagnosis budget:
0.0 % in the 3 s after a pause).

### 7. The media clock

Each element has a `MediaClock`: media time = anchor media time + (clock
now − anchor clock time) while playing (rate 1; `playbackRate` is in the
backlog). The clock source:

- **audio**, when the element has an audio track and the PipeWire output
  runs: the frames of this element that the mixer consumed, divided by
  48,000, minus the output delay that `pw_stream_get_time_n` reports;
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
  format. Measured: 512 frames per cycle, 10.7 ms delay. It closes 30 s
  after the last element stopped.
- **The mix** runs in the stream's process callback (PipeWire's real-time
  thread), in Rust, called through the C boundary: at most 32 sources
  (elements with audio that play), each a single-producer
  single-consumer ring of samples with an atomic gain; the callback sums
  `sample × gain`, clamps to [−1, 1], and counts the frames it took from
  each source (the audio clock of section 7). The callback does not lock,
  allocate or log. Sources are added and removed through a fixed table of
  atomic slots. A 33rd playing element is mixed as silence (its clock is
  the monotonic clock).
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
  at the next `play()` after 30 s.
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
  premultiplied alpha are the same; no extra copy). The frame becomes the
  `DecodedImage` of the element; paint and raster draw it as any image.
  Measured: 0.6 ms per 1080p frame, 3.4 ms per 4K frame.
- **Colors as Chromium shows them** (section "The spike"): the matrix is
  the stream's tag as FFmpeg's codec parameters give it (from the
  container or the bitstream: `AVCOL_SPC_BT709` → BT.709; BT.470BG and
  SMPTE 170M → BT.601), else the frame's tag, else the untagged rule: an
  untagged stream is BT.601 below 720 lines and BT.709 from 720 (the
  diagnosis's three sizes; feature 7 measures where the boundary lies:
  lines, width or area). Open for feature 7: the Wikipedia VP8 file has
  BT.709 in its container while FFmpeg's VP8 decoder tags every frame
  BT.601 (the test files' VP8 is untagged and matches as BT.601 either
  way); a probe of that file's pixels decides. The range is the stream's
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
  that has data (also a seek to 0); while it is set and a poster image
  exists, the poster shows, else the current frame; without a poster
  attribute the first frame shows as soon as it is decoded; `load()`
  sets it again; `ended` keeps the last frame. With `preload=none` and no
  data, the poster or nothing (no #333 with JavaScript on and without
  controls, as now).
- **The frame loop.** `Page::frame_time()` includes, for each playing
  video that is painted, the presentation time of its next frame mapped
  to page time through the media clock. The GUI wakes at that time
  (`ControlFlow::WaitUntil`), the frame pacing of ADR 0040 applies
  (Wayland frame callbacks), and `frame_outdated()` sees the new
  generation. A paint that comes after the next frame's time shows the
  newest due frame (dropped frames are counted, never queued). A paused
  video schedules nothing.
- **Headless screenshots** draw the frame at the media time of the page
  clock; in virtual time the pipeline has settled before the screenshot
  (section 7), so screenshots are deterministic.
- **The natural size** is the stream's display size (`videoWidth`,
  `videoHeight`; the sample aspect ratio applied; 321 x 241 stays
  exact).

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
