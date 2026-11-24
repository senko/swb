//! Tests of the animation input of the cascade (ADR 0044 section 3): the
//! records that a style pass creates, updates and cancels, and the animated
//! values at the timeline's time.

use std::cell::RefCell;
use std::sync::Arc;

use swb_css::MediaEnvironment;
use swb_dom::{Document, parse_html};

use super::compute_styles_animated;
use super::targeted::restyle_animated;
use super::test_util::{base, node, stylist};
use crate::ComputedStyle;
use crate::animation::{AnimationOwner, DocumentAnimations};
use crate::container::ContainerSizes;
use crate::dom_changes::DomChanges;
use crate::element::ElementStates;
use crate::style_map::{PseudoKind, StyleMap};
use crate::stylist::Stylist;
use crate::values::{LengthPercentage, Size};

use super::targeted::{Restyle, RestyleInputs};

/// A document with `css`, its stylist and animations, styled once.
struct Fixture {
    doc: Document,
    stylist: Stylist,
    animations: RefCell<DocumentAnimations>,
    map: StyleMap,
}

impl Fixture {
    fn new(html: &str, css: &str) -> Self {
        let doc = parse_html(html);
        let stylist = stylist(&doc, css);
        let animations = RefCell::new(DocumentAnimations::new(0.0));
        let map = StyleMap::with_capacity(0);
        let mut fixture = Fixture {
            doc,
            stylist,
            animations,
            map,
        };
        fixture.full();
        fixture
    }

    /// A full style pass.
    fn full(&mut self) {
        self.map = compute_styles_animated(
            &self.doc,
            &self.stylist,
            &MediaEnvironment::default(),
            &ElementStates::default(),
            &base(),
            &ContainerSizes::new(),
            None,
            &self.animations,
            None,
        );
    }

    /// The update of a rendering update at `time`, then the restyle of
    /// the marked owners.
    fn tick(&mut self, time: f64) {
        self.animations.borrow_mut().update(time);
        let marked = self.animations.borrow_mut().take_marked();
        let env = MediaEnvironment::default();
        let sizes = ContainerSizes::new();
        let inputs = RestyleInputs {
            doc: &self.doc,
            stylist: &self.stylist,
            env: &env,
            base_url: &base(),
            sizes: &sizes,
        };
        let states = ElementStates::default();
        let outcome = restyle_animated(
            &inputs,
            &states,
            &states,
            &DomChanges::default(),
            &marked,
            Some(&self.animations),
            None,
            &mut self.map,
        );
        assert!(matches!(outcome, Restyle::Done { .. }), "{outcome:?}");
    }

    fn style(&self, id: &str) -> &Arc<ComputedStyle> {
        self.map.get(node(&self.doc, id)).expect("a style")
    }

    fn ids(&self, id: &str) -> Vec<u64> {
        self.animations
            .borrow()
            .ids_of(AnimationOwner {
                node: node(&self.doc, id),
                pseudo: None,
            })
            .to_vec()
    }
}

#[test]
fn values_follow_the_timeline() {
    let mut f = Fixture::new(
        "<div id=a></div>",
        "#a { animation: k 1s linear } @keyframes k { from { opacity: 0 } to { opacity: 1 } }",
    );
    // Created at time 0, pending: the value at current time 0.
    assert_eq!(f.style("a").opacity, 0.0);
    f.tick(0.0);
    f.tick(250.0);
    assert!((f.style("a").opacity - 0.25).abs() < 1e-6);
    f.tick(1000.0);
    // Finished without a fill: the underlying value.
    assert_eq!(f.style("a").opacity, 1.0);
}

#[test]
fn the_implicit_keyframe_takes_the_underlying_value() {
    let mut f = Fixture::new(
        "<div id=a></div>",
        "#a { width: 100px; animation: k 1s linear } @keyframes k { to { width: 200px } }",
    );
    f.tick(0.0);
    f.tick(500.0);
    assert_eq!(
        f.style("a").width,
        Size::LengthPercentage(LengthPercentage::Px(150.0))
    );
}

#[test]
fn important_declarations_are_not_animated() {
    let mut f = Fixture::new(
        "<div id=a></div>",
        "#a { opacity: 0.5 !important; animation: k 1s linear } \
         @keyframes k { from { opacity: 0 } to { opacity: 1 } }",
    );
    f.tick(0.0);
    f.tick(250.0);
    assert_eq!(f.style("a").opacity, 0.5);
}

#[test]
fn children_inherit_animated_values_and_em_follows_the_font_size() {
    let mut f = Fixture::new(
        "<div id=a><p id=b></p></div>",
        "#a { animation: k 1s linear } #b { width: 2em } \
         @keyframes k { from { font-size: 10px; color: rgb(0, 0, 0) } \
         to { font-size: 20px; color: rgb(200, 0, 0) } }",
    );
    f.tick(0.0);
    f.tick(500.0);
    assert_eq!(f.style("a").font_size, 15.0);
    assert_eq!(f.style("b").font_size, 15.0);
    assert_eq!(f.style("b").color, f.style("a").color);
    assert_eq!(f.style("b").color.r, 100);
    assert_eq!(
        f.style("b").width,
        Size::LengthPercentage(LengthPercentage::Px(30.0))
    );
}

