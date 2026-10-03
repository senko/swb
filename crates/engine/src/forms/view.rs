//! What layout needs from the form controls: the kind of each control, the
//! text it shows (a password as bullets, the placeholder, a label, the
//! selected option), the caret and the scroll offset of its text.

use swb_dom::{Document, ElementData, NodeId, local_name};
use swb_layout::{Control, ControlKind, FormControls, MAX_SELECT_OPTIONS, Point};

use super::{ControlState, ControlType, Forms, InputType, option_label, parse_non_negative};

/// The bullet that shows one character of a password.
const BULLET: char = '\u{2022}';

/// The layout view of the controls of a document.
pub(crate) struct LayoutControls<'a> {
    pub(crate) doc: &'a Document,
    pub(crate) forms: &'a Forms,
    pub(crate) focus: Option<NodeId>,
}

impl FormControls for LayoutControls<'_> {
    fn is_control(&self, node: NodeId) -> bool {
        self.forms
            .control_type(node)
            .is_some_and(|t| t != ControlType::Input(InputType::Hidden))
    }

    fn control(&self, node: NodeId) -> Option<Control> {
        self.forms.layout_control(self.doc, node, self.focus)
    }
}

impl Forms {
    /// The control as layout sees it. `focus` is the focused element.
    pub(crate) fn layout_control(
        &self,
        doc: &Document,
        node: NodeId,
        focus: Option<NodeId>,
    ) -> Option<Control> {
        let state = self.get(node)?;
        let e = doc.element(node)?;
        let mut control = Control {
            kind: ControlKind::Button,
            text: String::new(),
            placeholder: false,
            options: Vec::new(),
            caret: None,
            focus: None,
            scroll: Point::default(),
            checked: state.checked,
            disabled: state.disabled,
        };
        match state.ty {
            ControlType::Input(InputType::Hidden) => return None,
            ControlType::Input(InputType::Checkbox) => control.kind = ControlKind::Checkbox,
            ControlType::Input(InputType::Radio) => control.kind = ControlKind::Radio,
            ControlType::Input(t) if !t.is_text() => control.text = button_label(e, t),
            ControlType::Input(t) => {
                // `size` applies to some types only (Chromium ignores it
                // for numbers too).
                let size = if t.has_size() {
                    positive_attr(e, "size", 20)
                } else {
                    20
                };
                control.kind = ControlKind::TextField { size };
                text_view(e, state, &mut control, focus == Some(node));
            }
            ControlType::TextArea => {
                control.kind = ControlKind::TextArea {
                    cols: positive_attr(e, "cols", 20),
                    rows: positive_attr(e, "rows", 2),
                };
                text_view(e, state, &mut control, focus == Some(node));
                // A text area keeps its scroll position without the focus.
                control.scroll = state.scroll;
            }
            ControlType::Select => {
                control.kind = ControlKind::Select;
                select_view(doc, state, &mut control);
            }
            ControlType::Button(_) => control.kind = ControlKind::ButtonElement,
        }
        Some(control)
    }
}

/// The label of an `input` button.
fn button_label(e: &ElementData, ty: InputType) -> String {
    let label = match ty {
        InputType::Submit => e.attr("value").unwrap_or("Submit"),
        InputType::Reset => e.attr("value").unwrap_or("Reset"),
        InputType::Image => e
            .attr("alt")
            .or_else(|| e.attr("value"))
            .unwrap_or("Submit"),
        InputType::File => "Choose File",
        _ => e.attr("value").unwrap_or(""),
    };
    label.to_owned()
}

