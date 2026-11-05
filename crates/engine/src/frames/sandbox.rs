//! The `sandbox` attribute of `iframe` (HTML "sandboxing", "parse a
//! sandboxing directive"; ADR 0031 section 3).
//! <https://html.spec.whatwg.org/multipage/browsers.html#parse-a-sandboxing-directive>
//!
//! swb uses these flags: without `allow-same-origin` the frame's document
//! has an opaque origin; without `allow-scripts` its scripts do not run
//! (frame documents run no scripts before M9 feature 12, so this flag
//! waits for it); without `allow-forms` its forms do not submit; without
//! `allow-popups` it opens no new browsing context; the
//! `allow-top-navigation` tokens let it navigate the top level, and a
//! sandboxed frame navigates only itself and its descendants otherwise
//! (`Page::may_navigate`, measured, cases `input-target-sb-*`). The other
//! tokens are parsed and ignored: they gate modals and downloads, which
//! swb lacks in frames.

/// The sandboxing flags of a frame that swb uses.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Sandbox {
    /// The container has a `sandbox` attribute.
    pub sandboxed: bool,
    /// The `allow-same-origin` token (keeps the document's origin).
    pub allow_same_origin: bool,
    /// The `allow-scripts` token.
    pub allow_scripts: bool,
    /// The `allow-forms` token.
    pub allow_forms: bool,
    /// The `allow-popups` token.
    pub allow_popups: bool,
    /// The `allow-top-navigation` token.
    pub allow_top_navigation: bool,
    /// The `allow-top-navigation-by-user-activation` token.
    pub allow_top_navigation_by_user_activation: bool,
}

impl Sandbox {
    /// The flags of a `sandbox` attribute value; `None` (no attribute)
    /// is no sandbox. Tokens are separated by ASCII white space and
    /// compared without ASCII case.
    pub fn parse(value: Option<&str>) -> Sandbox {
        let Some(value) = value else {
            return Sandbox::default();
        };
        let has = |token: &str| {
            value
                .split_ascii_whitespace()
                .any(|t| t.eq_ignore_ascii_case(token))
        };
        Sandbox {
            sandboxed: true,
            allow_same_origin: has("allow-same-origin"),
            allow_scripts: has("allow-scripts"),
            allow_forms: has("allow-forms"),
            allow_popups: has("allow-popups"),
            allow_top_navigation: has("allow-top-navigation"),
            allow_top_navigation_by_user_activation: has("allow-top-navigation-by-user-activation"),
        }
    }

    /// The flags of a frame whose container has the flags `self` inside a
    /// frame with the flags `parent`: the sandboxing flags set of the
    /// container's attribute and of the parent frame, joined (HTML
    /// "determining the creation sandboxing flags"). A token allows only
    /// if every sandboxed level allows it.
    #[must_use]
    pub fn inherit(self, parent: Sandbox) -> Sandbox {
        let sandboxed = self.sandboxed || parent.sandboxed;
        // The tokens mean something only in a sandbox.
        let token = |own: fn(Sandbox) -> bool| {
            let allows = |s: Sandbox| !s.sandboxed || own(s);
            sandboxed && allows(self) && allows(parent)
        };
        Sandbox {
            sandboxed,
            allow_same_origin: token(|s| s.allow_same_origin),
            allow_scripts: token(|s| s.allow_scripts),
            allow_forms: token(|s| s.allow_forms),
            allow_popups: token(|s| s.allow_popups),
            allow_top_navigation: token(|s| s.allow_top_navigation),
            allow_top_navigation_by_user_activation: token(|s| {
                s.allow_top_navigation_by_user_activation
            }),
        }
    }

    /// True if the document gets an opaque origin (HTML "sandboxed origin
    /// browsing context flag").
    pub fn opaque_origin(self) -> bool {
        self.sandboxed && !self.allow_same_origin
    }

    /// True if scripts may run (HTML "sandboxed scripts browsing context
    /// flag" not set).
    pub fn scripts_allowed(self) -> bool {
        !self.sandboxed || self.allow_scripts
    }

    /// True if forms may submit (HTML "sandboxed forms browsing context
    /// flag" not set).
    pub fn forms_allowed(self) -> bool {
        !self.sandboxed || self.allow_forms
    }

    /// True if a link or form may open a new browsing context (HTML
    /// "sandboxed auxiliary navigation browsing context flag" not set).
    pub fn popups_allowed(self) -> bool {
        !self.sandboxed || self.allow_popups
    }

    /// True if a sandbox lets the frame navigate the top level. Every
    /// navigation that reaches this check comes from the user's input
    /// while frame documents run no scripts, so the
    /// `allow-top-navigation-by-user-activation` token counts; scripts in
    /// frames (feature 12) need the activation.
    pub fn top_navigation_allowed(self) -> bool {
        !self.sandboxed || self.allow_top_navigation || self.allow_top_navigation_by_user_activation
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens() {
        assert_eq!(Sandbox::parse(None), Sandbox::default());
        let empty = Sandbox::parse(Some(""));
        assert!(empty.opaque_origin() && !empty.scripts_allowed());
        let scripts = Sandbox::parse(Some(" ALLOW-SCRIPTS\tallow-forms "));
        assert!(scripts.opaque_origin() && scripts.scripts_allowed());
        let same = Sandbox::parse(Some("allow-same-origin"));
        assert!(!same.opaque_origin() && !same.scripts_allowed());
        assert!(!Sandbox::parse(None).opaque_origin());
    }

    #[test]
    fn navigation_and_form_tokens() {
        let free = Sandbox::parse(None);
        assert!(free.forms_allowed() && free.popups_allowed() && free.top_navigation_allowed());
        let closed = Sandbox::parse(Some("allow-same-origin"));
        assert!(
            !closed.forms_allowed() && !closed.popups_allowed() && !closed.top_navigation_allowed()
        );
        assert!(Sandbox::parse(Some("allow-forms")).forms_allowed());
        assert!(Sandbox::parse(Some("allow-popups")).popups_allowed());
        assert!(Sandbox::parse(Some("allow-top-navigation")).top_navigation_allowed());
        let activation = Sandbox::parse(Some("allow-top-navigation-by-user-activation"));
        assert!(activation.top_navigation_allowed());
        // A level without the token closes it for the frames below.
        let forms = Sandbox::parse(Some("allow-forms"));
        assert!(!forms.inherit(closed).forms_allowed());
        assert!(free.inherit(forms).forms_allowed());
        assert!(!free.inherit(closed).top_navigation_allowed());
    }

    #[test]
    fn nested_flags_join() {
        let none = Sandbox::default();
        let scripts = Sandbox::parse(Some("allow-scripts"));
        let same = Sandbox::parse(Some("allow-same-origin allow-scripts"));
        // No attribute inside a sandbox: the parent's flags.
        assert_eq!(none.inherit(scripts), scripts);
        assert!(none.inherit(scripts).opaque_origin());
        assert_eq!(scripts.inherit(none), scripts);
        assert_eq!(none.inherit(none), none);
        // Both sandboxed: a token allows only if both allow it.
        let joined = same.inherit(scripts);
        assert!(joined.opaque_origin() && joined.scripts_allowed());
        assert!(!same.inherit(same).opaque_origin());
    }
}
