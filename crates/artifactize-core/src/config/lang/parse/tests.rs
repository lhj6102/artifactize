//! Parser tests: the four item forms and what the syntax alone refuses.

use super::*;

fn items(source: &str) -> Vec<Item> {
    parse(FileId(0), source).unwrap_or_else(|error| panic!("{}: {error:?}", source))
}

#[test]
fn items_take_macro_bangs_params_headers_and_bodies() {
    let parsed = items(
        r#"artf! crate(dir, extra = {}) : files & cargo = { path = "x", ...extra }
tool cargo = { agent test = command(command = "cargo", args = []) }
profile fast = agent(backend = "openai", model = "m")
eval! style() : fast = { title = "S", instruction = "I" }
"#,
    );
    let kinds: Vec<_> = parsed.iter().map(|item| (item.kind, item.params.is_some())).collect();
    assert_eq!(
        kinds,
        [
            (ItemKind::Artf, true),
            (ItemKind::Tool, false),
            (ItemKind::Profile, false),
            (ItemKind::Eval, true),
        ]
    );
    assert!(matches!(parsed[0].header, Some(Expr::And(..))));
    let Expr::Block(block) = &parsed[0].body else {
        panic!("a block body");
    };
    assert!(matches!(block.entries[1], Entry::Spread(..)));
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
