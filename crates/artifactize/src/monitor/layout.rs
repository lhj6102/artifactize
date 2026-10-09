//! Reactive panes: a pure function from the body area, the focused level and natural widths
//! to the visible panes. No rendering or I/O happens here.
use super::Pane;
use ratatui::layout::Rect;

/// From this width up to three panes are visible: Compact, Full and Preview.
pub(super) const WIDE: u16 = 140;
/// From this width two panes are visible; below it only the focused pane.
pub(super) const MEDIUM: u16 = 100;
/// Fixed Compact widths of Scope, Runs and the Run tree, borders included.
const COMPACT: [u16; 3] = [20, 18, 30];
/// The Full pane keeps at least this width before a neighbour is added.
const FULL_MIN: u16 = 40;
/// A Preview narrower than this is dropped and its width goes to the Full pane.
const PREVIEW_MIN: u16 = 36;
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

/// Natural widths, borders included. The Full pane grows only to its natural width.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Hints {
    pub scope: u16,
    pub runs: u16,
    pub tree: u16,
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

pub fn plan(area: Rect, focus: Pane, hints: Hints) -> Plan {
    let focused = level(focus);
    let width = area.width;
    // Detail keeps the tree as context at any two-pane width. Elsewhere a two-pane width shows
    // the Preview of the next level, since the breadcrumb already names the level before.
    let (mut left, right) = if width < MEDIUM {
        (None, None)
    } else if focus == Pane::Detail {
        (Some(level(Pane::Artifacts)), None)
    } else if width < WIDE {
        (None, Some(focused + 1))
    } else {
        (focused.checked_sub(1), Some(focused + 1))
    };
    let mut compact = left.map_or(0, |level| COMPACT[level]);
    if width.saturating_sub(compact) < FULL_MIN {
        left = None;
        compact = 0;
    }
    let rest = width - compact;
    let natural = match focus {
        Pane::Repositories => hints.scope,
        Pane::Runs => hints.runs,
        Pane::Artifacts => hints.tree,
        Pane::Detail => u16::MAX,
    }
    .max(FULL_MIN);
    let (full, preview) = match right {
        Some(_) if rest >= FULL_MIN + PREVIEW_MIN => {
            let full = natural.min(rest - PREVIEW_MIN);
            (full, rest - full)
        }
        _ => (rest, 0),
    };
    let mut panes = Vec::new();
    let mut x = area.x;
    let mut push = |pane, width, density| {
        panes.push((pane, Rect::new(x, area.y, width, area.height), density));
        x += width;
    };
    if let Some(level) = left {
        push(LEVELS[level], compact, Density::Compact);
    }
    push(focus, full, Density::Full);
    if let Some(level) = right.filter(|_| preview > 0) {
        push(LEVELS[level], preview, Density::Preview);
    }
    Plan { panes }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Density::{Compact, Full, Preview};
    use Pane::{Artifacts as Tree, Detail, Repositories as Scope, Runs};

    const HINTS: Hints = Hints {
        scope: 48,
        runs: 72,
        tree: 80,
    };

    fn widths(width: u16, focus: Pane, hints: Hints) -> Vec<(Pane, u16, Density)> {
        let plan = plan(Rect::new(0, 1, width, 30), focus, hints);
        let mut x = 0;
        for (_, area, _) in &plan.panes {
            assert_eq!((area.x, area.y, area.height), (x, 1, 30), "{plan:?}");
            x += area.width;
        }
        assert_eq!(x, width, "panes fill the body: {plan:?}");
        plan.panes
            .into_iter()
            .map(|(pane, area, density)| (pane, area.width, density))
            .collect()
    }

    #[test]
    fn breakpoints_choose_panes_and_widths_for_every_focus() {
        for (width, focus, expected) in [
            // Wide: Compact, Full up to its natural width, and the rest as Preview.
            (160, Scope, vec![(Scope, 48, Full), (Runs, 112, Preview)]),
            (
                160,
                Runs,
                vec![(Scope, 20, Compact), (Runs, 72, Full), (Tree, 68, Preview)],
            ),
            (
                160,
                Tree,
                vec![(Runs, 18, Compact), (Tree, 80, Full), (Detail, 62, Preview)],
            ),
            (160, Detail, vec![(Tree, 30, Compact), (Detail, 130, Full)]),
            (
                140,
                Runs,
                vec![(Scope, 20, Compact), (Runs, 72, Full), (Tree, 48, Preview)],
            ),
            (
                140,
                Tree,
                vec![(Runs, 18, Compact), (Tree, 80, Full), (Detail, 42, Preview)],
            ),
            // Medium: the Full pane and the Preview of the next level.
            (139, Scope, vec![(Scope, 48, Full), (Runs, 91, Preview)]),
            (139, Runs, vec![(Runs, 72, Full), (Tree, 67, Preview)]),
            (120, Tree, vec![(Tree, 80, Full), (Detail, 40, Preview)]),
            (110, Tree, vec![(Tree, 74, Full), (Detail, 36, Preview)]),
            (110, Detail, vec![(Tree, 30, Compact), (Detail, 80, Full)]),
            (100, Runs, vec![(Runs, 64, Full), (Tree, 36, Preview)]),
            (100, Detail, vec![(Tree, 30, Compact), (Detail, 70, Full)]),
            // Narrow: one pane, the breadcrumb carries the rest.
            (99, Scope, vec![(Scope, 99, Full)]),
            (99, Detail, vec![(Detail, 99, Full)]),
            (80, Runs, vec![(Runs, 80, Full)]),
            (60, Tree, vec![(Tree, 60, Full)]),
        ] {
            assert_eq!(widths(width, focus, HINTS), expected, "{width} {focus:?}");
        }
    }

    #[test]
    fn natural_width_caps_full_and_minimums_drop_neighbours() {
        let narrow = Hints {
            scope: 10,
            runs: 10,
            tree: 10,
        };
        // A pane narrower than the minimum still gets the minimum Full width.
        assert_eq!(
            widths(160, Scope, narrow),
            [(Scope, 40, Full), (Runs, 120, Preview)]
        );
        // A huge natural width leaves exactly the minimum Preview.
        let wide = Hints {
            scope: 500,
            runs: 500,
            tree: 500,
        };
        assert_eq!(
            widths(160, Tree, wide),
            [
                (Runs, 18, Compact),
                (Tree, 106, Full),
                (Detail, 36, Preview)
            ]
        );
        // Without room for Full and Preview minimums, the Preview goes and Full takes it.
        assert_eq!(widths(75, Runs, wide), [(Runs, 75, Full)]);
        assert_eq!(
            widths(100, Tree, wide),
            [(Tree, 64, Full), (Detail, 36, Preview)]
        );
    }

    #[test]
    fn density_reports_hidden_levels_and_back_never_leaves_the_first_level() {
        let wide = plan(Rect::new(0, 0, 160, 20), Detail, HINTS);
        assert_eq!(wide.density(Scope), Density::Hidden);
        assert_eq!(wide.density(Runs), Density::Hidden);
        assert_eq!(wide.density(Tree), Compact);
        assert_eq!(wide.density(Detail), Full);
        assert_eq!(wide.panes[1].1.x, 30);
        assert_eq!(
            [Scope, Runs, Tree, Detail].map(back),
            [Scope, Scope, Runs, Tree]
        );
        // A zero-sized body still has exactly the focused pane.
        assert_eq!(plan(Rect::default(), Tree, HINTS).panes.len(), 1);
    }
}
