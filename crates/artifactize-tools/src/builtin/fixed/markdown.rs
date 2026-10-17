//! Pure Markdown section selection.
use super::bounded;
use pulldown_cmark::{Event, Parser, Tag, TagEnd};

pub(super) fn section(text: &str, wanted: &str) -> Result<String, String> {
    let mut headings = Vec::new();
    let mut heading = None;
    for (event, range) in Parser::new(text).into_offset_iter() {
        match event {
            Event::Start(Tag::Heading { level, .. }) => {
                heading = Some((level, range.start, String::new()))
            }
            Event::Text(value) | Event::Code(value) => {
                if let Some((_, _, title)) = &mut heading {
                    title.push_str(&value);
                }
            }
            Event::SoftBreak | Event::HardBreak => {
                if let Some((_, _, title)) = &mut heading {
                    title.push(' ');
                }
            }
            Event::End(TagEnd::Heading(_)) => {
                if let Some(heading) = heading.take() {
                    headings.push(heading);
                }
            }
            _ => {}
        }
    }
    let Some(index) = headings.iter().position(|(_, _, title)| title == wanted) else {
        return Err(bounded(&format!(
            "No matching heading: {wanted}. Available headings:\n{}",
            headings
                .iter()
                .map(|(_, _, title)| title.as_str())
                .collect::<Vec<_>>()
                .join("\n")
        )));
    };
    let (level, start, _) = &headings[index];
    let end = headings[index + 1..]
        .iter()
        .find(|(next, _, _)| next <= level)
        .map_or(text.len(), |(_, start, _)| *start);
    Ok(bounded(&text[*start..end]))
}
