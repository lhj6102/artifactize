//! Parser tests: the item forms of `.artf` and `.artfprofiles` files and what the syntax alone
//! refuses.

use super::*;

fn items(source: &str) -> Vec<Item> {
    parse(FileId(0), source).unwrap_or_else(|error| panic!("{}: {error:?}", source))
}

#[test]
fn items_take_macro_bangs_params_headers_and_bodies() {
    let parsed = items(
        r#"artf! crate(dir, extra = {}) : files & cargo = { path = "x", ...extra }
tool cargo = { agent test = command(command = "cargo", args = []) }
artf core = crate!(dir = "core")
"#,
    );
    let kinds: Vec<_> = parsed
        .iter()
        .map(|item| (item.kind, item.params.is_some()))
        .collect();
    assert_eq!(
        kinds,
        [
            (ItemKind::Artf, true),
            (ItemKind::Tool, false),
            (ItemKind::Artf, false),
        ]
    );
    assert!(matches!(parsed[0].header, Some(Expr::And(..))));
    let Expr::Block(block) = &parsed[0].body else {
        panic!("a block body");
    };
    assert!(matches!(block.entries[1], Entry::Spread(..)));
}

#[test]
fn values_nested_past_the_limit_are_refused_as_input() {
    let nested = |depth: usize| {
        format!(
            "artf a = {{ path = \"a\", x = {}1{} }}",
            "[".repeat(depth),
            "]".repeat(depth)
        )
    };
    // The body and the innermost value count too: the limit itself parses, one more does not.
    items(&nested(MAX_NESTING - 2));
    assert!(parse(FileId(0), &nested(MAX_NESTING - 1)).is_err());
    // Deep enough to overflow the stack without the limit; refused with a message instead.
    let error = parse(FileId(0), &nested(100_000)).unwrap_err();
    assert_eq!(
        error.message,
        format!("Values nest more than {MAX_NESTING} deep.")
    );
    let blocks = format!(
        "artf a = {{ path = \"a\", x = {}1{} }}",
        "{ y = ".repeat(100_000),
        " }".repeat(100_000)
    );
    assert!(parse(FileId(0), &blocks).is_err());
}

#[test]
fn syntax_errors_point_at_the_token() {
    for (source, at) in [
        ("artf x = { path = }", 18),
        ("artf self = {}", 5),
        ("artf x = { eval a : human = { title = \"t\" } ", 44),
        ("artf x = call(a)", 15),
        ("artf x", 6),
    ] {
        let error = parse(FileId(0), source).unwrap_err();
        assert_eq!(error.span.start, at, "{source}: {}", error.message);
    }
}

#[test]
fn removed_forms_say_what_replaced_them() {
    for (source, says) in [
        (
            "eval style : human = { title = \"t\" }",
            "inside the `artf` block",
        ),
        (
            "eval! style() : human = { title = \"t\" }",
            "inside the `artf` block",
        ),
        ("profile fast = { model = \"m\" }", "`.artfprofiles` files"),
        ("tool! docs(file) = {}", "no `tool!` macros"),
        (
            "artf x = { path = \"x\", fingerprint = none }",
            "leave the field out",
        ),
    ] {
        let error = parse(FileId(0), source).unwrap_err();
        assert!(error.message.contains(says), "{source}: {}", error.message);
    }
}

#[test]
fn a_profile_is_one_setting_or_a_match_with_a_branch_per_value() {
    let parsed = parse_profiles(
        FileId(0),
        r#"profile reviewer : agent = match TIER {
  default => { backend = "codex", model = "gpt-6-luna", reasoning = "max" },
  heavy => { backend = "codex", model = "gpt-6.2-sol", reasoning = "high" },
}
profile translator : agent = { backend = "codex", model = "gpt-6-luna" }
"#,
    )
    .unwrap();
    assert_eq!(parsed.len(), 2);
    let ProfileBody::Match { variable, arms } = &parsed[0].body else {
        panic!("a match");
    };
    assert_eq!(variable.text, "TIER");
    let labels: Vec<_> = arms.iter().map(|(label, _)| label.text.as_str()).collect();
    assert_eq!(labels, ["default", "heavy"]);
    assert_eq!(parsed[0].kind.text, "agent");
    assert!(matches!(parsed[1].body, ProfileBody::Setting(_)));
    for bad in [
        "artf x = { path = \"x\" }",
        "profile x = { model = \"m\" }",
        "profile! x() : agent = {}",
        "profile x : agent = match TIER { default { } }",
    ] {
        assert!(parse_profiles(FileId(0), bad).is_err(), "{bad}");
    }
}
