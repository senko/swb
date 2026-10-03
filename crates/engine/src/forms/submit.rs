//! Form submission: the entry list of a form, constraint validation, and
//! the request that submits the form.
//!
//! <https://html.spec.whatwg.org/multipage/form-control-infrastructure.html#form-submission-algorithm>.
//! Deliberate simplifications: the `target` attribute is ignored (the
//! result replaces the page), `mailto:` and `javascript:` actions do
//! nothing, `dialog` forms do nothing, and constraint validation checks
//! only `required`.

use encoding_rs::Encoding;
use swb_dom::{Document, NodeId, is_html_whitespace, local_name};
use swb_net::Url;
use swb_style::is_actually_disabled;

use super::encode::{self, Entry, EntryValue};
use super::{ControlType, Forms, InputType, option_value};
use crate::history::PostData;

/// What submits a form.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Submitter {
    /// The form itself: implicit submission without a submit button.
    Form,
    /// A submit button. For an image button, the coordinate of the click
    /// relative to the button (`None` for a keyboard activation: 0, 0).
    Button {
        node: NodeId,
        coordinate: Option<(i32, i32)>,
    },
}

impl Submitter {
    fn node(self) -> Option<NodeId> {
        match self {
            Submitter::Form => None,
            Submitter::Button { node, .. } => Some(node),
        }
    }
}

/// The request that submits a form: a `GET` of `url`, or a `POST` with a
/// body.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FormRequest {
    pub(crate) url: Url,
    pub(crate) post: Option<PostData>,
}

/// The document that a form submission needs.
pub(crate) struct FormContext<'a> {
    pub(crate) doc: &'a Document,
    pub(crate) forms: &'a Forms,
    /// The document's base URL (for the action).
    pub(crate) base_url: &'a Url,
    /// The document's URL (the action if the form has none).
    pub(crate) document_url: &'a Url,
    /// The document's character encoding.
    pub(crate) encoding: &'static Encoding,
}

/// The request that submits `form` with `submitter`, or `None` if the
/// submission does not navigate (an invalid action URL, an unsupported
/// scheme, a `dialog` form).
pub(crate) fn form_submission(
    cx: &FormContext<'_>,
    form: NodeId,
    submitter: Submitter,
) -> Option<FormRequest> {
    let doc = cx.doc;
    let form_element = doc.element(form)?;
    // A submit button's `form*` attribute overrides the form's attribute.
    let attr = |submitter_name: &str, form_name: &str| {
        submitter
            .node()
            .and_then(|n| doc.element(n))
            .and_then(|e| e.attr(submitter_name))
            .or_else(|| form_element.attr(form_name))
    };
    let encoding = pick_encoding(form_element.attr("accept-charset"), cx.encoding);
    let entries = entry_list(cx, form, submitter, encoding);
    let mut url = action_url(cx, attr("formaction", "action").unwrap_or(""))?;
    // `method` and `enctype` are enumerated attributes: matched ASCII
    // case-insensitively and not trimmed (`" post"` is the default, GET).
    let method = attr("formmethod", "method").map(str::to_ascii_lowercase);
    let post = match method.as_deref() {
        Some("post") => true,
        Some("dialog") => {
            log::warn!("form not submitted: dialog forms are not supported");
            return None;
        }
        _ => false,
    };
    match url.scheme() {
        "http" | "https" | "file" | "about" | "data" => {}
        scheme => {
            log::warn!("form not submitted: actions with the scheme {scheme}: are not supported");
            return None;
        }
    }
    if !post || url.scheme() == "data" {
        // Mutate the action URL: the query is the urlencoded entries.
        if !post {
            url.set_query(Some(&encode::urlencoded(&entries, encoding)));
        }
        return Some(FormRequest { url, post: None });
    }
    let enctype = attr("formenctype", "enctype").map(str::to_ascii_lowercase);
    let post = encode_body(&entries, enctype.as_deref(), encoding);
    Some(FormRequest {
        url,
        post: Some(post),
    })
}

/// The action URL: `action` parsed against the document's base URL, or
/// the document URL if `action` is empty.
fn action_url(cx: &FormContext<'_>, action: &str) -> Option<Url> {
    if action.is_empty() {
        return Some(cx.document_url.clone());
    }
    match cx.base_url.join(action.trim_matches(is_html_whitespace)) {
        Ok(url) => Some(url),
        Err(e) => {
            log::warn!("form not submitted: the action {action:?} is not a valid URL: {e}");
            None
        }
    }
}

