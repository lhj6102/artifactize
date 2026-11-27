use super::*;

fn kinds(source: &str) -> Vec<Kind> {
    lex(source)
        .unwrap()
        .tokens
        .into_iter()
        .map(|token| token.kind)
        .collect()
}

fn literal(source: &str) -> Literal {
    match kinds(source).remove(0) {
        Kind::Str(literal) => literal,
        other => panic!("not a string: {other:?}"),
    }
}

fn text(value: &str) -> Part {
    Part::Text(value.into())
}

#[test]
fn punctuation_identifiers_numbers_and_comments() {
    let source = "artf! x-1_y(a = -1.5e3, b = [0]) : files & html.view // note\n...rest";
    let lexed = lex(source).unwrap();
    assert_eq!(
        lexed.tokens.iter().map(|token| token.kind.clone()).collect::<Vec<_>>(),
        vec![
            Kind::Ident("artf".into()),
            Kind::Bang,
            Kind::Ident("x-1_y".into()),
            Kind::LParen,
            Kind::Ident("a".into()),
            Kind::Eq,
            Kind::Number("-1.5e3".into()),
            Kind::Comma,
            Kind::Ident("b".into()),
            Kind::Eq,
            Kind::LBracket,
            Kind::Number("0".into()),
            Kind::RBracket,
            Kind::RParen,
            Kind::Colon,
            Kind::Ident("files".into()),
            Kind::Amp,
            Kind::Ident("html".into()),
            Kind::Dot,
            Kind::Ident("view".into()),
            Kind::Spread,
            Kind::Ident("rest".into()),
        ]
    );
    assert_eq!(&source[lexed.comments[0].clone()], "// note");
    assert_eq!(&source[lexed.tokens[2].span.clone()], "x-1_y");
}

#[test]
fn malformed_tokens_say_where() {
    for (source, offset) in [
        ("a = -x", 4),
        ("a = 01", 4),
        ("a = 1.", 5),
        ("a = 1e", 6),
        ("a = 12ab", 4),
        ("a = 'b'", 4),
        ("a = \"b", 4),
        ("a = \"\"\"b", 4),
        ("a = $", 4),
        (&format!("{} = 1", "a".repeat(65)), 0),
    ] {
        let failure = lex(source).unwrap_err();
        assert_eq!(failure.offset, offset, "{source}: {failure}");
    }
}

#[test]
fn strings_decode_escapes_and_keep_braces_literal() {
    assert_eq!(
        literal(r#""a\"b\\c\né {#id} {\"json\": 1} ${HOME}""#).parts,
        vec![text("a\"b\\c\né {#id} {\"json\": 1} ${HOME}")]
    );
    assert_eq!(literal(r#""""#).parts, vec![text("")]);
    assert!(lex(r#""\q""#).is_err());
}

#[test]
fn slots_are_typed_with_spans_and_double_at_escapes_them() {
    let source = r#""Read @{artf rules} at @{path self/src/x.rs} via @{tool self human html.view}, not @@{artf x}.""#;
    let parts = literal(source).parts;
    let Part::Slot(path) = &parts[3] else {
        panic!("{parts:?}");
    };
    assert_eq!(path.kind, SlotKind::Path);
    assert_eq!(&source[path.span.clone()], "@{path self/src/x.rs}");
    assert_eq!(&source[path.words[0].1.clone()], "self/src/x.rs");
    let Part::Slot(tool) = &parts[5] else {
        panic!("{parts:?}");
    };
    assert_eq!(
        tool.words.iter().map(|(word, _)| word.as_str()).collect::<Vec<_>>(),
        ["self", "human", "html.view"]
    );
    assert_eq!(parts[0], text("Read "));
    assert_eq!(parts[6], text(", not @{artf x}."));
    // A file name under an Artifact may hold spaces; the path runs to the closing brace.
    let spaced = r#""@{path self/open me.txt }""#;
    let Part::Slot(path) = &literal(spaced).parts[0] else {
        panic!("{spaced}");
    };
    assert_eq!(path.words.len(), 1);
    assert_eq!(&spaced[path.words[0].1.clone()], "self/open me.txt");
    for bad in [r#""@{}""#, r#""@{name x}""#, r#""@{tool a b}""#, r#""@{artf a b}""#, r#""@{artf a""#, r#""@{path a b}""#] {
        assert!(lex(bad).is_err(), "{bad}");
    }
}

#[test]
fn multiline_strings_dedent_and_join_continued_lines() {
    let source = "x = \"\"\"\n    Read @{artf a}.\n      Indented {#id}\n\n    Joined \\\n      here.\n  \"\"\"";
    let Kind::Str(literal) = kinds(source).remove(2) else {
        panic!("not a string");
    };
    let parts = literal.parts;
    assert_eq!(parts[0], text("Read "));
    let Part::Slot(slot) = &parts[1] else {
        panic!("{parts:?}");
    };
    assert_eq!(&source[slot.span.clone()], "@{artf a}");
    assert_eq!(parts[2], text(".\n  Indented {#id}\n\nJoined here."));
}

#[test]
fn raw_strings_keep_every_byte() {
    let parts = literal("'''\n  @{artf x} \\n @@{\n  '''").parts;
    assert_eq!(parts, vec![text("@{artf x} \\n @@{")]);
}
