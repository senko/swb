"""The media element, M14 feature 6a (ADR 0047 sections 6, 10 and 11; review N1 and N2): many
`<source>` children that the resource selection skips, a flood of `load()` calls in one task,
and 16,000 `<video>` elements. They run in the default build (without the Cargo feature
`media`): the selection, the load algorithm and the events run, and each load fails with code 4
at once. Chromium has no limits for these; swb keeps the states of at most 16,384 media
elements, walks the `source` children with a pointer (no rescan), and delivers at most 4,096
events per round."""

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
