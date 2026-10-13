use super::{
    document::{self, Position},
    pages::Pages,
};

#[test]
fn bounded_disk_layout_matches_pure_rows_at_every_small_chunk_boundary() {
    let text = "abc 한글e\u{301}ｶﾞ👩‍💻\r\n0123456789\nlong\"json\":xxxxxxxxxxxxxxxxxxxxxxxxx\n";
    for chunk in 1..text.len() {
        for width in [1, 2, 3, 7, 13] {
            let mut pages = Pages::new(width).unwrap();
            pages.append(text).unwrap();
            let mut done = false;
            for _ in 0..1000 {
                if !pages.layout_chunk(chunk).unwrap() {
                    done = true;
                    break;
                }
            }
            assert!(done, "chunk {chunk}, width {width}");
            let (_, total, _, actual) = pages.window(Position::Row(0), 1000).unwrap();
            let expected = document::rows(text, width)
                .iter()
                .map(|range| document::row(text, range, width))
                .collect::<Vec<_>>();
            assert_eq!(actual, expected, "chunk {chunk}, width {width}");
            assert_eq!(total, expected.len());
        }
    }
}
#[test]
fn invisible_wide_runs_are_not_reread_by_visible_page_in_or_append() {
    let text = "A".to_owned() + &"한".repeat(1000000) + "B\nC\n";
    let mut pages = Pages::new(1).unwrap();
    pages.append(&text).unwrap();
    while pages.layout().unwrap() {}
    let before = pages.bytes_read;
    for _ in 0..30 {
        let (_, _, _, rows) = pages.window(Position::Row(0), 3).unwrap();
        assert_eq!(rows, ["A", "B", "C"]);
    }
    assert!(pages.bytes_read - before < 1024);
    pages.append("D\n").unwrap();
    while pages.layout().unwrap() {}
    assert_eq!(
        pages.window(Position::Bottom, 4).unwrap().3,
        ["A", "B", "C", "D"]
    );
}

#[test]
fn huge_ascii_grapheme_tail_and_offsets_are_not_limited_to_u16() {
    let text = "x".repeat(70000) + "e" + &"\u{301}".repeat(300000) + "\n";
    let mut pages = Pages::new(1).unwrap();
    pages.append(&text).unwrap();
    for _ in 0..10000 {
        if !pages.layout().unwrap() {
            break;
        }
    }
    let (top, total, _, rows) = pages.window(Position::Bottom, 2).unwrap();
    assert_eq!(total, 70001);
    assert_eq!(top, 69999);
    assert_eq!(rows[0], "x");
    assert!(rows[1].starts_with('e'));
    assert!(pages.private());
}
