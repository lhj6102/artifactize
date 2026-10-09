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

/// One drill-down level of a screen: its fixed Compact width and its natural Full width,
/// borders included. `u16::MAX` lets a level grow to the whole body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Level {
    pub compact: u16,
    pub natural: u16,
}

/// The monitor's levels: Scope, Runs and the Run tree have Compact widths; Detail grows freely.
pub fn plan(area: Rect, focus: Pane, hints: Hints) -> Plan {
    let natural = [hints.scope, hints.runs, hints.tree, u16::MAX];
    let levels = LEVELS.map(|pane| {
        let index = level(pane);
        Level {
            compact: COMPACT.get(index).copied().unwrap_or(0),
            natural: natural[index],
        }
    });
    let panes = columns(area, &levels, level(focus))
        .into_iter()
        .map(|(index, area, density)| (LEVELS[index], area, density))
        .collect();
    Plan { panes }
}

/// Visible levels of any drill-down screen, by index, left to right: the focused level is Full
/// up to its natural width, the level to its left is Compact and the level to its right a
/// Preview. The last level (a Detail) keeps its left neighbour as context at any two-pane width
/// and never has a Preview. Below `MEDIUM` only the focused level shows.
pub(crate) fn columns(area: Rect, levels: &[Level], focused: usize) -> Vec<(usize, Rect, Density)> {
    let last = levels.len().saturating_sub(1);
    let width = area.width;
    let next = (focused < last).then_some(focused + 1);
    // A Detail keeps the level before it as context at any two-pane width. Elsewhere a two-pane
    // width shows the Preview of the next level, since the breadcrumb already names the level
    // before.
    let (mut left, right) = if width < MEDIUM {
        (None, None)
    } else if focused == last {
        (focused.checked_sub(1), None)
    } else if width < WIDE {
        (None, next)
    } else {
        (focused.checked_sub(1), next)
    };
    let mut compact = left.map_or(0, |level| levels[level].compact);
    if width.saturating_sub(compact) < FULL_MIN {
        left = None;
        compact = 0;
    }
    let rest = width - compact;
    let natural = levels[focused].natural.max(FULL_MIN);
    let (full, preview) = match right {
        Some(_) if rest >= FULL_MIN + PREVIEW_MIN => {
            let full = natural.min(rest - PREVIEW_MIN);
            (full, rest - full)
        }
        _ => (rest, 0),
    };
    let mut panes = Vec::new();
    let mut x = area.x;
    let mut push = |level, width, density| {
        panes.push((level, Rect::new(x, area.y, width, area.height), density));
        x += width;
    };
    if let Some(level) = left {
        push(level, compact, Density::Compact);
    }
    push(focused, full, Density::Full);
    if let Some(level) = right.filter(|_| preview > 0) {
        push(level, preview, Density::Preview);
    }
    panes
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

    /// The standalone review's two levels: its waiting list (Compact 30) and the Detail.
    fn review(width: u16, focused: usize, natural: u16) -> Vec<(usize, u16, Density)> {
        let levels = [
            Level {
                compact: 30,
                natural,
            },
            Level {
                compact: 0,
                natural: u16::MAX,
            },
        ];
        let panes = columns(Rect::new(0, 1, width, 20), &levels, focused);
        assert_eq!(
            panes.iter().map(|(_, area, _)| area.width).sum::<u16>(),
            width,
            "{panes:?}"
        );
        panes
            .into_iter()
            .map(|(level, area, density)| (level, area.width, density))
            .collect()
    }

    #[test]
    fn review_list_and_detail_follow_the_same_breakpoints() {
        for (width, focused, expected) in [
            // List focused: the list grows to its natural width and the Detail previews.
            (160, 0, vec![(0, 70, Full), (1, 90, Preview)]),
            (120, 0, vec![(0, 70, Full), (1, 50, Preview)]),
            (100, 0, vec![(0, 64, Full), (1, 36, Preview)]),
            // Detail focused: the list is Compact beside the Full Detail, at any 2-pane width.
            (160, 1, vec![(0, 30, Compact), (1, 130, Full)]),
            (100, 1, vec![(0, 30, Compact), (1, 70, Full)]),
            // Narrow: one pane; the breadcrumb names the request.
            (99, 0, vec![(0, 99, Full)]),
            (80, 1, vec![(1, 80, Full)]),
        ] {
            assert_eq!(review(width, focused, 70), expected, "{width} {focused}");
        }
        // A very wide list leaves the minimum Preview.
        assert_eq!(review(160, 0, 500), [(0, 124, Full), (1, 36, Preview)]);
    }
}
