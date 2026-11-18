//! Read-only observations of the terminal test backend.

/// Find click coordinates from the visible label, not the input router's hit rectangles.
pub(crate) fn point(buffer: &ratatui::buffer::Buffer, label: &str) -> ratatui::layout::Position {
    buffer
        .content()
        .chunks(usize::from(buffer.area.width))
        .enumerate()
        .find_map(|(y, row)| {
            let text: String = row.iter().map(|cell| cell.symbol()).collect();
            let start = text.find(label)?;
            let mut offset = 0;
            let x = row.iter().position(|cell| {
                let at_start = offset == start;
                offset += cell.symbol().len();
                at_start
            })?;
            Some(ratatui::layout::Position::new(x as u16, y as u16))
        })
        .unwrap_or_else(|| panic!("not rendered: {label:?}\n{buffer:?}"))
}
