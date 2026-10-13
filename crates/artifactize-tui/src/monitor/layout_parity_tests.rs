//! The session layout, which the core computes, matches what the terminal renderer draws.
use artifactize_core::agent::session::document;

#[test]
fn layout_matches_actual_renderer_for_cjk_combining_halfwidth_and_long_json() {
    use ratatui::{
        buffer::Buffer,
        layout::Rect,
        text::Line,
        widgets::{Paragraph, Widget},
    };
    let text = "한글e\u{301}ｶﾞｶﾞAB\n{\"long\":\"xxxxxxxxxxxxxxxxx\"}\n";
    for width in [1, 2, 3, 7, 20] {
        let ranges = document::rows(text, width);
        let rows = ranges
            .iter()
            .map(|range| document::row(text, range, width))
            .collect::<Vec<_>>();
        let mut buffer = Buffer::empty(Rect::new(0, 0, width as u16, rows.len() as u16));
        Paragraph::new(
            rows.iter()
                .map(|row| Line::from(row.as_str()))
                .collect::<Vec<_>>(),
        )
        .render(buffer.area, &mut buffer);
        for (y, row) in rows.iter().enumerate() {
            use ratatui::buffer::CellWidth;
            let mut x = 0;
            let mut rendered = String::new();
            while x < width {
                let symbol = buffer[(x as u16, y as u16)].symbol();
                rendered.push_str(symbol);
                x += usize::from(symbol.cell_width().max(1));
            }
            assert_eq!(rendered.trim_end(), row.trim_end());
        }
    }
    let rows = document::rows("ｶﾞｶﾞ", 2);
    assert_eq!(rows.len(), 2);
    assert_eq!(document::row("ｶﾞｶﾞ", &rows[0], 2), "ｶﾞ");
}
