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
        b"path = './'\n[evals.check]\nprofile = !sol(reasoning = 'low'\n".as_slice(),
        b"path = './'\n[evals.check]\nprofile = !\n".as_slice(),
        b"path = './'\n[evals.check]\nprofile = !sol(reasoning = )\n".as_slice(),
        &[0xff, 0x00, 0xfe],
    ] {
        assert_eq!(definition(bytes).as_ref(), bytes);
    }
}

#[test]
fn alias_expressions_canonicalize_overrides_and_strip_annotations() {
    let first = br#"# Leading comment
path = "crates/app/"

[evals.review]
title = "Review"
profile = !sol(
  timeout_ms = 600000, # custom timeout
  reasoning = "low",
)
payload.instruction = "Check !sol and !sol(model = 'ignored') in {app}."

[evals.review.profile_variants]
base = !sol()
alt = !luna

[annotations.style]
at = [12, 34]
"#;
    let second = br#"path = 'crates/app/'

[evals.review]
profile = !sol(reasoning = 'low', timeout_ms = 600000)
title = 'Review'

[evals.review.payload]
instruction = '''Check !sol and !sol(model = 'ignored') in {app}.'''

[evals.review.profile_variants]
alt = !luna
base = !sol
"#;
    assert_eq!(definition(first), definition(second));
    let viewed = String::from_utf8(definition(first).into_owned()).unwrap();
    assert!(viewed.contains("profile = !sol(reasoning = \"low\", timeout_ms = 600000)"));
    assert!(viewed.contains("base = !sol\n"));
    assert!(viewed.contains("alt = !luna\n"));
    assert!(viewed.contains("Check !sol and !sol(model = 'ignored') in {app}."));
    assert!(!viewed.contains("[annotations"));
    assert!(!viewed.contains("custom timeout"));
}

#[test]
fn mask_preserves_offsets_nested_parens_and_placeholder_collisions() {
    let source = concat!(
        "note = \"__artf_alias_0__\"\n",
        "profile = !sol(\n",
        "  # closing paren in comment )\n",
        "  model = \"gpt-6(preview)\",\n",
        "  reasoning = \"low\",\n",
        ")\n",
        "after = \"ok\"\n",
    );
    let masked = artifactize_tools::artf::mask(source).unwrap();
    assert_eq!(masked.invocations().len(), 1);
    let invocation = &masked.invocations()[0];
    assert_eq!(invocation.name, "sol");
    assert_eq!(
        &source[invocation.span.clone()],
        "!sol(\n  # closing paren in comment )\n  model = \"gpt-6(preview)\",\n  reasoning = \"low\",\n)"
    );
    let after_masked = masked.toml.find("after").unwrap();
    let after_original = source.find("after").unwrap();
    assert_eq!(masked.original_offset(after_masked), after_original);
    assert_eq!(masked.unmask(&masked.toml, source), source);
}
