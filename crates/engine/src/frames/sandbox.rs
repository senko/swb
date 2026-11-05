//! The `sandbox` attribute of `iframe` (HTML "sandboxing", "parse a
//! sandboxing directive"; ADR 0031 section 3).
//! <https://html.spec.whatwg.org/multipage/browsers.html#parse-a-sandboxing-directive>
//!
//! swb uses two flags: without `allow-same-origin` the frame's document
//! has an opaque origin; without `allow-scripts` its scripts do not run
//! (frame documents run no scripts before M9 feature 12, so this flag
//! waits for it). The other tokens are parsed and ignored: they gate
//! forms, popups, top-level navigation, modals and downloads, which swb
//! either lacks or blocks for every cross-origin frame.

/// The sandboxing flags of a frame that swb uses.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Sandbox {
    /// The container has a `sandbox` attribute.
    pub sandboxed: bool,
    /// The `allow-same-origin` token (keeps the document's origin).
    pub allow_same_origin: bool,
    /// The `allow-scripts` token.
    pub allow_scripts: bool,
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
        }
    }

    /// The flags of a frame whose container has the flags `self` inside a
    /// frame with the flags `parent`: the sandboxing flags set of the
    /// container's attribute and of the parent frame, joined (HTML
    /// "determining the creation sandboxing flags"). A token allows only
    /// if every sandboxed level allows it.
    #[must_use]
    pub fn inherit(self, parent: Sandbox) -> Sandbox {
        let allows = |s: Sandbox, own: bool| !s.sandboxed || own;
        let sandboxed = self.sandboxed || parent.sandboxed;
        // The tokens mean something only in a sandbox.
        Sandbox {
            sandboxed,
            allow_same_origin: sandboxed
                && allows(self, self.allow_same_origin)
                && allows(parent, parent.allow_same_origin),
            allow_scripts: sandboxed
                && allows(self, self.allow_scripts)
                && allows(parent, parent.allow_scripts),
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
