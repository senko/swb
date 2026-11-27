"""The media element, M14 feature 6a (ADR 0047 sections 6, 10 and 11; review N1 and N2): many
`<source>` children that the resource selection skips, a flood of `load()` calls in one task,
and 16,000 `<video>` elements. They run in the default build (without the Cargo feature
`media`): the selection, the load algorithm and the events run, and each load fails with code 4
at once. Chromium has no limits for these; swb keeps the states of at most 16,384 media
elements, walks the `source` children with a pointer (no rescan), and delivers at most 4,096
events per round. The last three cases (M14 feature 7a and its review fix) play a real test
video: with the default build each load fails at once; run them with a build that has the
feature `media` (`just build-media`, then `swbtools hostile --swb target/release/swb NAME...`)
to decode and draw the frames."""

from swbtools import paths
from swbtools.hostile_cases.core import Page, case, doc, steps_args, virtual_run


def _fixture(name: str) -> bytes:
    """The bytes of a test file of `fixtures/media/`."""
    return (paths.repo_root() / "fixtures/media" / name).read_bytes()


RUNNING = virtual_run(3000)
"""Scripts and 3 s of virtual time: the selections and the events run."""


@case(
    "A `<video>` with 20,000 `<source media='not all'>` children (M14 feature 6a, review N1): "
    "the resource selection skips each with an `error` event at the source; the walk keeps a "
    "pointer, so the cost grows with the number of children, not its square",
    time_limit_s=20,
    swb_args=RUNNING,
    expect_log="source errors 20000",
)
def media_many_skipped_sources() -> Page:
    sources = "<source src=a.mp4 media='not all'>" * 20_000
    script = (
        "<script>let n=0;document.addEventListener('error',"
        "e=>{if(e.target.localName==='source')n++;},true);"
        "setTimeout(()=>console.warn('source errors '+n),2500);</script>"
    )
    return Page(doc(f"{script}<video id=v>{sources}</video>"))


@case(
    "One million `load()` calls on one `<video>` in one task (M14 feature 6a, review N2): each "
    "runs the load algorithm and queues `emptied` and `timeupdate` (two million events, "
    "delivered 4,096 per round); the queue is not bounded (backlog), the case shows that it "
    "stays within the limits",
    time_limit_s=30,
    swb_args=RUNNING,
    expect_log="load flood done",
)
def media_load_flood() -> Page:
    script = (
        "<script>let n=0;v.addEventListener('emptied',()=>n++);"
        "for(let i=0;i<1000000;i++)v.load();"
        "setTimeout(()=>console.warn('load flood done '+(n>0?'events':'none')),2500);</script>"
    )
    return Page(doc(f"<video id=v src=a.mp4></video>{script}"))


@case(
    "16,000 `<video src>` elements (M14 feature 6a, ADR 0047 section 10): each gets a state "
    "(at most 16,384), a selection and a load that fails at once without the feature "
    "(`loadstart`, `error`); 32,000 events",
    time_limit_s=30,
    rss_limit_mib=1536,
    swb_args=RUNNING,
    expect_log="video errors 16000",
)
def media_16000_videos() -> Page:
    videos = "<video src=a.mp4></video>" * 16_000
    script = (
        "<script>let n=0;document.addEventListener('error',"
        "e=>{if(e.target.localName==='video')n++;},true);"
        "setTimeout(()=>console.warn('video errors '+n),2500);</script>"
    )
    return Page(doc(script + videos))


@case(
    "200,000 pairs of `play()` and `pause()` on one muted `<video>` without a source in one task "
    "(M14 feature 6b, ADR 0047 section 10): each pair queues `play`, `waiting`, `timeupdate`, "
    "`pause` and a promise that `pause()` rejects; the queue of media tasks keeps at most 65,536 "
    "events (the settling of promises joins the one before it), every promise settles",
    time_limit_s=30,
    rss_limit_mib=1536,
    swb_args=RUNNING,
    expect_log="play flood settled 200000",
)
def media_play_pause_flood() -> Page:
    script = (
        "<script>const p=[];for(let i=0;i<200000;i++){p.push(v.play().then(()=>'ok',e=>e.name));"
        "v.pause();}"
        "Promise.all(p).then(r=>console.warn("
        "'play flood settled '+r.filter(x=>x==='AbortError').length));"
        "</script>"
    )
    return Page(doc(f"<video id=v muted></video>{script}"))


