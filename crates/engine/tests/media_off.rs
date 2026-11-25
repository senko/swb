//! M14 feature 6a: `Page::set_media(false)` (in both builds): a media element
//! loads nothing and fires no event, as before M14. The exit test of
//! videojs.org uses it (its Chromium recording has Media Source Extensions;
//! swb has none until M15).

// Test helpers outside `#[test]` functions unwrap too.
#![allow(clippy::unwrap_used)]

mod common;

use std::sync::Arc;
use std::time::Duration;

use common::media_site::MediaSite;
use common::{new_page, node, on_page_thread};
use swb_engine::{ClockMode, PageClock, Size, Url};
use swb_net::Fetcher;

#[test]
fn a_media_element_loads_nothing_when_media_is_off() {
    on_page_thread(|| {
        let html = "<!DOCTYPE html><script>const EVO = []; \
            function watch(m) { ['loadstart', 'error', 'loadedmetadata'].forEach(n => m.addEventListener(n, () => EVO.push(n))); }\
            </script><video id=v src=a.mp4></video><audio id=a><source src=a.mp4></audio><script>watch(v)</script>";
        for javascript in [true, false] {
            let mut site = MediaSite::new("case-1", html);
            site.add_file("a.mp4", "fixtures/media/mp4-h264-high-aac.mp4");
            let site = Arc::new(site);
            let mut page = new_page(
                Arc::clone(&site) as Arc<dyn Fetcher>,
                2,
                Size::new(800.0, 600.0),
            );
            page.set_javascript(javascript);
            page.set_media(false);
            page.set_clock(PageClock::new(ClockMode::Virtual, None));
            page.navigate(Url::parse(&MediaSite::page_url("case-1")).unwrap());
            assert!(page.wait_until_loaded(Duration::from_secs(20)));
            page.run_event_loop(Duration::from_secs(5), Duration::from_secs(30));
            for id in ["v", "a"] {
                let n = node(&page, id);
                let info = page.media_info(n).unwrap();
                assert_eq!((info.network_state, info.ready_state), (0, 0), "#{id}");
                assert!(info.error.is_none());
            }
            assert_eq!(page.media_pipelines(), 0);
            assert!(site.log_lines().is_empty(), "no request");
        }
    });
}