/// The body of a `POST` submission in the encoding that `enctype` names
/// (urlencoded for a missing or unknown value).
fn encode_body(entries: &[Entry], enctype: Option<&str>, encoding: &'static Encoding) -> PostData {
    let (body, content_type) = match enctype {
        Some("multipart/form-data") => {
            let boundary = encode::boundary();
            (
                encode::multipart(entries, encoding, &boundary),
                format!("multipart/form-data; boundary={boundary}"),
            )
        }
        Some("text/plain") => (
            encode::text_plain(entries, encoding),
            "text/plain".to_owned(),
        ),
        _ => (
            encode::urlencoded(entries, encoding).into_bytes(),
            "application/x-www-form-urlencoded".to_owned(),
        ),
    };
    PostData {
        body: body.into(),
        content_type,
    }
}

/// The encoding of a form: the first label in `accept-charset` that names
/// an encoding, else the document's encoding. Deviations, both as in
/// Chromium (measured, `FormDataEncoder::EncodingFromAcceptCharset`): the
/// labels are separated by commas as well as by whitespace (the
/// specification splits on whitespace only), and with an `accept-charset`
/// that names no encoding, the document's encoding is used (the
/// specification says UTF-8).
/// <https://html.spec.whatwg.org/multipage/form-control-infrastructure.html#picking-an-encoding-for-the-form>
fn pick_encoding(accept_charset: Option<&str>, document: &'static Encoding) -> &'static Encoding {
    accept_charset
        .into_iter()
        .flat_map(|v| v.split(|c| c == ',' || is_html_whitespace(c)))
        .find_map(|label| Encoding::for_label(label.as_bytes()))
        .unwrap_or(document)
        .output_encoding()
}

/// The entry list of `form`: the names and values of its submittable
/// controls in tree order.
/// <https://html.spec.whatwg.org/multipage/form-control-infrastructure.html#constructing-the-form-data-set>
fn entry_list(
    cx: &FormContext<'_>,
    form: NodeId,
    submitter: Submitter,
    encoding: &'static Encoding,
) -> Vec<Entry> {
    let (doc, forms) = (cx.doc, cx.forms);
    let mut entries = Vec::new();
    for node in doc.descendants(NodeId::DOCUMENT) {
        let (Some(state), Some(e)) = (forms.get(node), doc.element(node)) else {
            continue;
        };
        let skipped = state.owner != Some(form)
            || state.disabled
            || (state.ty.is_button() && submitter.node() != Some(node))
            || (state.ty.is_checkable() && !state.checked)
            || doc
                .ancestors(node)
                .any(|a| doc.is_html_element(a, &local_name!("datalist")));
        if skipped {
            continue;
        }
        let name = e.attr("name").unwrap_or("");
        if state.ty == ControlType::Input(InputType::Image) {
            image_entries(&mut entries, name, submitter);
        } else if !name.is_empty() {
            control_entries(&mut entries, cx, node, name, encoding);
            if let Some(dirname) = e.attr("dirname").filter(|d| !d.is_empty())
                && has_dirname(state.ty)
            {
                entries.push(Entry::text(dirname, direction(doc, node)));
            }
        }
    }
    entries
}

/// The entries of a submitting image button: the coordinate of the click
/// (0, 0 for the keyboard) as `name.x` and `name.y` (`x` and `y` without a
/// name).
fn image_entries(entries: &mut Vec<Entry>, name: &str, submitter: Submitter) {
    let (x, y) = match submitter {
        Submitter::Button {
            coordinate: Some(c),
            ..
        } => c,
        _ => (0, 0),
    };
    let prefix = if name.is_empty() {
        String::new()
    } else {
        format!("{name}.")
    };
    entries.push(Entry::text(&format!("{prefix}x"), &x.to_string()));
    entries.push(Entry::text(&format!("{prefix}y"), &y.to_string()));
}

/// The entries of a control with a name: the selected options of a
/// select, an empty file of a file input, the encoding for a hidden
/// `_charset_` input, otherwise the value.
fn control_entries(
    entries: &mut Vec<Entry>,
    cx: &FormContext<'_>,
    node: NodeId,
    name: &str,
    encoding: &'static Encoding,
) {
    let (doc, forms) = (cx.doc, cx.forms);
    let Some(state) = forms.get(node) else {
        return;
    };
    match state.ty {
        ControlType::Select => {
            for option in state.options.iter().filter(|o| o.selected) {
                if !is_actually_disabled(doc, option.node) {
                    entries.push(Entry::text(name, &option_value(doc, option.node)));
                }
            }
        }
        ControlType::Input(InputType::File) => entries.push(Entry {
            name: name.to_owned(),
            value: EntryValue::File {
                filename: String::new(),
                content_type: "application/octet-stream".to_owned(),
                data: Vec::new(),
            },
        }),
        ControlType::Input(InputType::Hidden) if name.eq_ignore_ascii_case("_charset_") => {
            entries.push(Entry::text(name, encoding.name()));
        }
        _ => {
            let value = forms.value(doc, node).unwrap_or_default();
            entries.push(Entry::text(name, &value));
        }
    }
}

/// True for the controls whose `dirname` attribute adds an entry with the
/// direction: text areas, and inputs of type hidden, text, search, tel,
/// url, email, password, submit, reset and button.
/// <https://html.spec.whatwg.org/multipage/form-control-infrastructure.html#attr-fe-dirname>
fn has_dirname(ty: ControlType) -> bool {
    match ty {
        ControlType::TextArea => true,
        ControlType::Input(t) => {
            t.has_size()
                || matches!(
                    t,
                    InputType::Hidden | InputType::Submit | InputType::Reset | InputType::Button
                )
        }
        _ => false,
    }
}

/// The directionality of an element for `dirname`: from the nearest valid
/// `dir` attribute (`ltr`, `rtl` or `auto`, ASCII case-insensitive) on the
/// element or an ancestor; invalid values are skipped (as the
/// specification and Chromium do). Without one, `ltr`. Deliberate
/// simplification: `dir=auto` counts as `ltr` (it should look at the first
/// strong character of the value). Chromium sends the attribute value in
/// its own case (`RTL`); swb sends the directionality in lower case, as
/// the specification says.
/// <https://html.spec.whatwg.org/multipage/dom.html#the-directionality>
fn direction(doc: &Document, node: NodeId) -> &'static str {
    let dir = std::iter::once(node)
        .chain(doc.ancestors(node))
        .filter_map(|n| doc.element(n)?.attr("dir"))
        .find(|d| {
            ["ltr", "rtl", "auto"]
                .iter()
                .any(|v| d.eq_ignore_ascii_case(v))
        });
    if dir.is_some_and(|d| d.eq_ignore_ascii_case("rtl")) {
        "rtl"
    } else {
        "ltr"
    }
}