@case(
    "100,000 `play()` calls that wait on one muted `<video>` without a source (M14 feature 6b): "
    "at most 65,536 promises wait for their settling, the others reject at once; one `pause()` "
    "rejects the 65,536 (one task, not one task for each)",
    time_limit_s=30,
    rss_limit_mib=1536,
    swb_args=RUNNING,
    expect_log="many promises 65536 34464",
)
def media_many_play_promises() -> Page:
    script = (
        "<script>const p=[];for(let i=0;i<100000;i++)p.push(v.play().then(()=>'ok',e=>e.message));"
        "v.pause();"
        "Promise.all(p).then(r=>{const a=r.filter(x=>x.includes('pause()')).length;"
        "console.warn('many promises '+a+' '+r.filter(x=>x.includes('Too many')).length);});"
        "</script>"
    )
    return Page(doc(f"<video id=v muted></video>{script}"))


@case(
    "1,000,000 assignments to `currentTime` and 20,000 `new Audio(src).play()` calls in one task "
    "(M14 feature 6b): a position set before the metadata is one number; each `new Audio` is a "
    "detached element that gets a state (at most 16,384; the states of detached elements that do "
    "not play go first) and its `play()` rejects (no user activation)",
    time_limit_s=30,
    rss_limit_mib=1536,
    swb_args=RUNNING,
    expect_log="audio flood 20000",
)
def media_seek_and_audio_flood() -> Page:
    script = (
        "<script>for(let i=0;i<1000000;i++)v.currentTime=i%10;"
        "let n=0;for(let i=0;i<20000;i++)new Audio('a.mp3').play().catch(()=>n++);"
        "setTimeout(()=>console.warn('audio flood '+n),2500);</script>"
    )
    return Page(doc(f"<video id=v></video>{script}"))


# `mp4-h264-high-aac.mp4` is a 2 s test video (320 x 240, H.264, 55 KB): the cases below play it.


@case(
    "100 muted `autoplay loop` videos on screen, each its own pipeline (M14 feature 7a, ADR 0047 "
    "sections 6 and 9): 16 pipelines run at a time and decode, the others wait for a place; the "
    "screenshot at the end of 3 s of virtual time converts each shown frame once; time and "
    "memory stay bounded (a media build plays them, the default build fails each load)",
    time_limit_s=30,
    rss_limit_mib=1536,
    swb_args=virtual_run(3000),
    expect_log="videos 100",
    media=True,
)
def media_100_autoplay_videos() -> Page:
    videos = "<video src=a.mp4 muted autoplay loop width=120 height=90></video>" * 100
    script = (
        "<script>setTimeout(()=>console.warn('videos '+"
        "document.querySelectorAll('video').length),2500);</script>"
    )
    return Page(doc(videos + script), {"a.mp4": _fixture("mp4-h264-high-aac.mp4")})


@case(
    "A video in a box of 1,000,000 x 1,000,000 px with `object-fit: none`, and one scaled 1,000 "
    "times by a transform (M14 feature 7a, ADR 0047 section 9): the frame is placed at its "
    "natural size in the middle of the huge box, outside the viewport, and the scaled frame is "
    "drawn through the clip of the viewport; the screenshot is one viewport",
    time_limit_s=30,
    rss_limit_mib=1536,
    swb_args=virtual_run(1500),
    expect_log="huge boxes 2",
    media=True,
)
def media_huge_object_fit_none_box() -> Page:
    css = (
        "video{display:block}#a{width:1000000px;height:1000000px;object-fit:none}"
        "#b{width:100px;height:100px;object-fit:none;object-position:-1000000px 0;"
        "transform-origin:0 0;transform:scale(1000)}"
    )
    body = (
        "<video id=a src=a.mp4 muted autoplay loop></video>"
        "<video id=b src=a.mp4 muted autoplay loop></video>"
        "<script>setTimeout(()=>console.warn('huge boxes '+"
        "document.querySelectorAll('video').length),1200);</script>"
    )
    return Page(doc(body, css), {"a.mp4": _fixture("mp4-h264-high-aac.mp4")})


