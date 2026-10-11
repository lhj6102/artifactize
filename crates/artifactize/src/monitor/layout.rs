//! Reactive panes: a pure function from the body area and the focused level to the visible
//! panes. No rendering or I/O happens here.
use super::Pane;
use ratatui::layout::Rect;

/// Terminal widths from this breakpoint use the widest side panes.
const WIDE: u16 = 140;
/// Terminal widths from this breakpoint use medium side panes.
const MEDIUM: u16 = 100;
/// Side-pane widths below `MEDIUM`, from `MEDIUM`, and from `WIDE`, borders included.
const SIDE_WIDTHS: [u16; 3] = [16, 28, 36];
/// Drill-down order: Scope → Runs → Run (tree) → Detail.
const LEVELS: [Pane; 4] = [
    Pane::Repositories,
    Pane::Runs,
    Pane::Artifacts,
    Pane::Detail,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Density {
    /// The focused level.
    Full,
    /// The level left of focus, as narrow context.
    Compact,
    /// The level right of focus, as a preview of what Enter opens.
    Preview,
    /// Only in the breadcrumb.
    Hidden,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Plan {
    /// Visible panes from left to right.
    pub panes: Vec<(Pane, Rect, Density)>,
}

impl Plan {
    pub fn density(&self, pane: Pane) -> Density {
        self.panes
            .iter()
            .find(|(visible, _, _)| *visible == pane)
            .map_or(Density::Hidden, |(_, _, density)| *density)
    }
}

pub(super) fn level(pane: Pane) -> usize {
    LEVELS
        .iter()
        .position(|level| *level == pane)
        .expect("every pane is a level")
}

/// One level to the left; the first level has none.
pub(super) fn back(pane: Pane) -> Pane {
    LEVELS[level(pane).saturating_sub(1)]
}

/// The focused monitor level fills the width between its immediate neighbours.
pub fn plan(area: Rect, focus: Pane) -> Plan {
    let panes = columns(area, LEVELS.len(), level(focus))
        .into_iter()
        .map(|(index, area, density)| (LEVELS[index], area, density))
        .collect();
    Plan { panes }
}

/// Visible levels of any drill-down screen, by index, left to right. Immediate neighbours
/// always remain: Compact left of the Full focus and Preview right of it. Both side panes
/// use the breakpoint's fixed width; Full fills the rest. Only the first and last levels
/// have two panes. A body too small even for the sides clips them equally without dropping
/// levels, including when the terminal has a zero-sized body.
pub(crate) fn columns(area: Rect, levels: usize, focused: usize) -> Vec<(usize, Rect, Density)> {
    assert!(focused < levels, "focus must be a drill-down level");
    let left = focused.checked_sub(1);
    let right = (focused + 1 < levels).then_some(focused + 1);
    let side_count = u16::from(left.is_some()) + u16::from(right.is_some());
    let side = match area.width {
        WIDE.. => SIDE_WIDTHS[2],
        MEDIUM..WIDE => SIDE_WIDTHS[1],
        _ => SIDE_WIDTHS[0],
    };
    let side = area
        .width
        .checked_div(side_count)
        .map_or(0, |available| side.min(available));
    let full = area.width - side * side_count;
    let mut panes = Vec::new();
    let mut x = area.x;
    let mut push = |level, width, density| {
        panes.push((level, Rect::new(x, area.y, width, area.height), density));
        x += width;
    };
    if let Some(level) = left {
        push(level, side, Density::Compact);
    }
    push(focused, full, Density::Full);
    if let Some(level) = right {
        push(level, side, Density::Preview);
    }
    panes
}

#[cfg(test)]
mod tests {
    use super::*;
    use Density::{Compact, Full, Preview};
    use Pane::{Artifacts as Tree, Detail, Repositories as Scope, Runs};

    fn widths(width: u16, focus: Pane) -> Vec<(Pane, u16, Density)> {
        let plan = plan(Rect::new(5, 1, width, 30), focus);
        let mut x = 5;
        for (_, area, _) in &plan.panes {
            assert_eq!((area.x, area.y, area.height), (x, 1, 30), "{plan:?}");
            x += area.width;
        }
        assert_eq!(x, 5 + width, "panes fill the body: {plan:?}");
        plan.panes
            .into_iter()
            .map(|(pane, area, density)| (pane, area.width, density))
            .collect()
    }

    #[test]
    fn breakpoints_choose_panes_and_widths_for_every_focus() {
        // Independent expectations at each boundary and inside each breakpoint.
        for (width, side, middle, end) in [
            (60, 16, 28, 44),
            (80, 16, 48, 64),
            (99, 16, 67, 83),
            (100, 28, 44, 72),
            (120, 28, 64, 92),
            (139, 28, 83, 111),
            (140, 36, 68, 104),
            (160, 36, 88, 124),
            (200, 36, 128, 164),
        ] {
            for (focus, expected) in [
                (Scope, vec![(Scope, end, Full), (Runs, side, Preview)]),
                (
                    Runs,
                    vec![
                        (Scope, side, Compact),
                        (Runs, middle, Full),
                        (Tree, side, Preview),
                    ],
                ),
                (
                    Tree,
                    vec![
                        (Runs, side, Compact),
                        (Tree, middle, Full),
                        (Detail, side, Preview),
                    ],
                ),
                (Detail, vec![(Tree, side, Compact), (Detail, end, Full)]),
            ] {
                assert_eq!(widths(width, focus), expected, "{width} {focus:?}");
            }
        }
    }

    #[test]
    fn tiny_bodies_keep_neighbours_and_fill_the_body_without_overflow() {
        for (width, side, full) in [(0, 0, 0), (1, 0, 1), (31, 15, 1), (32, 16, 0)] {
            assert_eq!(
                widths(width, Runs),
                [
                    (Scope, side, Compact),
                    (Runs, full, Full),
                    (Tree, side, Preview)
                ]
            );
        }
        assert_eq!(widths(0, Scope), [(Scope, 0, Full), (Runs, 0, Preview)]);
        assert_eq!(widths(15, Detail), [(Tree, 15, Compact), (Detail, 0, Full)]);
        assert_eq!(
            columns(Rect::new(5, 1, 15, 20), 1, 0),
            [(0, Rect::new(5, 1, 15, 20), Full)]
        );
    }

    #[test]
    fn density_reports_hidden_levels_and_back_never_leaves_the_first_level() {
        let wide = plan(Rect::new(0, 0, 160, 20), Detail);
        assert_eq!(wide.density(Scope), Density::Hidden);
        assert_eq!(wide.density(Runs), Density::Hidden);
        assert_eq!(wide.density(Tree), Compact);
        assert_eq!(wide.density(Detail), Full);
        assert_eq!(wide.panes[1].1.x, 36);
        assert_eq!(
            [Scope, Runs, Tree, Detail].map(back),
            [Scope, Scope, Runs, Tree]
        );
    }

    #[test]
    fn review_list_and_detail_follow_the_same_breakpoints() {
        for (width, side, full) in [
            (80, 16, 64),
            (99, 16, 83),
            (100, 28, 72),
            (120, 28, 92),
            (139, 28, 111),
            (140, 36, 104),
            (160, 36, 124),
        ] {
            for (focused, expected) in [
                (0, vec![(0, full, Full), (1, side, Preview)]),
                (1, vec![(0, side, Compact), (1, full, Full)]),
            ] {
                let panes = columns(Rect::new(5, 1, width, 20), 2, focused);
                let mut x = 5;
                for (_, area, _) in &panes {
                    assert_eq!((area.x, area.y, area.height), (x, 1, 20), "{panes:?}");
                    x += area.width;
                }
                assert_eq!(x, 5 + width, "panes fill the body: {panes:?}");
                let actual: Vec<_> = panes
                    .into_iter()
                    .map(|(level, area, density)| (level, area.width, density))
                    .collect();
                assert_eq!(actual, expected, "{width} {focused}");
            }
        }
    }
}