#[test]
fn var_follows_an_animated_custom_property() {
    let mut f = Fixture::new(
        "<div id=a></div>",
        "@property --len { syntax: '<length>'; inherits: false; initial-value: 0px } \
         #a { width: var(--len); animation: k 1s linear } \
         @keyframes k { from { --len: 10px } to { --len: 30px } }",
    );
    f.tick(0.0);
    f.tick(500.0);
    assert_eq!(
        f.style("a").width,
        Size::LengthPercentage(LengthPercentage::Px(20.0))
    );
}

#[test]
fn a_record_keeps_its_identity_while_its_name_stays() {
    let mut f = Fixture::new(
        "<div id=a></div>",
        "#a { animation: k 1s } @keyframes k { to { opacity: 0 } } @keyframes j { to { opacity: 0 } }",
    );
    f.animations.borrow_mut().track_released();
    let first = f.ids("a");
    assert_eq!(first.len(), 1);
    f.full();
    assert_eq!(f.ids("a"), first);
    // A new list with the old name first: the old record stays, a new one
    // comes.
    f.stylist = stylist(
        &f.doc,
        "#a { animation: k 2s, j 1s } @keyframes k { to { opacity: 0 } } \
         @keyframes j { to { opacity: 0 } }",
    );
    f.full();
    let both = f.ids("a");
    assert_eq!(both.len(), 2);
    assert_eq!(both[0], first[0]);
    // An unknown name makes no animation; `none` cancels.
    f.stylist = stylist(&f.doc, "#a { animation: nothing 1s }");
    f.full();
    assert_eq!(f.ids("a"), Vec::<u64>::new());
    assert_eq!(f.animations.borrow_mut().take_released().len(), 2);
}

#[test]
fn display_none_has_animations_of_display_only() {
    let f = Fixture::new(
        "<div id=a></div><div id=b><p id=c></p></div>",
        "#a { display: none; animation: k 1s } #b { display: none } #c { animation: k 1s } \
         @keyframes k { to { opacity: 0 } }",
    );
    assert_eq!(f.ids("a"), Vec::<u64>::new());
    assert_eq!(f.ids("c"), Vec::<u64>::new());
    let f = Fixture::new(
        "<div id=a></div>",
        "#a { display: none; animation: k 1s paused } @keyframes k { from { display: block } }",
    );
    assert_eq!(f.ids("a").len(), 1);
    assert_eq!(f.style("a").display, crate::values::Display::Block);
}

#[test]
fn pseudo_elements_with_a_box_have_animations() {
    let mut f = Fixture::new(
        "<div id=a></div>",
        "#a::before { content: 'x'; animation: k 1s linear } \
         @keyframes k { from { opacity: 0 } to { opacity: 1 } }",
    );
    let owner = AnimationOwner {
        node: node(&f.doc, "a"),
        pseudo: Some(PseudoKind::Before),
    };
    assert_eq!(f.animations.borrow().ids_of(owner).len(), 1);
    f.tick(0.0);
    f.tick(500.0);
    let before = f
        .map
        .pseudo(node(&f.doc, "a"), PseudoKind::Before)
        .expect("a ::before");
    assert!((before.opacity - 0.5).abs() < 1e-6);
}

#[test]
fn animations_off_resets_the_properties() {
    let doc = parse_html("<div id=a></div>");
    let mut st = stylist(
        &doc,
        "#a { animation: k 1s; transition: opacity 1s } @keyframes k { to { opacity: 0 } }",
    );
    st.set_animations(false);
    let animations = RefCell::new(DocumentAnimations::new(0.0));
    let map = compute_styles_animated(
        &doc,
        &st,
        &MediaEnvironment::default(),
        &ElementStates::default(),
        &base(),
        &ContainerSizes::new(),
        None,
        &animations,
        None,
    );
    let style = map.get(node(&doc, "a")).expect("a style");
    assert!(style.animations.is_none());
    // Every `animation-*` and `transition-*` longhand is initial.
    let mut reset = (**style).clone();
    super::animated::reset_animation_properties(&mut reset, &ComputedStyle::initial());
    assert_eq!(reset, **style);
    assert!(animations.borrow().is_empty());
}

#[test]
fn only_animation_names_stop_the_sharing_of_styles() {
    // A reduced-motion reset sets `animation-*` values without a name: the
    // siblings still share a style. With a name each has its own.
    let f = Fixture::new(
        "<p id=a class=x></p><p id=b class=x></p><p id=c class=y></p><p id=d class=y></p>",
        ".x { animation-duration: 0.01ms !important } .y { animation: k 1s }\
         @keyframes k { to { opacity: 0 } }",
    );
    assert!(Arc::ptr_eq(f.style("a"), f.style("b")));
    assert!(!Arc::ptr_eq(f.style("c"), f.style("d")));
    assert_eq!(f.ids("a").len(), 0);
    assert_eq!(f.ids("c").len(), 1);
    assert_eq!(f.ids("d").len(), 1);
}
