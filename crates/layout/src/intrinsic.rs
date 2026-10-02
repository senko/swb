//! Intrinsic (content-based) widths: min-content and max-content.
//!
//! <https://www.w3.org/TR/css-sizing-3/#intrinsic-sizes>. Used for
//! shrink-to-fit widths (floats, inline-blocks, absolutely positioned
//! boxes) and for flex base sizes.

use swb_style::{BoxSizing, ComputedStyle, Size};

use crate::LayoutContext;
use crate::block::BoxEdges;
use crate::box_tree::{BlockContainer, BlockLevelBox, IndependentBox, IndependentContents};
use crate::inline;

/// Min-content and max-content widths.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct ContentSizes {
    pub(crate) min: f32,
    pub(crate) max: f32,
}

impl ContentSizes {
    fn max_with(self, other: ContentSizes) -> ContentSizes {
        ContentSizes {
            min: self.min.max(other.min),
            max: self.max.max(other.max),
        }
    }

    fn add(self, v: f32) -> ContentSizes {
        ContentSizes {
            min: self.min + v,
            max: self.max + v,
        }
    }
}

/// Content-box sizes of an independent box's contents.
pub(crate) fn independent_content_sizes(
    ctx: &mut LayoutContext<'_>,
    ib: &IndependentBox,
) -> ContentSizes {
    match &ib.contents {
        IndependentContents::Flow(container) => {
            container_content_sizes(ctx, container, &ib.base.style)
        }
        IndependentContents::Flex(items) => {
            let row = ib.base.style.flex_direction.is_row();
            let mut sizes = ContentSizes::default();
            let in_flow = items
                .iter()
                .filter(|item| !item.base.style.is_absolutely_positioned());
            for item in in_flow {
                let s = independent_outer_sizes(ctx, item);
                if row {
                    sizes.min = sizes.min.max(s.min);
                    sizes.max += s.max;
                } else {
                    sizes = sizes.max_with(s);
                }
            }
            sizes
        }
        IndependentContents::Replaced(r) => {
            let w = r.natural_size.map_or(0.0, |(w, _)| w);
            ContentSizes { min: w, max: w }
        }
    }
}

/// Margin-box sizes of an independent box, honoring a fixed `width`.
pub(crate) fn independent_outer_sizes(
    ctx: &mut LayoutContext<'_>,
    ib: &IndependentBox,
) -> ContentSizes {
    let style = &ib.base.style;
    if let IndependentContents::Replaced(r) = &ib.contents {
        let edges = BoxEdges::resolve(style, 0.0);
        let cb = crate::block::ContainingBlock {
            width: 0.0,
            height: None,
        };
        let (w, _) = crate::replaced::used_size(style, r, cb, &edges);
        let outer = w + edges.sum().horizontal() + fixed_margins(style);
        return ContentSizes {
            min: outer,
            max: outer,
        };
    }
    outer_sizes(style, || independent_content_sizes(ctx, ib))
}

fn block_level_outer_sizes(ctx: &mut LayoutContext<'_>, b: &BlockLevelBox) -> ContentSizes {
    match b {
        BlockLevelBox::Block { base, contents, .. } => outer_sizes(&base.style, || {
            container_content_sizes(ctx, contents, &base.style)
        }),
        BlockLevelBox::Independent(ib) | BlockLevelBox::Float(ib) => {
            independent_outer_sizes(ctx, ib)
        }
        BlockLevelBox::AbsolutelyPositioned(_) => ContentSizes::default(),
    }
}

pub(crate) fn container_content_sizes(
    ctx: &mut LayoutContext<'_>,
    container: &BlockContainer,
    style: &ComputedStyle,
) -> ContentSizes {
    match container {
        BlockContainer::Inline(ifc) => inline::content_sizes(ctx, ifc, style),
        BlockContainer::Blocks(children) => {
            let mut sizes = ContentSizes::default();
            for child in children {
                sizes = sizes.max_with(block_level_outer_sizes(ctx, child));
            }
            sizes
        }
    }
}

/// Adds padding, border and fixed margins to content sizes; a fixed
/// `width` replaces the content sizes.
fn outer_sizes(style: &ComputedStyle, content: impl FnOnce() -> ContentSizes) -> ContentSizes {
    let edges = BoxEdges::resolve(style, 0.0);
    let edge_sum = edges.sum().horizontal();
    let inner = match &style.width {
        Size::LengthPercentage(lp) if !lp.has_percentage() => {
            let w = lp.resolve(0.0);
            let w = match style.box_sizing {
                BoxSizing::ContentBox => w,
                BoxSizing::BorderBox => (w - edge_sum).max(0.0),
            };
            ContentSizes { min: w, max: w }
        }
        _ => content(),
    };
    let min_width = match &style.min_width {
        Size::LengthPercentage(lp) if !lp.has_percentage() => lp.resolve(0.0),
        _ => 0.0,
    };
    let max_width = match &style.max_width {
        swb_style::MaxSize::LengthPercentage(lp) if !lp.has_percentage() => lp.resolve(0.0),
        _ => f32::INFINITY,
    };
    let clamp = |v: f32| v.min(max_width).max(min_width);
    ContentSizes {
        min: clamp(inner.min),
        max: clamp(inner.max),
    }
    .add(edge_sum + fixed_margins(style))
}

fn fixed_margins(style: &ComputedStyle) -> f32 {
    let m = |lp: &swb_style::LengthPercentageOrAuto| match lp.non_auto() {
        Some(lp) if !lp.has_percentage() => lp.resolve(0.0),
        _ => 0.0,
    };
    m(&style.margin_left) + m(&style.margin_right)
}