@case(
    "A video whose `loadstart` listener calls `setTimeout(() => v.load(), 0)` (M14 feature 7a, "
    "review fix, ADR 0047 section 9): the media never settle, so the screenshot at the end must "
    "stop after its bound (about 2 s of real time, no page task runs in it), not after 30 s",
    # The page reloads 300 times, then goes idle; the unit test
    # `a_screenshot_runs_no_page_task_and_ends_in_bounded_time` has the endless loop.
    time_limit_s=20,
    rss_limit_mib=1536,
    swb_args=virtual_run(1500),
    expect_log="reload loop",
    media=True,
)
def media_reload_loop_screenshot() -> Page:
    body = (
        "<video id=v src=a.mp4 muted preload=auto width=160 height=90></video>"
        "<script>let n=0;v.addEventListener('loadstart',"
        "()=>setTimeout(()=>{if(++n<300)v.load();},0));"
        "setTimeout(()=>console.warn('reload loop '+(n>0?'ran':'idle')),1200);</script>"
    )
    return Page(doc(body), {"a.mp4": _fixture("mp4-h264-high-aac.mp4")})


@case(
    "1,000 muted `autoplay loop` videos of 320 x 240 in a page that scrolls, in the frame loop of "
    "the GUI (M14 feature 7b, ADR 0047 section 9 and its notes: `--frame-loop-ms` runs the pump, "
    "the frame decision `frame_outdated`, the render of the damage and a scroll every 100 ms in "
    "real time with painted frames): 16 pipelines run, only the videos near the viewport convert "
    "frames, and the frame decision (it reads the frame queue of each running pipeline) stays "
    "below 50 ms (a media build plays them; the default build fails each load)",
    time_limit_s=40,
    rss_limit_mib=1536,
    swb_args=("--js", "--frame-loop-ms", "6000"),
    expect_log="frame loop ok",
    forbid_log="frame loop slow",
    media=True,
)
def media_1000_playing_videos_frame_loop() -> Page:
    videos = "<video src=a.mp4 muted autoplay loop width=320 height=240></video>" * 1000
    return Page(doc(videos), {"a.mp4": _fixture("mp4-h264-high-aac.mp4")})


@case(
    "2,000 clicks on the play button of the default controls of one video (M14 feature 9, ADR "
    "0047 notes of feature 9): each click toggles play and pause through the playback API, so "
    "the events queue at the element (the queue keeps at most 65,536) and every promise "
    "settles; the page stays responsive and the controls' picture is made once for the last "
    "state (a media build plays the video, the default build fails the load and the clicks do "
    "nothing)",
    time_limit_s=40,
    rss_limit_mib=1536,
    swb_args=steps_args([{"click": [24, 192]}] * 2000 + [{"wait": 6000}]),
    expect_log="control clicks ok events",
    media=True,
)
def media_controls_click_flood() -> Page:
    script = (
        "<script>let n=0;for(const e of ['play','pause'])v.addEventListener(e,()=>n++);"
        "setTimeout(()=>console.warn('control clicks ok '+(n>1000?'events':'none '+n)),5000);"
        "</script>"
    )
    return Page(
        doc(f"<video id=v src=a.mp4 controls width=320 height=240></video>{script}"),
        {"a.mp4": _fixture("mp4-h264-high-aac.mp4")},
    )


@case(
    "1,000 `<video controls>` elements of 320 x 240 under a pointer that moves over 300 places "
    "(M14 feature 9): each move updates the hover state of one element and the one it left; "
    "the places of the controls' boxes come from one walk of the fragment tree for all 1,000 "
    "(kept while the layout, the scroll and the display list do not change), not one walk for "
    "each move; time and memory stay bounded",
    time_limit_s=40,
    rss_limit_mib=1536,
    swb_args=steps_args(
        [{"hover": [20 + (i * 37) % 1500, 20 + (i * 53) % 700]} for i in range(300)]
        + [{"wait": 1000}]
    ),
    expect_log="controls moves ok",
    media=True,
)
def media_1000_videos_with_controls_under_a_moving_pointer() -> Page:
    videos = "<video src=a.mp4 controls width=320 height=240></video>" * 1000
    script = "<script>setTimeout(()=>console.warn('controls moves ok'),500);</script>"
    return Page(doc(videos + script), {"a.mp4": _fixture("mp4-h264-high-aac.mp4")})


# `m4a-aac.m4a` is a 2 s test audio file (AAC in MP4): the cases below play it.


@case(
    "100 muted `autoplay loop` elements that play audio (M14 feature 8, ADR 0047 sections 6 and "
    "8): each has its own pipeline and a ring of samples; 16 pipelines run at a time, the others "
    "wait for a place, and the silent output consumes the samples (headless mode never opens the "
    "audio output); time and memory stay bounded (a media build plays them, the default build "
    "fails each load)",
    time_limit_s=30,
    rss_limit_mib=1536,
    swb_args=virtual_run(3000),
    expect_log="audio elements 100",
    media=True,
)
def media_100_playing_audio_elements() -> Page:
    videos = "<video src=a.m4a muted autoplay loop></video>" * 100
    script = (
        "<script>setTimeout(()=>console.warn('audio elements '+"
        "document.querySelectorAll('video').length),2500);</script>"
    )
    return Page(doc(videos + script), {"a.m4a": _fixture("m4a-aac.m4a")})


