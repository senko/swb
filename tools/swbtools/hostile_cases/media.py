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

from pathlib import Path

from swbtools.hostile_cases.core import Page, case, doc, virtual_run

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


FIXTURE_VIDEO = Path(__file__).resolve().parents[3] / "fixtures/media/mp4-h264-high-aac.mp4"
"""A 2 s test video (320 x 240, H.264, 55 KB): the cases below play it."""


@case(
    "100 muted `autoplay loop` videos on screen, each its own pipeline (M14 feature 7a, ADR 0047 "
    "sections 6 and 9): 16 pipelines run at a time and decode, the others wait for a place; the "
    "screenshot at the end of 3 s of virtual time converts each shown frame once; time and "
    "memory stay bounded (a media build plays them, the default build fails each load)",
    time_limit_s=30,
    rss_limit_mib=1536,
    swb_args=virtual_run(3000),
    expect_log="videos 100",
)
def media_100_autoplay_videos() -> Page:
    videos = "<video src=a.mp4 muted autoplay loop width=120 height=90></video>" * 100
    script = (
        "<script>setTimeout(()=>console.warn('videos '+"
        "document.querySelectorAll('video').length),2500);</script>"
    )
    return Page(doc(videos + script), {"a.mp4": FIXTURE_VIDEO.read_bytes()})


@case(
    "A video in a box of 1,000,000 x 1,000,000 px with `object-fit: none`, and one scaled 1,000 "
    "times by a transform (M14 feature 7a, ADR 0047 section 9): the frame is placed at its "
    "natural size in the middle of the huge box, outside the viewport, and the scaled frame is "
    "drawn through the clip of the viewport; the screenshot is one viewport",
    time_limit_s=30,
    rss_limit_mib=1536,
    swb_args=virtual_run(1500),
    expect_log="huge boxes 2",
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
    return Page(doc(body, css), {"a.mp4": FIXTURE_VIDEO.read_bytes()})


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
)
def media_reload_loop_screenshot() -> Page:
    body = (
        "<video id=v src=a.mp4 muted preload=auto width=160 height=90></video>"
        "<script>let n=0;v.addEventListener('loadstart',"
        "()=>setTimeout(()=>{if(++n<300)v.load();},0));"
        "setTimeout(()=>console.warn('reload loop '+(n>0?'ran':'idle')),1200);</script>"
    )
    return Page(doc(body), {"a.mp4": FIXTURE_VIDEO.read_bytes()})


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
)
def media_1000_playing_videos_frame_loop() -> Page:
    videos = "<video src=a.mp4 muted autoplay loop width=320 height=240></video>" * 1000
    return Page(doc(videos), {"a.mp4": FIXTURE_VIDEO.read_bytes()})


FIXTURE_AUDIO = Path(__file__).resolve().parents[3] / "fixtures/media/m4a-aac.m4a"
"""A 2 s test audio file (AAC in MP4): the cases below play it."""


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
)
def media_100_playing_audio_elements() -> Page:
    videos = "<video src=a.m4a muted autoplay loop></video>" * 100
    script = (
        "<script>setTimeout(()=>console.warn('audio elements '+"
        "document.querySelectorAll('video').length),2500);</script>"
    )
    return Page(doc(videos + script), {"a.m4a": FIXTURE_AUDIO.read_bytes()})


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
        {"a.m4a": FIXTURE_AUDIO.read_bytes()},
    )
