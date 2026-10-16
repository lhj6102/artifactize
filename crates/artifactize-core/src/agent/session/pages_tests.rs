//! Disk layout is exercised through the viewer's read-only session window.
use super::{
    document::Position,
    live::Reader,
    live_tests::{answer, append, finish, fixture},
};

#[test]
fn session_windows_wrap_unicode_against_independent_rows() {
    // Cell widths: CJK, halfwidth kana with dakuten, and the joined emoji each
    // occupy two columns; the combining accent stays attached to its e.
    let text = "ab한e\u{301}ｶﾞ👩‍💻\r\n012345";
    for (width, expected) in [
        (
            1,
            vec!["a", "b", "e\u{301}", "0", "1", "2", "3", "4", "5", ""],
        ),
        (3, vec!["ab", "한e\u{301}", "ｶﾞ", "👩‍💻", "012", "345", ""]),
        (7, vec!["ab한e\u{301}ｶﾞ", "👩‍💻", "012345", ""]),
        (13, vec!["ab한e\u{301}ｶﾞ👩‍💻", "012345", ""]),
    ] {
        let (_root, source) = fixture();
        append(&source, &answer(text));
        let mut reader = Reader::new(source);
        let window = finish(&mut reader, width, 20, Position::Row(0));
        assert!(window.status.is_none(), "{:?}", window.status);
        assert_eq!(window.rows, expected, "width {width}");
        assert_eq!(window.total, expected.len());
    }
}

#[test]
fn wide_graphemes_do_not_hide_following_rows_or_later_appends() {
    let (_root, source) = fixture();
    // A large invisible run crosses many normal indexing/layout batches.
    append(
        &source,
        &answer(&("A".to_owned() + &"한".repeat(1_000_000) + "B\nC")),
    );
    let mut reader = Reader::new(source.clone());
    for _ in 0..3 {
        let window = finish(&mut reader, 1, 4, Position::Row(0));
        assert!(window.status.is_none(), "{:?}", window.status);
        assert_eq!(window.rows, ["A", "B", "C", ""]);
    }
    append(&source, &answer("D"));
    let window = finish(&mut reader, 1, 2, Position::Bottom);
    assert!(window.status.is_none(), "{:?}", window.status);
    assert_eq!(window.rows, ["D", ""]);
}

#[test]
fn huge_combining_grapheme_and_row_offsets_are_not_limited_to_u16() {
    let (_root, source) = fixture();
    let grapheme = "e".to_owned() + &"\u{301}".repeat(300_000);
    append(&source, &answer(&("x".repeat(70_000) + &grapheme)));
    let mut reader = Reader::new(source);
    let window = finish(&mut reader, 1, 3, Position::Bottom);
    assert!(window.status.is_none(), "{:?}", window.status);
    assert_eq!(window.total, 70_002);
    assert_eq!(window.top, 69_999);
    assert_eq!(window.rows, ["x", grapheme.as_str(), ""]);
}
