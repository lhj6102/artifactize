//! The public definition view is independent of layout, comments and TOML map ordering.
use artifactize_tools::artf::definition;

#[test]
fn reordered_tables_and_inline_array_entries_have_one_definition_view() {
    let before = br#"# An author's comment
path = './'
data = [{ z = 1, a = { z = 2, a = 3 } }]
[properties.comment]
z = 4
a = 5
[properties.annotations]
text = 'structural schema data'
[annotations.style]
at = [1, 2]
"#;
    let after = br#"data=[{a={a=3,z=2},z=1}]
path="./"
[annotations]
future = "ignored"
[properties.annotations]
text = "structural schema data"
[properties.comment]
a=5
z=4
"#;
    assert_eq!(definition(before), definition(after));
    let viewed = String::from_utf8(definition(before).into_owned()).unwrap();
    assert!(viewed.contains("[properties.annotations]"));
    assert!(viewed.contains("[properties.comment]"));
    assert!(!viewed.contains("[annotations"));
    assert!(!viewed.contains("author's comment"));
}

#[test]
fn unreadable_toml_and_non_utf8_keep_their_original_bytes() {
    for bytes in [
        b"path = [\n[annotations]\ntext = 'not stripped'".as_slice(),
        &[0xff, 0x00, 0xfe],
    ] {
        assert_eq!(definition(bytes).as_ref(), bytes);
    }
}
