# M14 step 1: diagnosis of media playback

The report of the diagnosis that plans the features of M14 (docs/roadmap.md,
"M14"), written on 2026-11-24. Base 19d8022 plus the tools of this step.
Chromium 148.0.7778.96 (Playwright 1.60, the `chromium-headless-shell`
build; it has H.264, AAC, VP8, VP9, AV1, Opus, Vorbis, FLAC and MP3). The
test files come from the static FFmpeg build N-66244 of the development
machine (x264, libvpx, libopus, libvorbis, libmp3lame); swb does not use it.
Each probe file ran three times in Chromium with equal output, except
`media-real-files.json` (section 5). The machine was idle (load below 1)
except while three runs went in parallel; the equal output shows that the
cases do not depend on that. Raw numbers and the commands: the notes file of
the step.

## Summary

- **Test media and real files.** `fixtures/media/` (37 files, 2.0 MB,
  generator `tools/media/generate.py`, committed) and `fixtures/js/media/`
  (local). **Mux's `highest.mp4` is not 1080p**: H.264 High level 5.2,
  **3840 x 2160**, 24 fps, 36 Mbit/s, 36 s, **161.6 MB**, BT.709. The
  Wikipedia file is 25 fps (not 24), 23.2 s, VP8 2558 x 1626, no audio. For
  works item 5 (1080p 30 fps H.264) the step made a local transcode,
  `mux-1080p30-20s.mp4` (20 s, 4.9 Mbit/s, 12.8 MB).
- **Formats.** For every format and codec of the M14 list Chromium's
  `canPlayType` answers `"probably"` with the codecs and `"maybe"` without
  (section 2). Chromium also supports AV1, HEVC is `""`, native HLS types are
  `"maybe"`. `MediaSource.isTypeSupported` needs codecs and is `false` for
  Ogg, WAV, FLAC and bare MIME types.
- **Fetches** (section 5): one open-ended `Range: bytes=0-` request, 206.
  After the first request Chromium reads big files in chunks of 1 to 32 MiB
  with open-ended ranges at offsets that are multiples of 64 KiB, and cancels
  them; a seek outside the buffered ranges opens a new `bytes=N-`.
  `preload=none` sends nothing until `play()`. A server without `Range`
  (status 200) plays, but a seek ends at time 0. **swb's loader cannot do
  this**: a `Response` is one whole buffer with a limit of 64 MiB, and the
  replay fetcher has no `Range` logic.
- **Autoplay** (section 4): Playwright's Chromium has Chrome's default
  policy (no `--autoplay-policy` flag; it passes `--mute-audio`). A muted video autoplays, an
  unmuted one needs user activation. Playwright's `evaluate` carries user
  activation, so tests of the policy must run the script at load.
- **Frames** (section 7): Chromium's pixels differ from FFmpeg's decode and
  conversion by at most 3 levels (BT.601, untagged below 720 lines, full
  range) and by at most 13 levels for BT.709 limited range (saturated
  colors). An untagged stream is BT.601 at 640 x 480 and 720 x 576 and
  BT.709 at 1280 x 720; where the boundary lies (lines, width or area) is
  open (feature 7 measures more sizes).
- **Poster** (section 8): the poster stays after the first frame has loaded,
  with every `preload` value, until `play()` or a seek.
- **Budgets** (section 6): Chromium plays the 1080p 30 fps file at 25 % of
  one core (all processes), the 4K file at 93 %, the Wikipedia file at 22 %;
  0 % when paused or ended, no dropped frames.
- **swb today** (section 10): `<video>` and `<audio>` have the reflected
  attributes only. No `play()`, no `Audio`, no `MediaSource`, no
  `MediaError`. videojs.org gives its video the HLS URL as `src` and loads
  nothing else.
- **Plan** (section 11): feature 3 or 4 needs a range-capable byte source
  (loader and replay); split feature 5 in two; the probe files are the
  tests of features 5, 6 and 8.

## 1. Test media and real files