/// Fills the text, placeholder, caret and scroll of a text control. Only
/// a focused control that can be edited has a caret.
fn text_view(e: &ElementData, state: &ControlState, control: &mut Control, focused: bool) {
    let password = state.ty == ControlType::Input(InputType::Password);
    let value = state.edit.text();
    if value.is_empty() {
        if let Some(placeholder) = e.attr("placeholder").filter(|p| !p.is_empty()) {
            control.text = if state.ty == ControlType::TextArea {
                placeholder.replace("\r\n", "\n").replace('\r', "\n")
            } else {
                placeholder.replace(['\r', '\n'], "")
            };
            control.placeholder = true;
        }
    } else if password {
        control.text = BULLET.to_string().repeat(value.chars().count());
    } else {
        value.clone_into(&mut control.text);
    }
    let editable = !control.disabled && !e.has_attr("readonly");
    if focused && editable && state.edit.selection().is_empty() {
        control.caret = Some(if control.placeholder {
            0
        } else {
            display_offset(value, state.edit.cursor(), password)
        });
    }
    if focused {
        control.scroll = state.scroll;
        // With the placeholder shown, the start of the field stays visible.
        control.focus = Some(if control.placeholder {
            0
        } else {
            display_offset(value, state.edit.cursor(), password)
        });
    }
}

/// Fills the label and the option labels of a select. Options in an
/// `optgroup` are indented by four spaces (Chromium measures them so).
fn select_view(doc: &Document, state: &ControlState, control: &mut Control) {
    control.text = state
        .options
        .iter()
        .find(|o| o.selected)
        .map(|o| option_label(doc, o.node))
        .unwrap_or_default();
    control.options = state
        .options
        .iter()
        .take(MAX_SELECT_OPTIONS)
        .map(|o| {
            let label = option_label(doc, o.node);
            let in_group = doc
                .parent(o.node)
                .is_some_and(|p| doc.is_html_element(p, &local_name!("optgroup")));
            if in_group {
                format!("    {label}")
            } else {
                label
            }
        })
        .collect();
}

/// A positive integer attribute (`size`, `cols`, `rows`), or `default`.
fn positive_attr(e: &ElementData, name: &str, default: u32) -> u32 {
    e.attr(name)
        .and_then(parse_non_negative)
        .filter(|&v| v > 0)
        .unwrap_or(default)
}

/// The offset in the shown text of byte offset `offset` of the value: the
/// same offset, or for a password the offset of the bullet of that
/// character.
pub(crate) fn display_offset(value: &str, offset: usize, password: bool) -> usize {
    if !password {
        return offset;
    }
    let chars = value.get(..offset).map_or(0, |v| v.chars().count());
    chars * BULLET.len_utf8()
}

/// The byte offset in the value of offset `offset` in the shown text (the
/// inverse of [`display_offset`]).
pub(crate) fn value_offset(value: &str, offset: usize, password: bool) -> usize {
    if !password {
        return value.floor_char_boundary(offset.min(value.len()));
    }
    let index = offset / BULLET.len_utf8();
    value
        .char_indices()
        .nth(index)
        .map_or(value.len(), |(i, _)| i)
}

#[cfg(test)]
mod tests {
    use super::*;
    use swb_dom::parse_html;

    #[test]
    fn password_offsets() {
        assert_eq!(display_offset("aččb", 5, true), 9);
        assert_eq!(value_offset("aččb", 9, true), 5);
        assert_eq!(value_offset("ab", 99, true), 2);
        assert_eq!(value_offset("ač", 2, false), 1);
    }

    #[test]
    fn read_only_fields_have_no_caret() {
        let doc = parse_html("<input id=r readonly value=x><input id=e value=y type=password>");
        let mut forms = Forms::new(&doc);
        let r = doc.element_by_id("r").expect("r");
        let e = doc.element_by_id("e").expect("e");
        assert_eq!(
            forms.layout_control(&doc, r, Some(r)).expect("r").caret,
            None
        );
        // The caret starts at the start; at the end of a password, it is
        // after the bullet (3 bytes).
        let password = forms.layout_control(&doc, e, Some(e)).expect("e");
        assert_eq!(password.text, "\u{2022}");
        assert_eq!(password.caret, Some(0));
        forms.get_mut(e).expect("e").edit.text_end(false);
        let password = forms.layout_control(&doc, e, Some(e)).expect("e");
        assert_eq!(password.caret, Some(3));
    }
}
