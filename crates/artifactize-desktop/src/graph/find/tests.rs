use super::{LISTED, Named, find_in};
use crate::status::Tone;
use artifactize_core::{monitor::NodeId, types::ArtifactName};

fn artifact(name: &str) -> Named<'_> {
    Named {
        id: NodeId::artifact(name.parse::<ArtifactName>().unwrap()),
        name,
        glyph: "✓",
        tone: Tone::Green,
    }
}

/// An eval of `artifact`; its id only has to differ from every other node's.
fn eval<'a>(artifact: &str, name: &'a str) -> Named<'a> {
    Named {
        id: NodeId::artifact(
            format!("{artifact}-{name}")
                .parse::<ArtifactName>()
                .unwrap(),
        ),
        name,
        glyph: "✗",
        tone: Tone::Red,
    }
}

/// What was found, read as the field lists it (`artifact` or `artifact / eval`), and how many
/// more.
fn listed(text: &str, graph: &[(&str, &[&str])]) -> (Vec<String>, u32) {
    let finding = find_in(
        graph.iter().map(|(name, evals)| {
            let evals: Vec<_> = evals.iter().map(|each| eval(name, each)).collect();
            (artifact(name), evals)
        }),
        text,
    );
    let names = finding
        .found
        .iter()
        .map(|found| match &found.eval {
            Some(eval) => format!("{} / {eval}", found.artifact),
            None => found.artifact.clone(),
        })
        .collect();
    (names, finding.more)
}

#[test]
fn finds_artifacts_and_evals_by_part_of_their_names_ignoring_case_in_the_graph_order() {
    let graph: &[(&str, &[&str])] = &[
        ("docs::Guide", &["reads-well", "matches-code"]),
        ("code", &["clippy", "Docs-links"]),
        ("release", &["signoff"]),
    ];
    assert_eq!(
        listed("DOCS", graph),
        (vec!["docs::Guide".into(), "code / Docs-links".into()], 0)
    );
    assert_eq!(
        listed("  sign ", graph),
        (vec!["release / signoff".into()], 0)
    );
    assert_eq!(listed("nothing", graph), (vec![], 0));
}

#[test]
fn an_empty_text_finds_nothing() {
    let graph: &[(&str, &[&str])] = &[("docs", &["reads-well"])];
    assert_eq!(listed("", graph), (vec![], 0));
    assert_eq!(listed("   ", graph), (vec![], 0));
}

#[test]
fn an_eval_opens_its_artifact_on_its_own_tab() {
    let finding = find_in(
        [(artifact("release"), vec![eval("release", "signoff")])],
        "sign",
    );
    let [found] = finding.found.as_slice() else {
        panic!("one eval found: {finding:?}");
    };
    assert_eq!(
        found.node.inner(),
        &NodeId::artifact("release".parse().unwrap())
    );
    assert_eq!(
        found.tab.as_ref().map(|tab| tab.inner()),
        Some(&eval("release", "signoff").id)
    );
    assert_eq!((found.glyph.as_str(), found.tone), ("✗", Tone::Red));
}

#[test]
fn lists_the_first_ones_and_counts_the_rest() {
    let names: Vec<String> = (0..LISTED + 7)
        .map(|at| format!("review-{at:03}"))
        .collect();
    let graph: Vec<(&str, &[&str])> = names.iter().map(|name| (name.as_str(), &[][..])).collect();
    let (found, more) = listed("review", &graph);
    assert_eq!(found.len(), LISTED);
    assert_eq!(found.first().map(String::as_str), Some("review-000"));
    assert_eq!(more, 7);
}
