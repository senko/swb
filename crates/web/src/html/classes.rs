//! The interface of an element in the HTML namespace: a table by tag
//! name (HTML "elements", the element index; the interfaces were also
//! measured in Chromium, `tools/probes/web-html-elements.json`).
//!
//! An element whose name is not in the table gets `HTMLElement` if the name
//! is a valid custom element name (`swb_dom::is_valid_custom_element_name`),
//! else `HTMLUnknownElement` (DOM "create an element" step 7).

use std::collections::HashMap;
use std::sync::OnceLock;

use swb_dom::LocalName;
use swb_js::ClassId;

use crate::generated as g;

/// The interface of each tag name that has its own interface.
#[allow(clippy::too_many_lines, reason = "one table")]
fn table() -> &'static HashMap<LocalName, ClassId> {
    static TABLE: OnceLock<HashMap<LocalName, ClassId>> = OnceLock::new();
    TABLE.get_or_init(|| {
        let entries: &[(&[&str], ClassId)] = &[
            (&["a"], g::HTML_ANCHOR_ELEMENT),
            (&["area"], g::HTML_AREA_ELEMENT),
            (&["audio"], g::HTML_AUDIO_ELEMENT),
            (&["base"], g::HTML_BASE_ELEMENT),
            (&["blockquote", "q"], g::HTML_QUOTE_ELEMENT),
            (&["body"], g::HTML_BODY_ELEMENT),
            (&["br"], g::HTMLBR_ELEMENT),
            (&["button"], g::HTML_BUTTON_ELEMENT),
            (&["canvas"], g::HTML_CANVAS_ELEMENT),
            (&["caption"], g::HTML_TABLE_CAPTION_ELEMENT),
            (&["col", "colgroup"], g::HTML_TABLE_COL_ELEMENT),
            (&["data"], g::HTML_DATA_ELEMENT),
            (&["datalist"], g::HTML_DATA_LIST_ELEMENT),
            (&["del", "ins"], g::HTML_MOD_ELEMENT),
            (&["details"], g::HTML_DETAILS_ELEMENT),
            (&["dialog"], g::HTML_DIALOG_ELEMENT),
            (&["dir"], g::HTML_DIRECTORY_ELEMENT),
            (&["div"], g::HTML_DIV_ELEMENT),
            (&["dl"], g::HTMLD_LIST_ELEMENT),
            (&["embed"], g::HTML_EMBED_ELEMENT),
            (&["fieldset"], g::HTML_FIELD_SET_ELEMENT),
            (&["font"], g::HTML_FONT_ELEMENT),
            (&["form"], g::HTML_FORM_ELEMENT),
            (&["frame"], g::HTML_FRAME_ELEMENT),
            (&["frameset"], g::HTML_FRAME_SET_ELEMENT),
            (
                &["h1", "h2", "h3", "h4", "h5", "h6"],
                g::HTML_HEADING_ELEMENT,
            ),
            (&["head"], g::HTML_HEAD_ELEMENT),
            (&["hr"], g::HTMLHR_ELEMENT),
            (&["html"], g::HTML_HTML_ELEMENT),
            (&["iframe"], g::HTMLI_FRAME_ELEMENT),
            (&["img"], g::HTML_IMAGE_ELEMENT),
            (&["input"], g::HTML_INPUT_ELEMENT),
            (&["label"], g::HTML_LABEL_ELEMENT),
            (&["legend"], g::HTML_LEGEND_ELEMENT),
            (&["li"], g::HTMLLI_ELEMENT),
            (&["link"], g::HTML_LINK_ELEMENT),
            (&["map"], g::HTML_MAP_ELEMENT),
            (&["marquee"], g::HTML_MARQUEE_ELEMENT),
            (&["menu"], g::HTML_MENU_ELEMENT),
            (&["meta"], g::HTML_META_ELEMENT),
            (&["meter"], g::HTML_METER_ELEMENT),
            (&["object"], g::HTML_OBJECT_ELEMENT),
            (&["ol"], g::HTMLO_LIST_ELEMENT),
            (&["optgroup"], g::HTML_OPT_GROUP_ELEMENT),
            (&["option"], g::HTML_OPTION_ELEMENT),
            (&["output"], g::HTML_OUTPUT_ELEMENT),
            (&["p"], g::HTML_PARAGRAPH_ELEMENT),
            (&["param"], g::HTML_PARAM_ELEMENT),
            (&["picture"], g::HTML_PICTURE_ELEMENT),
            (&["pre", "listing", "xmp"], g::HTML_PRE_ELEMENT),
            (&["progress"], g::HTML_PROGRESS_ELEMENT),
            (&["script"], g::HTML_SCRIPT_ELEMENT),
            (&["select"], g::HTML_SELECT_ELEMENT),
            (&["selectedcontent"], g::HTML_SELECTED_CONTENT_ELEMENT),
            (&["slot"], g::HTML_SLOT_ELEMENT),
            (&["source"], g::HTML_SOURCE_ELEMENT),
            (&["span"], g::HTML_SPAN_ELEMENT),
            (&["style"], g::HTML_STYLE_ELEMENT),
            (&["table"], g::HTML_TABLE_ELEMENT),
            (&["tbody", "tfoot", "thead"], g::HTML_TABLE_SECTION_ELEMENT),
            (&["td", "th"], g::HTML_TABLE_CELL_ELEMENT),
            (&["template"], g::HTML_TEMPLATE_ELEMENT),
            (&["textarea"], g::HTML_TEXT_AREA_ELEMENT),
            (&["time"], g::HTML_TIME_ELEMENT),
            (&["title"], g::HTML_TITLE_ELEMENT),
            (&["tr"], g::HTML_TABLE_ROW_ELEMENT),
            (&["track"], g::HTML_TRACK_ELEMENT),
            (&["ul"], g::HTMLU_LIST_ELEMENT),
            (&["video"], g::HTML_VIDEO_ELEMENT),
            // Known elements without members of their own.
            (
                &[
                    "abbr",
                    "acronym",
                    "address",
                    "article",
                    "aside",
                    "b",
                    "basefont",
                    "bdi",
                    "bdo",
                    "big",
                    "center",
                    "cite",
                    "code",
                    "dd",
                    "dfn",
                    "dt",
                    "em",
                    "figcaption",
                    "figure",
                    "footer",
                    "header",
                    "hgroup",
                    "i",
                    "kbd",
                    "main",
                    "mark",
                    "nav",
                    "nobr",
                    "noembed",
                    "noframes",
                    "noscript",
                    "plaintext",
                    "rb",
                    "rp",
                    "rt",
                    "rtc",
                    "ruby",
                    "s",
                    "samp",
                    "search",
                    "section",
                    "small",
                    "strike",
                    "strong",
                    "sub",
                    "summary",
                    "sup",
                    "tt",
                    "u",
                    "var",
                    "wbr",
                ],
                g::HTML_ELEMENT,
            ),
        ];
        let mut table = HashMap::new();
        for (names, class) in entries {
            for name in *names {
                table.insert(LocalName::from(*name), *class);
            }
        }
        table
    })
}

/// The interface of an element in the HTML namespace with this local
/// name.
pub(crate) fn class_of_html(local: &LocalName) -> ClassId {
    if let Some(class) = table().get(local) {
        return *class;
    }
    if swb_dom::is_valid_custom_element_name(local) {
        g::HTML_ELEMENT
    } else {
        g::HTML_UNKNOWN_ELEMENT
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classes_by_name() {
        assert_eq!(class_of_html(&LocalName::from("div")), g::HTML_DIV_ELEMENT);
        assert_eq!(class_of_html(&LocalName::from("xmp")), g::HTML_PRE_ELEMENT);
        assert_eq!(class_of_html(&LocalName::from("em")), g::HTML_ELEMENT);
        assert_eq!(class_of_html(&LocalName::from("x-y")), g::HTML_ELEMENT);
        assert_eq!(
            class_of_html(&LocalName::from("\u{e9}")),
            g::HTML_UNKNOWN_ELEMENT
        );
        assert_eq!(
            class_of_html(&LocalName::from("DIV")),
            g::HTML_UNKNOWN_ELEMENT
        );
        assert_eq!(
            class_of_html(&LocalName::from("image")),
            g::HTML_UNKNOWN_ELEMENT
        );
    }
}