@case(
    "`play()`, `pause()`, `volume` and `muted` toggled from a script in a loop (M14 feature 8, "
    "ADR 0047 sections 8 and 10): 20,000 rounds in one task and a 1 ms timer for 2.5 s of virtual "
    "time on one video with audio; each change of the gain reaches the pipeline; the queue of "
    "media tasks stays bounded, every promise settles, no crash. The run is headless: it never "
    "opens the audio output, so it does not reach the mixer (the mixer's churn is in the engine "
    "tests with the fake device)",
    time_limit_s=30,
    rss_limit_mib=1536,
    swb_args=virtual_run(3000),
    expect_log="toggles 20000 timer",
    media=True,
)
def media_play_pause_volume_toggle_loop() -> Page:
    script = (
        "<script>const v=document.querySelector('video');let n=0,t=0;"
        "for(let i=0;i<20000;i++){v.volume=(i%3)/2;v.muted=i%2==0;"
        "v.play().catch(()=>{});if(i%5==0)v.pause();n++;}"
        "const id=setInterval(()=>{v.volume=(t%3)/2;v.muted=t%2==0;"
        "if(++t%2)v.play().catch(()=>{});else v.pause();},1);"
        "setTimeout(()=>{clearInterval(id);console.warn('toggles '+n+' '+(t>100?'timer':'none'));},"
        "2500);</script>"
    )
    return Page(
        doc(f"<video src=a.m4a muted></video>{script}"),
        {"a.m4a": _fixture("m4a-aac.m4a")},
    )


# --- M14 feature 10a: hostile media files and servers (ADR 0047 section 11 and the notes of
# feature 10a). The media files come from `tools/media/generate.py`; Chromium's behaviour for
# each is in `tools/probes/media-hostile.json`.


def _report(expr: str, label: str, at_ms: int) -> str:
    """A script that logs `label` and the value of `expr` at `at_ms` of page time."""
    return f"<script>setTimeout(()=>console.warn('{label} '+({expr})),{at_ms});</script>"


_STATES = (
    "[...document.querySelectorAll('video,audio')].map(m=>m.error?'e'+m.error.code:m.readyState)"
)
"""The state of each media element: `eN` for an error of code N, else its `readyState`."""


@case(
    "16 muted `autoplay loop` videos at the frame size limit (8192 x 4352, 142.6 MB per RGBA "
    "frame; M14 feature 10a, ADR 0047 notes of feature 10a): the frame budget of the process "
    "(2 GiB) holds three such runs (713 MB each: a queue of 2 and 3 pixmaps); the others wait "
    "with their place and without pending work, and FFmpeg decodes such frames on 2 threads. "
    "Before feature 10a the page passed 16 GB and was killed; now about 2.4 GiB. The limit "
    "(3 GiB) fails every run without the 2-thread rule (3.2 to 3.7 GiB measured)",
    time_limit_s=30,
    rss_limit_mib=3072,
    swb_args=virtual_run(3000),
    expect_log="frame size limit videos 16",
    media_only=True,
)
def media_16_videos_at_the_frame_size_limit() -> Page:
    videos = "<video src=a.webm muted autoplay loop width=160 height=85></video>" * 16
    script = _report("document.querySelectorAll('video').length", "frame size limit videos", 2500)
    return Page(doc(videos + script), {"a.webm": _fixture("webm-vp9-8192x4352.webm")})


@case(
    "16 muted `autoplay loop` videos of a 86 KB WebM that declares 16 x 16 and switches in the "
    "stream to 8192 x 4352 (M14 feature 10a, review fixes): a run reserves frame bytes for the "
    "declared size, and a larger decoded frame grows the reservation at once or fails the run "
    "(code 3). Three runs get the 713 MB of the large frames, the other 13 fail; 2.0 to 2.2 "
    "GiB measured. Before the fix one such video held 643 MiB on a reservation for 16 x 16 "
    "frames, and 16 passed 4 GiB. Chromium plays the file (`resize`, `videoWidth` 8192; probe "
    "`media-hostile.json`)",
    time_limit_s=30,
    rss_limit_mib=3072,
    swb_args=virtual_run(3000),
    expect_log="size switch failed 13",
    media_only=True,
)
def media_16_videos_that_switch_to_the_frame_size_limit() -> Page:
    videos = "<video src=a.webm muted autoplay loop width=160 height=85></video>" * 16
    failed = "[...document.querySelectorAll('video')].filter(m=>m.error&&m.error.code===3).length"
    script = _report(failed, "size switch failed", 2500)
    return Page(doc(videos + script), {"a.webm": _fixture("webm-vp8-16x16-to-8192x4352.webm")})