**Test media**: see docs/testing.md ("Media: test files, real files and
probes"). All durations are 2 s (the long files 10 s). Chromium's duration
of each file (probe `format-metadata-table`):

| File | `duration` | `currentTime` at the start | Note |
|---|---|---|---|
| MP4 (H.264 + AAC), M4A, FLAC, WAV, MP3 (2 s), Ogg Vorbis, Ogg FLAC | 2 | 0 | the MP3 says 2 (the stream has 2.037 s with the encoder delay) |
| WebM VP8 + Vorbis | 2.003 | 0 | |
| WebM VP9 + Opus | 2.008 | 0 | `played` and `buffered` end at 2.01 |
| Ogg Opus | 2.0065 | 0 | |
| ADTS AAC | 2.027269 | 0 | no index: estimated |
| Ogg Vorbis, served without `Range` | 1.986757 | 0 | a different estimate |
| WebM, start time 1.5 s | 3.5 | **1.497** | `seekable` is 0-3.5 |
| MP4, start time 1.5 s | 3.496145 | **1.476009** | |
| 321 x 241 WebM (VP8 and VP9) | 2 | 0 | `videoWidth` 321, `videoHeight` 241 |

Playback starts at the start time of the stream and ends at the duration:
the timeline is the stream's, not shifted to 0.

**Real files** (`ffprobe`, local; the downloads of 2026-11-24):

| File | Size | Container, streams |
|---|---|---|
| `mux-highest.mp4` | 161,597,070 B | MP4, index first (`faststart`); H.264 High, level 5.2, 3840 x 2160, yuv420p, BT.709 tagged (limited range), 77490000/3225877 (24.02) fps, 861 frames, first frame at 0.058 s; AAC-LC 48 kHz stereo; 35.968 s; 35.9 Mbit/s. Served with `accept-ranges: bytes`, `cache-control: max-age=86400`, `access-control-allow-origin: *`. |
| `mux-audio.m4a` | 869,283 B | MP4; AAC-LC 48 kHz stereo; 35.968 s; 193 kbit/s |
| `wikimedia-web-browser-demo.webm` | 4,424,086 B | WebM; VP8 2558 x 1626, 25 fps, BT.709 tags, limited range; no audio; 23.2 s; 1.5 Mbit/s. Served with `accept-ranges: bytes`. |
| `mux-1080p30-20s.mp4` (made here) | 12,837,975 B | `ffmpeg -t 20 -i mux-highest.mp4 -vf scale=1920:1080,fps=30 -c:v libx264 -crf 23 -profile:v high -level 4.1 -c:a copy -movflags +faststart` |

The Wikimedia file: "File:Web browser demo.webm" on Commons, author
DecafPotato, CC BY-SA 4.0
(<https://creativecommons.org/licenses/by-sa/4.0>), screencast of Firefox
106.0.1 of 3 November 2022. It stays local; the committed fixtures have none
of the three files.

## 2. `canPlayType` and `isTypeSupported`

`probe media-can-play-type.json` (149 types; `video` and `audio` elements
give the same answer for every type). Notable rows, `canPlayType` /
`MediaSource.isTypeSupported`:

| Type | canPlayType | isTypeSupported |
|---|---|---|
| `video/mp4` | maybe | false |
| `video/mp4; codecs="avc1.42E01E, mp4a.40.2"` (also `avc1.4D401E`, `avc1.640028`, `avc1.64001F`, `avc1.640033`, `avc3.640028`, with and without AAC) | probably | true |
| `video/mp4; codecs="avc1.F4001F"` (level 3.1, profile 0xF4) / `"avc1"` | maybe | true / false |
| `mp4a.40.2`, `mp4a.40.5`, `mp4a.40.29` in `video/mp4` | probably | true |
| `mp4a.69`, `mp4a.6B` (MP3 in MP4) | probably | false |
| `hev1`, `hvc1`, `mp4v.20.8`, `ac-3`, `ec-3`, `vp9` (the short name in MP4), `theora` | "" | false |
| `av01.0.05M.08`, `vp09.00.10.08`, `opus`, `flac` in `video/mp4` | probably | true |
| `audio/mp4` / `audio/x-m4a` | maybe | false |
| `audio/mp4; codecs="mp4a.40.2"` | probably | true |
| `audio/m4a`, `audio/aacp`, `audio/x-aac`, `audio/adts` | "" | false |
| `audio/aac` | probably | true |
| `video/webm`, `audio/webm` | maybe | false |
| `video/webm; codecs="vp8, vorbis"`, `vp8`, `vp9`, `vp09.00.10.08`, `vp09.01.10.08`, `vp09.02.10.10`, `av01...`, `opus`, `vorbis` | probably | true |
| `video/webm; codecs="vp8.0"` | probably | false |
| `video/webm; codecs="avc1.42E01E"`, `theora` | "" | false |
| `audio/ogg; codecs=vorbis`, `opus`, `flac` (also in quotes) | probably | false |
| `video/ogg; codecs="vp8"` | probably | false |
| `video/ogg`, `audio/ogg`, `application/ogg` | maybe | false |
| `video/ogg; codecs="theora"`, `audio/ogg; codecs=speex`, `audio/opus` | "" | false |
| `audio/mpeg`, `audio/mp3`, `audio/x-mp3`, `audio/mpeg; codecs="mp3"` | probably | true for `audio/mpeg`, else false |
| `audio/mpeg3` | "" | false |
| `audio/wav`, `audio/x-wav` | maybe | false |
| `audio/wav; codecs=1` (also `"1"`) | probably | false |
| `audio/wave`, `audio/vnd.wave`, `audio/wav; codecs="pcm"` | "" | false |
| `audio/flac` | probably | false |
| `audio/x-flac`, `audio/flac; codecs=flac` | "" | false |
| `video/x-matroska`, `audio/x-matroska`, `video/x-m4v`, `video/3gpp` | maybe | false |
| `video/x-matroska; codecs="avc1.42E01E"` | probably | false |
| `video/quicktime`, `video/x-msvideo`, `video/mpeg`, `video/x-flv`, `audio/aiff`, `audio/midi`, `audio/amr`, `application/dash+xml`, `text/plain`, `image/png` | "" | false |
| `application/vnd.apple.mpegurl`, `application/x-mpegURL`, `audio/mpegurl`, `audio/x-mpegurl` | **maybe** (native HLS) | false |
| `video/mp2t` | "" | true |
| `""`, `" "`, `video`, `video/`, `/mp4`, `garbage` | "" | false |

Parsing rules (all measured): the answer is a string, never `null` or a
boolean. A type with a missing, empty or unterminated `codecs` parameter
(`video/mp4;`, `codecs=`, `codecs=""`, `codecs="`) is `maybe`. Unquoted
`codecs=avc1.42E01E` and `codecs=avc1.42E01E, mp4a.40.2` work; single quotes
give `""`. Spaces around `;`, before or after the whole type and between
the codecs are ignored. The MIME type is case-insensitive (`VIDEO/MP4` is
`maybe`). The codec string is case-sensitive in its prefix:
`codecs="AVC1.42E01E"` is `""`, `codecs="avc1.42e01e"` (lower-case hex)
is `probably`. One unknown codec makes the whole list `""`. An unknown
parameter (`; foo=bar`, `; extra`) is ignored when `codecs` is valid;
`video/mp4; profile=foo` is `maybe`. A NUL character gives `""`.

**Which of these swb takes.** For the build with media: `probably` for the
codecs of the build, `maybe` for the containers of the build without codecs,
`""` for everything else, including HLS (ground rules: `canPlayType` answers
`""` for HLS until native HLS exists; this departs from Chromium's `maybe`
on purpose, so that sites use hls.js). Without the feature: `""` for all
(works item 7). The `isTypeSupported` rows are for M15 (the MSE types with
codecs: `video/mp4`, `video/webm`, `audio/mp4`, `audio/webm`, `audio/mpeg`,
`audio/aac`, and `video/mp2t`, which is `true` without codecs in this
build).

`api-surface` (same file): `MediaSource`, `SourceBuffer`, `MediaError`,
`TimeRanges`, `HTMLMediaElement`, `Audio`, `MediaStream`, `AudioContext`,
`webkitAudioContext`, `VideoPlaybackQuality`, `MediaCapabilities`,
`MediaSession`, `AudioTrackList`, `VideoTrackList`, `TextTrack`, `VTTCue`,
`MediaKeys`, `RemotePlayback` exist; `ManagedMediaSource` does not;
`navigator.mediaCapabilities`, `mediaSession` and `mediaDevices` are
objects. `HTMLMediaElement.prototype` has 49 members: `src`, `srcObject`,
`currentSrc`, `crossOrigin`, `networkState`, `preload`, `buffered`, `load`,
`canPlayType`, `readyState`, `seeking`, `currentTime`, `duration`, `paused`,
`defaultPlaybackRate`, `playbackRate`, `played`, `seekable`, `ended`,
`autoplay`, `loop`, `play`, `pause`, `controls`, `controlsList`, `volume`,
`muted`, `defaultMuted`, `textTracks`, `addTextTrack`, `error`,
`captureStream`, `preservesPitch`, `disableRemotePlayback`, `remote`,
`sinkId`, `setSinkId`, `mediaKeys`, `setMediaKeys`, `loading`,
`onencrypted`, `onwaitingforkey`, the nine constants and two
`webkit...DecodedByteCount`. **`fastSeek` does not exist**; `audioTracks` and
`videoTracks` do not exist (they are behind a flag). `HTMLVideoElement` has
`width`, `height`, `videoWidth`, `videoHeight`, `poster`, `playsInline`,
`disablePictureInPicture`, `requestPictureInPicture`,
`requestVideoFrameCallback`, `cancelVideoFrameCallback`,
`getVideoPlaybackQuality`, `webkitDecodedFrameCount`,
`webkitDroppedFrameCount`, `onenterpictureinpicture`,
`onleavepictureinpicture`. `HTMLAudioElement` has no own members. Defaults:
`preload` is `"metadata"` for `<video>` and `<audio>` and `"auto"` for
`new Audio()` (which also sets the attribute: `getAttribute('preload')` is
`"auto"`), `volume` 1, `muted` false, `playbackRate` 1, `readyState` 0,
`networkState` 0, `duration` NaN, `buffered`, `seekable` and `played` empty,
`currentSrc` "", `error` null, `preservesPitch` true.

## 3. States, values and events

All from `media-states.json` and `media-playback.json` (small files fully
fetched, so `readyState` is 4 at the first `canplay`; section 9 says what
varies). A line is `event rs ns` at dispatch (rs = `readyState`, ns =
`networkState`):

- **Load with `preload=metadata`, `auto` or no attribute** (MP4, WebM, MP3,
  the same order): `loadstart` (ns 2), `progress`, `suspend`,
  `durationchange`, `resize` (video only), `loadedmetadata`, `loadeddata`,
  `canplay`, `canplaythrough`; ns 1 (IDLE) from `progress` on. The first
  events have rs 0 or 1 and the last four rs 4, depending on how much of the
  file has arrived (so a test samples only the order and the values from
  `loadeddata` on). `buffered` and `seekable` are `0-duration`.
- **`preload=none`**: `loadstart` (ns 1), `suspend`; rs 0, `duration` NaN,
  `videoWidth` 0, no request. `currentTime = 1` is stored (reads 1, no
  `seeking`) and starts no load. `play()` or `load()` start it.
- `play()` (muted or activated): `play`, then `playing`, then the promise
  resolves (all within one task: the order is `play`, `playing`, promise);
  `timeupdate` every **250 ms**, `pause`, `ended` at the end (`ended` true,
  `paused` true, `currentTime` = `duration`; `pause` comes before `ended`). `played`
  is `0-end`.
- **`play()` after `ended`**: `seeking`, `play`, `waiting`, `timeupdate`,
  `seeked`, `canplay`, `playing`, `canplaythrough` (restart at 0).
- **`loop`**: at the end `seeking`, `waiting`, `timeupdate`, `seeked`,
  `canplay`, `playing`, `canplaythrough`; no `pause`, no `ended`. Setting
  `loop` while playing near the end works.
- **Seek, paused**: `currentTime = 1` reads 1 at once and `seeking` is true;
  events `seeking`, `timeupdate`, `seeked`, `canplay`, `canplaythrough`;
  values past the end clamp to `duration`, negative to 0; `NaN`, `Infinity`,
  a string and `undefined` throw `TypeError: Failed to set the
  'currentTime' property on 'HTMLMediaElement': The provided double value is
  non-finite.` Three assignments in a row give three `seeking` and one
  `seeked`, at the last value. **Seek, playing**: `seeking`, `waiting`,
  `timeupdate`, `seeked`, `canplay`, `playing`, `canplaythrough`. A seek
  before the metadata arrived reads the new value at once with `seeking`
  true; the seek happens after `loadedmetadata`.
- **`pause()`** during `play()` before any data: the promise rejects with
  `AbortError: The play() request was interrupted by a call to pause().
  https://goo.gl/LdLk22`; events `play`, `waiting`, `timeupdate`, `pause`.
  `load()` or a new `src` rejects with `AbortError: The play() request was
  interrupted by a new load request. https://goo.gl/LdLk22`.
- **`load()` during playback**: events `abort`, `emptied`, `timeupdate`,
  `loadstart`, then the load events again; at once after `load()`: rs 0,
  ns 3 (NO_SOURCE), `paused` true, `duration` NaN, `currentTime` 0. Kept:
  `volume`, `muted`, `loop`, `preload`; reset: `currentTime`, `played`.
- `volume`: 1.5 and -0.1 throw `IndexSizeError: Failed to set the 'volume'
  property on 'HTMLMediaElement': The volume provided (1.5) is outside the
  range [0, 1].`; `NaN`, `Infinity` and `undefined` throw `TypeError: ...
  The provided double value is non-finite.`; `null` sets 0, `'0.3'` sets 0.3.
  `volumechange` fires only for a change. `muted` attribute is `defaultMuted`;
  `removeAttribute('muted')` keeps `muted` true.
- **`new Audio('a.mp3')`**: `preload` "auto", `networkState` 3 (NO_SOURCE)
  and `readyState` 0 at once (the load starts at the next task), `src`
  absolute, `getAttribute('src')` as given, `isConnected` false; then the
  load events without `resize`. A missing file: `error` with code 4.
- **Errors.** `error` events have `loadstart`, `error` (and `progress`,
  `suspend` before them when a body arrived), ns 3, rs 0; `play()` then
  rejects with `NotSupportedError: The element has no supported sources.`
  Code 4 (`MEDIA_ERR_SRC_NOT_SUPPORTED`) with the message
  `MEDIA_ELEMENT_ERROR: Format error` for a failed request (404, 500,
  connection refused) and `PipelineStatus::DEMUXER_ERROR_COULD_NOT_OPEN:
  FFmpegDemuxer: open context failed` for a body that no demuxer opens (text,
  HTML, an empty body, 4096 random bytes, an MP4 whose index was cut off);
  `MEDIA_ELEMENT_ERROR: Empty src attribute` for `src=""` (`src` reads as the
  page URL, `currentSrc` is ""). A file that opens and is damaged later
  **loads** (`readyState` 4) and fails during playback only for some damage:
  an MP4 with a corrupt middle or a cut `mdat` gives `error` code 3
  (`MEDIA_ERR_DECODE`) with a message such as `PipelineStatus::
  PIPELINE_ERROR_DECODE: video decode error!` (events `error`,
  `timeupdate`, `pause`; `ended` false); a corrupt or cut WebM and a cut
  MP3 play to `ended` without an error. A redirect (302) to the file works.
  An unsupported `type` on `<source>` raises `error` on that `<source>`
  only, with no request; when all sources fail `networkState` is 3, the
  video's `error` is `null` and `play()` rejects as above. A response is not
  checked against its `Content-Type`: a WebM served as `video/mp4`, an MP4
  served as `video/webm` or as `text/plain` play.
- **Resource selection** (`<source>`): the `src` attribute wins over children
  (children are never used when `src` is present, also when `src` fails);
  `type` is checked with `canPlayType` (`""` skips, `error` on the source, no
  request); **`media` is evaluated** (`(min-width: 5000px)` skips the source with an
  `error` event on it); a source that fails to load (404 or a file that
  cannot be opened) raises `error` on it and the next one is tried (the
  request log shows both); an empty `src` or a `<source>` without `src` is
  skipped without an event; a `<source>` added to a video that has none starts
  the selection (`preload=none` only until `play()`); `currentSrc` is the
  absolute URL (fragment kept: `a.mp4#t=1.0` starts at 1.0, `currentTime` 1).
  Changing the `src` of a loaded video: `abort`, `emptied`, `timeupdate`,
  `loadstart` and the load events; removing the `src` attribute does nothing
  (no event, the element stays loaded and `currentSrc` stays); `load()` after
  that would start the selection again. Changing the `src` of a `<source>`
  after its `error` does not retry. A `#t=1.0` fragment seeks after
  `loadedmetadata` (`seeking`, `timeupdate`, `seeked`, then `loadeddata`).
- `played` and `buffered` are `TimeRanges` (`length`, `start(i)`, `end(i)`;
  an index out of range throws `IndexSizeError: Failed to execute 'end' on
  'TimeRanges': The index provided (5) is greater than the maximum bound (1).`).
  `played` ranges merge adjacent time and keep separate ranges after a seek
  (`0-0.5,6-6.5`).
- An element removed from the document **pauses** (`timeupdate`, `pause`);
  it does not resume when inserted again. A `display: none` video plays.
- `playbackRate = 2` fires `ratechange` (not in M14).
- The size: before the metadata a video box is 300 x 150 (audio with
  controls 300 x 54, audio without controls no box); after it the box has
  the stream's size (320 x 240), the stream's ratio with a given `width` or
  `height` (`width=100` gives 100 x 75; `style="width:200px"` on 321 x 241
  gives 200 x 150.14) and the poster's size while a poster shows (200 x 100
  stays 200 x 100 after the metadata). `videoWidth` and `videoHeight` are
  exposed after `loadedmetadata` (`resize` event) and are 0 before; the odd
  size 321 x 241 is exact.

## 4. Autoplay

Playwright starts Chromium without `--autoplay-policy`, with `--mute-audio`
(audio is decoded and not output: `AudioService` runs), `--headless`,
`--disable-renderer-backgrounding`, `--disable-background-timer-throttling`,
`--no-sandbox`, `--disable-features=...MediaRouter...GlobalMediaControls...`
(the whole list: docs/testing.md "Chromium settings"). The policy is
Chrome's default, "document user activation required". Chrome's public rules
(developer.chrome.com/blog/autoplay): muted autoplay is always allowed;
autoplay with sound is allowed when the user has interacted with the document
(a click, a tap or a key press: sticky activation) or when the site has a
high Media Engagement Index (desktop) or is an installed app; a top-level
page can delegate to a frame with `allow="autoplay"`. A headless profile has
no engagement history, so only the first two rules show.

Measured (`media-autoplay.json`, script at load, no activation):

| Case | Result |
|---|---|
| `play()` of an unmuted video or audio | rejects `NotAllowedError: play() failed because the user didn't interact with the document first. https://goo.gl/xX8pDD`; paused; no event beyond the load events |
| the same with a video that has no audio track | rejects the same way |
| `play()` muted (attribute, `muted = true` or `volume = 0`) | resolves; plays |
| `new Audio(url).play()`, also muted | **rejects** (the muted rule is for `<video>`) |
| `autoplay` attribute, unmuted | does not play, no error event, no promise |
| `autoplay muted` (attribute) | plays: `play` and `playing` after `canplaythrough`; also with `preload=none`, with a `<source>` child, and when `autoplay` is set by script before the load |
| `autoplay` set by script after `canplaythrough` | stays paused |
| a video created by script and not attached, `play()` unmuted | rejects |
| a video created by script and attached, `play()` unmuted | the promise stays **pending**, the video stays paused |
| `play()` after a click (in the handler, in a later script) or after `keydown` | resolves; the probe's own `evaluate` calls also give activation, so these rows do not show which input gives it (see the note below). The HTML activation-triggering events are `keydown` (not Escape), `mousedown`, `pointerdown` (mouse), `pointerup` (not mouse) and `touchend`; `mousemove` is not one |
| `autoplay` video inserted by a click handler | plays |
| a muted playing video that a script unmutes without activation | keeps playing, unmuted (`volumechange`) |
| `muted = true` and `play()` again after a blocked `play()` | resolves |

Note: the probe tool's own `evaluate` calls (the font wait before the steps,
every `js` query) give the page sticky activation, so the rows with steps
cannot say which input gives activation, and every `js` query may `play()`
unmuted: `navigator.userActivation.hasBeenActive` is true from the first
query, `isActive` for about 5 s after each one. A swb test that compares with
these values must give `--eval` the same activation.