/// The default button of a form: its first submit button in tree order.
/// <https://html.spec.whatwg.org/multipage/form-control-infrastructure.html#default-button>
pub(crate) fn default_button(doc: &Document, forms: &Forms, form: NodeId) -> Option<NodeId> {
    doc.descendants(NodeId::DOCUMENT).find(|&n| {
        forms
            .control_type(n)
            .is_some_and(ControlType::is_submit_button)
            && forms.owner(n) == Some(form)
    })
}

/// The number of fields of a form that block implicit submission.
pub(crate) fn blocking_fields(doc: &Document, forms: &Forms, form: NodeId) -> usize {
    doc.descendants(NodeId::DOCUMENT)
        .filter(|&n| {
            matches!(forms.control_type(n), Some(ControlType::Input(t)) if t.blocks_implicit_submission())
                && forms.owner(n) == Some(form)
        })
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use swb_dom::parse_html;

    fn submit(html: &str, submitter: Option<&str>) -> Option<FormRequest> {
        let doc = parse_html(html);
        let forms = Forms::new(&doc);
        let url = Url::parse("https://example.test/dir/page?old=1").unwrap();
        let cx = FormContext {
            doc: &doc,
            forms: &forms,
            base_url: &url,
            document_url: &url,
            encoding: encoding_rs::UTF_8,
        };
        let form = doc
            .find_element(NodeId::DOCUMENT, |e| e.is_html_named(&local_name!("form")))
            .unwrap();
        let submitter = match submitter {
            Some(id) => Submitter::Button {
                node: doc.element_by_id(id).unwrap(),
                coordinate: Some((3, 4)),
            },
            None => Submitter::Form,
        };
        form_submission(&cx, form, submitter)
    }

    #[test]
    fn get_replaces_the_query() {
        let r = submit(
            "<form action=search><input name=q value='a b'><input name=n>\
             <input type=checkbox name=c><input type=checkbox name=d checked>\
             <input type=radio name=r value=1><input type=radio name=r value=2 checked>\
             <input name=dis disabled value=x><input value=noname>\
             <select name=s><option>o1<option selected value=v2>o2</select>\
             <textarea name=t>l1\nl2</textarea><input type=hidden name=_charset_>\
             <input type=submit name=go value=Go><button name=b value=bv>B</button></form>",
            None,
        )
        .unwrap();
        assert_eq!(r.post, None);
        assert_eq!(
            r.url.as_str(),
            "https://example.test/dir/search?q=a+b&n=&d=on&r=2&s=v2&t=l1%0D%0Al2&_charset_=UTF-8"
        );
    }

    #[test]
    fn multiple_selects_send_every_selected_option() {
        let r = submit(
            "<form><select name=m multiple><option selected>a<option>b\
             <option selected disabled>x<option selected>c</select>\
             <datalist><input name=hidden value=1></datalist></form>",
            None,
        )
        .unwrap();
        assert_eq!(r.url.query(), Some("m=a&m=c"));
    }

    #[test]
    fn the_submitter_is_included() {
        let r = submit(
            "<form><input name=q value=x><input type=submit name=a value=A id=a>\
             <input type=submit name=b value=B id=b></form>",
            Some("b"),
        )
        .unwrap();
        assert_eq!(r.url.as_str(), "https://example.test/dir/page?q=x&b=B");
        let r = submit(
            "<form><input type=image name=img id=i><input type=image id=j></form>",
            Some("i"),
        )
        .unwrap();
        assert_eq!(r.url.query(), Some("img.x=3&img.y=4"));
    }

    #[test]
    fn post_bodies() {
        let r = submit(
            "<form method=POST action=/login><input name=user value=me>\
             <input type=password name=pw value='p&w'></form>",
            None,
        )
        .unwrap();
        assert_eq!(r.url.as_str(), "https://example.test/login");
        let post = r.post.unwrap();
        assert_eq!(&*post.body, b"user=me&pw=p%26w");
        assert_eq!(post.content_type, "application/x-www-form-urlencoded");
        let r = submit(
            "<form method=post enctype=text/plain><input name=a value=1></form>",
            None,
        )
        .unwrap();
        let post = r.post.unwrap();
        assert_eq!(
            (&*post.body, post.content_type.as_str()),
            (&b"a=1\r\n"[..], "text/plain")
        );
        let r = submit(
            "<form method=post enctype=multipart/form-data><input name=a value=1></form>",
            None,
        )
        .unwrap();
        let post = r.post.unwrap();
        let boundary = post
            .content_type
            .strip_prefix("multipart/form-data; boundary=")
            .unwrap();
        let body = String::from_utf8(post.body.to_vec()).unwrap();
        assert!(body.starts_with(&format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"a\"\r\n\r\n1\r\n"
        )));
        assert!(body.ends_with(&format!("--{boundary}--\r\n")));
    }

    #[test]
    fn submitter_attributes_override_the_form() {
        let r = submit(
            "<form action=a method=post><input name=q value=1>\
             <button id=b formaction=other formmethod=get>go</button></form>",
            Some("b"),
        )
        .unwrap();
        assert_eq!(r.post, None);
        assert_eq!(r.url.as_str(), "https://example.test/dir/other?q=1");
    }

    #[test]
    fn unsupported_actions() {
        assert!(submit("<form action='mailto:a@b.test'></form>", None).is_none());
        assert!(submit("<form action='javascript:x()'></form>", None).is_none());
        assert!(submit("<form method=dialog></form>", None).is_none());
        // An empty action submits to the document URL.
        let r = submit("<form></form>", None).unwrap();
        assert_eq!(r.url.as_str(), "https://example.test/dir/page?");
    }

    #[test]
    fn methods_are_not_trimmed_and_dirname_reports_the_direction() {
        let r = submit(
            "<form method=' post' action=a><input name=a value=x dirname=d dir=rtl>\
             <input type=hidden name=h value=1 dirname=hd><input type=checkbox name=c checked dirname=no>\
             <div dir=rtl><textarea name=t dirname=td>v</textarea></div></form>",
            None,
        )
        .unwrap();
        assert_eq!(r.post, None);
        assert_eq!(r.url.query(), Some("a=x&d=rtl&h=1&hd=ltr&c=on&t=v&td=rtl"));
        // Invalid and empty `dir` values are skipped (Chromium sends rtl).
        let r = submit(
            "<div dir=rtl><form><input name=a dirname=d dir=foo><input name=b dirname=e dir=''>\
             <input name=c dirname=f dir=AUTO></form></div>",
            None,
        )
        .unwrap();
        assert_eq!(r.url.query(), Some("a=&d=rtl&b=&e=rtl&c=&f=ltr"));
    }

    #[test]
    fn accept_charset_picks_the_encoding() {
        let r = submit(
            "<form accept-charset='bogus windows-1252'><input name=a value='é'></form>",
            None,
        )
        .unwrap();
        assert_eq!(r.url.query(), Some("a=%E9"));
        // Commas separate labels too (Chromium sends %E8 for this).
        let r = submit(
            "<form accept-charset='iso-8859-2,utf-8'><input name=a value='\u{10d}'></form>",
            None,
        )
        .unwrap();
        assert_eq!(r.url.query(), Some("a=%E8"));
    }

    #[test]
    fn default_button_and_implicit_submission_fields() {
        let doc = parse_html(
            "<form id=f><input id=t><input type=checkbox><button id=b type=button>x</button>\
             <input id=s type=submit><input type=number></form>",
        );
        let forms = Forms::new(&doc);
        let f = doc.element_by_id("f").unwrap();
        assert_eq!(default_button(&doc, &forms, f), doc.element_by_id("s"));
        assert_eq!(blocking_fields(&doc, &forms, f), 2);
    }
}