@case(
    "Videos whose frames are above the size limits of ADR 0047 section 11 (16400 x 16, wider "
    "than 16,384 pixels; 8192 x 4416, more than 8192 x 4352 pixels): each fails at the "
    "metadata with code 4 and allocates no frame. Chromium differs (probe "
    "`media-hostile.json`): it plays 8192 x 4416 and fails 16400 x 16 with code 3 after "
    "`loadedmetadata`",
    time_limit_s=20,
    swb_args=virtual_run(2000),
    expect_log="above the limit e4,e4",
    media=True,
)
def media_frame_size_above_the_limit() -> Page:
    videos = "<video src=a.webm preload=auto></video><video src=b.webm preload=auto></video>"
    files = {
        "a.webm": _fixture("webm-vp9-16400x16.webm"),
        "b.webm": _fixture("webm-vp9-8192x4416.webm"),
    }
    return Page(doc(videos + _report(_STATES, "above the limit", 1500)), files)


@case(
    "A server that sends a valid 2 s MP4 file (55 KB) in a 200 without a length and then holds "
    "the connection open (an endless body; M14 feature 10a; real time: a request in flight "
    "holds virtual time): the byte source makes a block readable only when its 64 KiB are in, "
    "so the demuxer gets no byte and the pipeline fails with code 3 after the no-progress "
    "limit (10 s). Chromium plays 0.88 s and waits (`waiting`, `stalled`; probe "
    "`media-hostile.json`; a departure in the backlog)",
    time_limit_s=30,
    server="endless",
    swb_args=("--js", "--settle-ms", "13000"),
    expect_log="endless body 0,false,3,0",
    media_only=True,
)
def media_endless_body() -> Page:
    body = "<video src=http://127.0.0.1:__PORT__/a.mp4 muted autoplay></video>"
    state = "[v.readyState,v.ended,v.error&&v.error.code,Math.round(v.currentTime*10)/10]"
    script = "<script>const v=document.querySelector('video');</script>"
    return Page(
        doc(body + script + _report(state, "endless body", 12000)),
        {"a.mp4": _fixture("mp4-h264-high-aac.mp4")},
    )


@case(
    "A server that sends a valid MP4 file and then MP4 `free` boxes without end (bytes that the "
    "demuxer skips; M14 feature 10a; real time): the skipped bytes count as progress, so the "
    "no-progress limit does not fail the pipeline, and the byte budget bounds the memory of "
    "the endless body. Measured (fix round of 10a, also before it): the video plays to 1.0 s "
    "and waits there (`readyState` 2, no error) from 1.5 s to at least 8 s; Chromium plays to "
    "1.98 s and waits (a departure in the backlog)",
    time_limit_s=30,
    server="endless-skip",
    swb_args=("--js", "--settle-ms", "4000"),
    expect_log="endless skip 2,false,,1",
    media_only=True,
)
def media_endless_skip_body() -> Page:
    body = "<video src=http://127.0.0.1:__PORT__/a.mp4 muted autoplay></video>"
    state = "[v.readyState,v.ended,v.error&&v.error.code,Math.round(v.currentTime*10)/10]"
    script = "<script>const v=document.querySelector('video');</script>"
    return Page(
        doc(body + script + _report(state, "endless skip", 3500)),
        {"a.mp4": _fixture("mp4-h264-high-aac.mp4")},
    )


def _range_lie_page() -> Page:
    # A 10 s file (302 KB): the server closes each answer after 64 KiB, so the byte source asks
    # again for the rest during the load and meets the lie; the seek to 9 s asks once more.
    body = "<video src=http://127.0.0.1:__PORT__/a.mp4 preload=metadata></video>"
    script = (
        "<script>const v=document.querySelector('video');"
        "v.addEventListener('loadedmetadata',()=>setTimeout(()=>{v.currentTime=9;},200));</script>"
    )
    state = "[v.readyState,v.error&&v.error.code,v.currentTime]"
    return Page(
        doc(body + script + _report(state, "range lie", 2500)),
        {"a.mp4": _fixture("mp4-h264-aac-long.mp4")},
    )