**Rules that swb takes.**

- Tests and headless mode: the policy above. A document has sticky and
  transient user activation, set by the input of `input.*` (mouse down/up,
  key down, touch) and by every `--eval` expression and every `eval` step
  (as Playwright's `evaluate`). Muted `<video>` autoplays; unmuted needs
  activation; `new Audio().play()` needs activation even when muted.
- GUI: the same rules, with real input as the activation; no engagement
  index; every window starts without activation (a page that autoplays with
  sound waits for the first click), which is Chrome's behavior for a first
  visit. A pending `play()` of the "attached, unmuted" row is a quirk that
  swb does not copy: it rejects with the same `NotAllowedError`.
- The flag `--autoplay-policy` is not needed; a test that wants "no
  restriction" starts the page with a click step first.

## 5. What Chromium fetches

`media-fetch.json` (10 test files; each case: wait 1.2 s, `play()` muted
0.9 s, `pause()`, seek to 60 %, seek to 0.1 s; the requests of each phase)
and `media-real-files.json`. Every request is a `GET` with `Range`.
Chromium sends `Range: bytes=0-` (open-ended)
as its first request, never a plain request.

**Test files (all below 310 KB)**: one `bytes=0-` request answered 206 with
the whole file, for every file and every `preload` value but `none`. With
`preload=none` there is no request until `play()`, and then the same single
request. A seek inside the file sends no request (all buffered). Exception:
the 300 KB `mp4-h264-aac-long.mp4`, `preload=none`: one request; with
`metadata` the log shows `bytes=0-` and then `bytes=65536-`, `131072-`
(re-opens after Chromium cancelled the first when it had enough; the
amounts vary from run to run, so that case is not in the file).
**An MP4 with its index at the end** (`moov` last) also needs only one
request (the whole file arrives; the index is found in the same body). A file
served without `Range` (status 200, no `Accept-Ranges`): one request
`bytes=0-` answered 200, the file plays, but a
seek leaves `currentTime` at 0 (14 of 14 cases): swb's own server for
tests must send `Accept-Ranges: bytes` and 206.

**Real files** (all requests `bytes=N-`; `sent` is what the server wrote
before Chromium cancelled the request):

| File, `preload` | Requests before `play()` (1.2 s) | During 0.9 s of playback | Seek to 60 % |
|---|---|---|---|
| Mux 4K MP4, `none` | none | `bytes=0-`, then `bytes=60194816-` (the end of the file: 26 MB; index probe) | 10 to 12 requests of 1 to 23 MB from `bytes=90701824-` (the target) onward |
| Mux 4K MP4, `metadata` | `bytes=0-` (14 MB) + 9 more of 3 to 6 MB at 4, 5.4, 7.5, 8.4, 9.6, 10.6, 11.9, 13.2, 14.0 MB | 1 request at 14.5 MB | 10 to 12 requests from `bytes=90701824-` |
| Mux 4K MP4, `auto` | `bytes=0-` (66 MB), later `bytes=60096512-` | none | 1 request from `bytes=90701824-` (71 MB) |
| Mux M4A, `none` | none | `bytes=0-` (the whole 0.87 MB) | none |
| Mux M4A, `metadata` / `auto` | `bytes=0-` (the whole file) | `metadata`: one more at `bytes=98304-` | none |
| Wikimedia WebM, `none` | none | `bytes=0-` (the whole 4.4 MB) | `bytes=4423680-` (406 B, the last cluster or cues) and `bytes=3342336-` |
| Wikimedia WebM, `metadata` | `bytes=0-`, `bytes=4423680-`, then `bytes=65536-` or `163840-` chunks | one more chunk | none |
| Wikimedia WebM, `auto` | `bytes=0-` (the whole file) | none | `bytes=4423680-` (406 B) |

Facts that hold in all runs: the first request is `bytes=0-`; offsets are
multiples of 64 KiB; the 4K MP4 has `duration` 35.968 and the size 3840 x
2160 after about 14 MB; `readyState` is 4 and `buffered` about 3 to 6 s at
`loadeddata` (the 4K file, `preload=metadata`), 13.4 to 13.7 s (`auto`), with
the rest buffered while it plays (`0-14.4` after 0.9 s); a seek to 60 %
(21.6 s) after playing leaves `currentTime` exact (21.58 for the 4K file at
24 fps frame times; 13.92 for 60 % of 23.2 s) with `buffered`
`0-14.4,20.04-25` (a range that starts at the key frame before the target);
the Wikimedia file's `buffered` is `0-17.7` to `0-23.2` while it plays. What varies from run to run
(so `media-real-files.json` does not have three equal runs): the chunk sizes
(1 to 32 MiB), the number of requests, the amount buffered at a given time
and which of the two small tail requests of the WebM file come.

**What the loader and replay need.** (1) A request with `Range` and a
response with 206 and `Content-Range`; the loader must send the header
(Fetch treats `Range` as a CORS-safelisted request header when it is a
single range with a start, `bytes=N-` or `bytes=N-M`; a suffix range
`bytes=-N` is not safelisted; a cross-origin media request is a no-cors
request and needs no preflight). (2) A body that is not one buffer: today `Response` holds the
whole body and `MAX_BODY_SIZE` is 64 MiB; the 4K file is 161 MB and
Chromium itself holds at most 15 s of it. The media byte source needs
incremental reads (chunks of a request that stays open) and cancellation of
a request that has enough. (3) Replay: a fixture has one entry per (URL,
method, body hash) with the whole response; a media file is recorded once
with status 200 and the full body, and replay answers a `Range` request from
it (206, `Content-Range`, `Accept-Ranges: bytes`), because the Range
pattern differs from run to run; a test must not depend on the sequence of
requests, only on the bytes. (4) The capture tool (Playwright `route`)
cannot record partial responses well; capture media with a plain `GET` of the
full file outside the page. (5) `Content-Type` is not used to choose the
demuxer (sniff the bytes, as Chromium does). A 206 makes seeks possible;
a 200 without `Accept-Ranges` does not (the 14 `no-ranges` cases above). A
200 with `Accept-Ranges: bytes` and `Content-Length` is not measured
(correction of 2026-11-25: an earlier text said that these two headers
decide; M14 feature 6a measures it). (6) Cookies, redirects (302 to the file
works), status 404 and 500 (`MEDIA_ELEMENT_ERROR: Format error`) are as for
other loads.

## 6. Budgets

`swbtools media-cpu` (CDP `SystemInfo.getProcessInfo` CPU time by process
type over 10 s, `VmRSS` of the processes, `getVideoPlaybackQuality()`),
window 1280 x 720, headless shell (software compositing and raster: "GPU" is
the SwiftShader GPU process), muted, idle machine, one run per file:

| File (decoded size) | Phase | CPU, % of one core | of which renderer / GPU | Memory (RSS of all) | Frames / dropped in 10 s |
|---|---|---|---|---|---|
| `mux-1080p30-20s.mp4` (1920 x 1080, 30 fps, 4.9 Mbit/s) | playing | **24.9** | 17.6 / 7.3 | 537 MiB | 300 / 0 |
| | paused (10 s) | 0.0 | 0 / 0 | 537 MiB | |
| | ended | 0.1 | 0.1 / 0 | 526 MiB | |
| Wikimedia WebM (2558 x 1626, 25 fps) | playing | **21.6** | 15.7 / 5.9 | 510 MiB | 250 / 0 |
| | paused | 0.0 | 0 / 0 | 510 MiB | |
| | ended | 0.1 | 0.1 / 0 | 503 MiB | |
| `mux-highest.mp4` (3840 x 2160, 24 fps, 36 Mbit/s) | playing | **93.2** | 86.5 / 6.5 | 984 MiB | 240 / 0 |
| | paused | 0.0 | 0 / 0 | 982 MiB | |
| | ended | 0.0 | 0 / 0 | 950 MiB | |

Memory of an idle Chromium page with a video: browser 90, renderer 200 to 600
(the 4K file's decoded frames and 15 s of read-ahead), GPU 126 to 173,
network 64, audio service 54 MiB. The renderer decodes with several threads
(FFmpeg's own): one process counted, up to 87 % of a core.

**Proposed budgets for swb** (works item 5; GUI, release build; the renderer
of swb is on the CPU, so the whole frame pipeline counts; machine: the
development machine, window 1280 x 720):

- 1080p, 30 fps, H.264 High, 5 Mbit/s: 600 frames in 20 s with no dropped
  frame (a frame is dropped when it is shown more than one frame period late
  or skipped), CPU at most **150 % of one core** in total (all threads;
  Chromium needs 25 %; swb decodes with FFmpeg's software decoder and
  scales and converts on the CPU; 150 % is a ceiling, six times
  Chromium's number, that feature 10 lowers to what swb reaches), resident
  memory at most **600 MiB** above the idle page.
- The Wikimedia file (2558 x 1626, VP8, 25 fps): no dropped frames, CPU at
  most **150 %**, memory at most **600 MiB** (the decoded frame is 16 MB;
  at most 6 frames in the queue, see below).
- The 4K file (3840 x 2160): plays without crashing and without unbounded
  memory (at most **1.5 GiB**); dropped frames are allowed (Chromium needs
  93 % of a core); it is not a budget item of works 5.
- Paused, ended, a hidden tab, an element with `display: none` that is
  paused: **0.0 % CPU** in the 3 s after (no frame timer, no audio
  callback, no decoder thread spinning), as Chromium (0.0 and 0.1 %).
- Playing audio only (M4A): at most 3 % of a core.
- Start: `loadedmetadata` of the 1080p file at most 300 ms after the bytes
  arrive; the first frame at most 500 ms.
- A seek in the 1080p file shows the new frame within 250 ms.
- The frame queue and the sample queue are bounded in bytes (the ADR sets
  the numbers): decoded frames at most 6; samples at most 1 s.

## 7. Frames at paused times

`media-frames.json`: seek, wait for `seeked` and a presented frame, then read
a screenshot (`pixels`). Chromium paints the **decoded frame at the paused
time** (the frame whose timestamp is at or before the time; seeking to 0.44
shows frame 11 of 25 fps). The test files' frame counter ("00:00:00.440" and
"11") identifies the frame. FFmpeg's decode of frame `n` converted with its
`scale` filter compared with Chromium's pixels (24 points on the bars and 37
points along the counter row, per time, 0 / 0.44 / 1.0 / 1.6 s):

| Stream | max difference of a channel | median |
|---|---|---|
| H.264 High, Baseline, VP8, VP9, untagged 320 x 240 (BT.601 limited) | 2 or 3 | 1 or 2 |
| H.264 BT.601 tagged | 3 | 1 or 2 |
| H.264 full range (BT.709 tagged `pc`) | 2 on the bars, 8 on mid colors | 1 |
| H.264 BT.709 tagged, limited | **12** (blue bar 239 against 252) | 10 |
| untagged 640 x 480, 720 x 576 (content in 709) | 3 as **BT.601** (as 709: 21) | 2 |
| untagged 1280 x 720 (content in 709) | 12 as **BT.709** (as 601: 23) | 10 |

So: **Chromium takes an untagged stream as BT.601 at 640 x 480 and
720 x 576 and as BT.709 at 1280 x 720**. Width and height grow together in
these three sizes, so the boundary (the number of lines, the width or the
area) is open; feature 7 adds sizes such as 1280 x 544 and 720 x 720. A
tagged stream is taken by its tag (`smpte170m` = 601,
`bt709`, range by `color_range`). The conversion matches FFmpeg's `swscale`
within 3 levels except for BT.709 limited range, where Chromium differs by
up to 13 levels on saturated colors (the exact BT.709 matrix gives 252 for
the blue bar; Chromium 239, between 709 and 601; the cause is not known:
perhaps a 8-bit intermediate or a different chroma upsampling; the full
range 709 path is exact). **Tolerance for feature 6: 3 levels for 601 and
full range; 13 for 709 limited, unless the exact conversion is found.**
Chroma edges (the bar borders) are the other source of difference (the points
on the bars avoid them).

Placement (pixels, 400 x 200 box, 320 x 240 video, page background #ccc):
the letterbox area is **transparent** (the page background shows; no black);
`object-fit: fill` interpolates the video bilinearly (a pixel on the bar edge
is 128/128/1 at x=67); `contain` and `scale-down` give the box 66.67 to
333.33 wide, snapped to whole pixels (pixel 66 is background, 67 is video);
`cover` shows the middle 400 x 300 scaled; `none` is centered and clipped;
`object-position` (`0 0`, `100% 100%`, `25% 75%`, `10px 20px`) as for images.
`padding` and `border` of the video element are painted and the video is in
the content box. The cases `frames-clip-rounded-parent`,
`frames-border-radius-on-video`, `frames-clip-path-circle`, the transforms
(rotate, scale, flip, fractional translate, skew) and the scale cases (2x,
3.5x, 0.5x, 0.37x) record the other placements; the Rust tests of feature 6
take their values from the probe output (`tools/probes/media-frames.json`
`--json`).

## 8. Poster

`media-poster.json` (flat poster 49,100,151; `pixels` of the video box):

| State | What shows |
|---|---|
| `preload=none`, `metadata`, `auto` or no attribute, loaded to `readyState` 4, never played | **the poster** (all four) |
| `play()` then `pause()` at 0.06 s | the frame |
| a seek (any `preload` with data), also to 0 | the frame |
| `preload=none`, `currentTime = 1` (no data) | the poster |
| `load()` after playing | the poster again |
| ended | the last frame |
| `autoplay muted`, playing | the frame |
| `poster` attribute removed, with data | the first frame at once |
| `poster` removed, `preload=none` / no `poster` and `preload=none` | the page background (nothing painted; no default #333 with JS on and no controls) |
| `poster` that fails to load (`missing.png`), `preload=none` | the page background |
| no `src`, `poster` set | the poster |
| `poster` with `object-fit: cover` in a 320 x 120 box | the poster covers the box |

With no poster and `preload=metadata` or `auto` the first frame shows at once.

## 9. The default controls

Screenshots at scale 1 of `media-controls.json` (`--shots-dir`; the case
names are the files; a 320 x 240 video at the top left of a 400 x 300
viewport). Geometry measured on the 320 x 240 video (all positions are
pixels from the top left of the video):

- A black gradient over the bottom 112 px. The play/pause button is centered
  at (24, 199) (a 48 x 48 hit box, the icon 20 px); the time text "0:00 /
  0:02" starts at x=48 (white, 14 px, **`m:ss / m:ss`**, floor of the seconds,
  `0:01 / 0:10` at 1.9 s; an hour format was not measured); the mute button
  at (200, 191), the fullscreen button at (248, 191), the overflow menu button
  (three dots) at (296, 191); the timeline from x=16 to x=304 at y=225 (4 px,
  the played part white, the rest white at 30 %; the thumb, a white dot of
  12 px, shows while the pointer is over the control area).
- Click on the video toggles play and pause. A click on the play button
  toggles; its hover state is a dark round background (48 px circle). A click
  on the timeline seeks to the point (x=240 gives 7.8 s of 10) and keeps the
  state. A drag on the timeline: `pause` at the mouse down, `seeking`
  events while it moves (the thumb and the time follow), `seeked` at the
  release, then `play` and `playing` (a playing video resumes). Mute click:
  the icon changes to a speaker with a slash and the slider track empties.
- Hover over the mute button: a **volume slider** (a dark rounded pill, about
  110 x 36 px at x=108 to 218) opens to the left of the button and covers
  the time text from the slash on ("0:02 0:10"); the slider thumb sits at
  the volume (right end at volume 1). It closes when the pointer leaves.
- Visibility: **paused: always visible** (also with the pointer outside);
  **playing: visible while the pointer moves inside the video and for
  about 3 s after the last move, then hidden** (a fade of about 0.3 s;
  hidden at 3.5 s; at 2 s still visible); when the pointer leaves, it fades
  at once (visible at 0.2 s, faded at 0.8 s). After the end, the controls
  stay: the play icon, "0:02 / 0:02", the full timeline and the last frame;
  a click replays from 0.
- No metadata (`preload=none`), no source, error: the controls show the
  time "0:00", the play icon (or the disabled gray icons), the mute and the
  fullscreen buttons at 30 %, on a #333 area (no poster) or the poster; a
  video with no audio track shows the mute button at 30 %. **A black arc**
  (a rotating spinner stroke, about 55 px across, near the center) appears
  for the first second or so after the load starts and also in the error
  state; it is gone after about 5 s. The state `no src` has no arc. swb's
  ADR 0020 does not draw it.
- The overflow menu (not in M14): a white panel at the bottom right with
  "Download" and "Playback speed" (more, below the crop of the test viewport).
- Audio: `<audio controls>` is the light gray panel 300 x 54 with
  rounded ends: play (a gray round hover background while playing), the
  time "0:00 / 0:02", the timeline, mute, the menu.
- Keyboard: a click on the video focuses it; Space then pauses; the arrow keys
  did not move in the one test made (not in M14).

## 10. swb today

Release build of the base commit (`target/release/swb`), `just probe
--with-swb`:

- `media-can-play-type.json`: 15 of 16 `js` rows differ (the 16th is the
  empty `HTMLAudioElement` prototype). swb: `HTMLMediaElement.prototype` has
  `autoplay`, `constructor`, `controls`, `crossOrigin`, `defaultMuted`, `loop`,
  `preload`, `src`; `HTMLVideoElement.prototype`: `height`, `playsInline`,
  `poster`, `width`; no `Audio`, `MediaSource`, `MediaError`, `TimeRanges`,
  `canPlayType`, `play`, `pause`, `currentTime`, `volume`, events.
- `media-playback.json`, `media-autoplay.json`, `media-poster.json`
  (78 cases): 97 of 110 `js` and `pixels` rows differ; the 13 equal rows are
  values that do not need playback (the poster pixels of a video that never
  loads, `readyState` 0 and the like).
- A page with three `<video controls>` (poster, `<source>` children) and an
  `<audio controls>`, JS off and on (`out/swb-media-*.png`): no request for
  a media file or a log line; the poster shows (JS off and on), the
  controls of ADR 0020 draw as measured; with JS on a `<video>` without
  `controls` and without a poster paints nothing. No console message, no
  warning.
- videojs.org (`fixtures/js/videojs`, JS on, replay): the page loads with no
  console error; its one `<video>` gets `playsinline`, `crossorigin` and
  `src="https://stream.mux.com/....m3u8"` (the HLS master playlist) and a
  `<track kind="metadata" label="thumbnails">`; `typeof MediaSource` is
  `"undefined"`; no request for the playlist is made (swb loads no media).
- **Chromium without `MediaSource`** (`replay videojs --init-script` that
  deletes `MediaSource`, `ManagedMediaSource` and `WebKitMediaSource`): the
  hero video gets the same `src` (the m3u8 URL), `canPlayType` of HLS is
  `maybe` there, Chromium's native HLS player requests the playlists
  (`rendition.m3u8`, not in the fixture) and the element ends with `error`
  code 4 (`PipelineStatus::DEMUXER_ERROR_COULD_NOT_PARSE` with the fixture
  that lacks the rendition playlist). So **Video.js falls back to native HLS
  (`video.src = master playlist`), shows no error text and no fallback to
  `highest.mp4`**. With swb after M14 (`canPlayType` `""` for HLS) the same
  `src` fails with `MEDIA_ERR_SRC_NOT_SUPPORTED` at once. M14 item 8 can
  therefore show: the media element API exists, the video has its poster and
  the controls of the page; the hero cannot play before M15.

## 11. Changes to the plan, and open questions for the ADR (step 2)

Changes to features 3 to 9 (the roadmap applies them with new numbers:
3 the FFmpeg build, 4 the media byte source, 5 the pipeline, 6a and 6b
the element, 7 frames, 8 audio output, 9 controls, 10 the exit test; the
text below keeps the numbers of the draft):

1. **A range-capable byte source** is the prerequisite of features 3 and 5
   (section 5): add to feature 3: "the media byte source: `Range` requests
   through `swb-net`, chunked reads of an open request, cancellation, replay
   of `Range` from a whole recorded body, no 64 MiB limit for media". The
   decode of the test files in feature 3 then reads the fixture files through
   it, with a local server of `Range` answers in the tests.
2. **Split feature 5**: 5a the element core (attributes, resource
   selection with `type` and `media`, the load algorithm and its events, the
   state machine of `networkState` and `readyState`, `MediaError` and its
   messages, `canPlayType`, `TimeRanges`, the fetches); 5b the playback API
   (`play()` and its promise rules, `pause()`, `currentTime` and the seeking
   events, `loop`, `volume`, `muted`, `autoplay` and the activation policy,
   `new Audio()`, `ended`, `played`, removal from the document, JS off
   behavior). The probe files are their tests: `media-can-play-type`,
   `media-states` for 5a; `media-playback`, `media-autoplay` for 5b; the Rust
   tests take Chromium's values from `probe --json` as the other probe tests
   do. Both run in virtual time with a silent output; the events come from
   the media clock, so the media clock (feature 4) must advance on virtual
   time without sleeping, and `timeupdate` every 250 ms of media time.
3. **Feature 6** takes the tolerances of section 7 (3 levels; 13 for BT.709
   limited), the untagged rule (601 at 640 x 480 and 720 x 576, 709 at
   1280 x 720; the boundary needs more sizes) and the
   poster rules of section 8; the poster stays until `play()` or a seek, so
   a paint of the first frame needs a "show poster" flag per element.
4. **Feature 8** (controls) has the measurements of section 9; the volume
   slider, the hover states, the auto-hide (3 s), the fade and the spinner
   arc are the states that the owner's list needs beyond ADR 0020. The
   controls need a timer in the GUI frame loop (the auto-hide fade) that must
   stop when nothing is visible (budget: 0 % idle).
5. **Feature 9**: the real files and the budgets of section 6 (the 4K file
   with a memory bound).
6. The probe tool and the automation API need: a way to give `--eval` user
   activation; media events in `--steps` (a `wait` that runs the media clock
   in virtual time); sampling at media times (`{"seek": ...}` is only
   `eval`); a `shot` equivalent for swb exists (`screenshot` step).
7. `swbtools capture` cannot make a media fixture (section 5, point 4).

Open questions for the ADR:

- FFmpeg build: which decoders of the list make how many bytes (the HEVC and
  AV1 ones stay out); the VP8/VP9 decoders of FFmpeg are native (LGPL) and
  the H.264 and AAC ones too; the MP3 decoder (`mp3float`) is LGPL. Does
  `libswscale` match Chromium's BT.709 limited result (section 7)? The
  tolerance 13 may need a hand-written conversion for that range.
- The I/O boundary: FFmpeg reads through custom I/O callbacks (`read`,
  `seek`), which block a decode thread on a network request; the Range
  chunk size (Chromium: 1 to 32 MiB), read-ahead (Chromium: about 15 s
  buffered with `preload=auto`, 3 to 6 s at `metadata`), and a byte budget
  for the source; what the callbacks do on cancel and on a server without
  `Range` (seek must fail then: `currentTime` goes to 0).
- Threads and the clock: one demux and one decode thread per element, a
  bound on concurrent decoders (hostile pages), the audio clock as master
  with PipeWire and a system clock without audio, the virtual clock in
  tests; the `timeupdate` period (250 ms); the start time of the stream as
  the origin of `currentTime`.
- Audio output: `--mute-audio` of the Playwright Chromium means the tests do
  not hear a difference; the mix of several elements, `volume` and `muted`,
  `volumechange` events; the silent output must still consume samples at
  real time (or virtual time) so that `ended` comes at the right time.
- Paint: the frame as a display list item with the RGB conversion in the
  raster; damage rectangles per frame (ADR 0044); BT.601/709 by the rule of
  section 7; the scale filter (bilinear); a frame is dropped when late.
- The element without the feature: `canPlayType` `""`, loading fails with
  `MEDIA_ERR_SRC_NOT_SUPPORTED` (`MEDIA_ELEMENT_ERROR: Format error` is
  Chromium's message for failed requests only; the message for a type that
  is not supported: the `<source type>` is skipped with an `error` on the
  source, `play()` rejects with `NotSupportedError: The element has no
  supported sources.`).
- `error.message` strings: Chromium's are implementation details
  (`PipelineStatus::...`); the ADR should say which of them swb copies
  (suggest: the two in section 3 that do not name internals, and a short
  text for the rest).

## 12. What was done and what was not

Done: items 1 to 8 of the brief. The probe files ran three times with equal
output (the exceptions: `media-real-files.json`, which cannot, section 5).
Not done: a headed Chromium (the headless shell gives the same decode and
the numbers of section 6 include its software compositor); the hour format of
the time text; the keyboard shortcuts; picture-in-picture, fullscreen,
text tracks and Media Source Extensions (M15); audio output levels (the
Playwright Chromium is muted); a color probe with graded patches to find the
exact BT.709 limited conversion; HEVC and AV1 files.