@case(
    "A server whose `Content-Range` starts one byte after the requested start from its second "
    "answer on, and that closes each answer after 64 KiB (`video-range-lies`, M14 feature 10a): "
    "the byte source fails the resource at the second request and the demuxer ends the stream "
    "there, without an error event: after the seek to 9 s the element has `readyState` 4, no "
    "error, `currentTime` 9; no crash or hang. Chromium fails such a resource with code 4 "
    "(probe `media-hostile.json`; a departure in the backlog)",
    time_limit_s=20,
    server="lie-range",
    swb_args=virtual_run(3000),
    expect_log="range lie 4,,9",
    media_only=True,
)
def media_range_lie_start() -> Page:
    return _range_lie_page()


@case(
    "A server whose `Content-Range` total grows by one at each answer and that closes each "
    "answer after 64 KiB (`video-range-lies`, M14 feature 10a): the byte source fails the "
    "resource at the second request and the stream ends there, without an error event (after "
    "the seek to 9 s: `readyState` 4, no error, `currentTime` 9). Chromium plays on (probe "
    "`media-hostile.json`)",
    time_limit_s=20,
    server="lie-total",
    swb_args=virtual_run(3000),
    expect_log="range lie 4,,9",
    media_only=True,
)
def media_range_lie_total() -> Page:
    return _range_lie_page()


@case(
    "1,000 `<video preload=metadata>` elements of a 1280 x 720 file that all decode their "
    "first frame (M14 feature 10a): 16 pipelines run at a time (16 places; each suspends 15 s "
    "after its load, 1,000 s of virtual time), their frames stay within the frame budget, and "
    "the frames that the suspended ones keep stay within the budget of retained frames (256 "
    "MiB; 1,000 such frames are 3.7 GB). The same page with a 1080p file held 10 GB before "
    "feature 10a and 2.6 GB after",
    time_limit_s=40,
    rss_limit_mib=2048,
    swb_args=virtual_run(1_001_000),
    expect_log="decoded 1000",
    media_only=True,
)
def media_1000_videos_that_decode() -> Page:
    videos = "<video src=a.mp4 preload=metadata width=32 height=18></video>" * 1000
    script = (
        "<script>let n=0;document.addEventListener('loadeddata',()=>n++,true);"
        "setTimeout(()=>console.warn('decoded '+n),1000000);</script>"
    )
    return Page(doc(videos + script), {"a.mp4": _fixture("mp4-h264-untagged-bt709-1280x720.mp4")})


@case(
    "Files with 100 streams (one video and 99 audio streams in MP4 and WebM; 100 Opus streams "
    "in Ogg; M14 feature 10a): the Matroska demuxer keeps the first 16 and the video plays; the "
    "MOV and Ogg demuxers fail the open at the 17th stream (code 4). Chromium plays all three "
    "(probe `media-hostile.json`; a departure in the backlog)",
    time_limit_s=20,
    swb_args=virtual_run(2000),
    expect_log="streams e4,4,e4",
    media_only=True,
)
def media_100_streams() -> Page:
    body = (
        "<video src=a.mp4 preload=auto></video><video src=b.webm preload=auto></video>"
        "<audio src=c.ogg preload=auto></audio>"
    )
    files = {
        "a.mp4": _fixture("mp4-h264-aac-100-streams.mp4"),
        "b.webm": _fixture("webm-vp8-opus-100-streams.webm"),
        "c.ogg": _fixture("ogg-opus-100-streams.ogg"),
    }
    return Page(doc(body + _report(_STATES, "streams", 1500)), files)


@case(
    "A media server that never sends a byte (a stalled open, M14 feature 10a): the pipeline "
    "needs the metadata and makes no progress, so it fails with code 3 after the no-progress "
    "limit (10 s of real time); virtual time waits for it meanwhile. Chromium fires `stalled` "
    "after 3 s and waits (probe `media-hostile.json`; a departure in the backlog)",
    time_limit_s=30,
    server="never",
    swb_args=virtual_run(2000),
    expect_log="stalled open e3",
    media_only=True,
)
def media_stalled_open() -> Page:
    body = "<video src=http://127.0.0.1:__PORT__/a.mp4 preload=auto></video>"
    return Page(doc(body + _report(_STATES, "stalled open", 1500)))
