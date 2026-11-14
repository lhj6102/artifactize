use super::{Light, NEAR, current_neighbours, run_neighbours};
use artifactize_core::{monitor, types::ArtifactName};

fn name(text: &str) -> ArtifactName {
    text.parse().unwrap()
}

fn node(text: &str, light: monitor::Light) -> monitor::GraphNode {
    monitor::GraphNode {
        id: monitor::NodeId::artifact(name(text)),
        name: name(text),
        light,
        tally: None,
        parts: Vec::new(),
    }
}

fn edge(from: &str, to: &str) -> monitor::GraphEdge {
    monitor::GraphEdge {
        from: name(from),
        to: name(to),
        light: monitor::Light::Green,
    }
}

#[test]
fn a_shared_input_shows_its_failing_users_first_and_counts_the_rest() {
    // `criteria` is used by `NEAR + 10` reviews, every third one RED; it uses `rules`.
    let reviews: Vec<_> = (0..NEAR + 10).map(|at| format!("review-{at}")).collect();
    let light = |at: usize| match at % 3 {
        0 => monitor::Light::Red,
        _ => monitor::Light::Green,
    };
    let graph = monitor::Graph {
        nodes: [
            node("rules", monitor::Light::Green),
            node("criteria", monitor::Light::Green),
        ]
        .into_iter()
        .chain(
            reviews
                .iter()
                .enumerate()
                .map(|(at, item)| node(item, light(at))),
        )
        .collect(),
        edges: std::iter::once(edge("rules", "criteria"))
            .chain(reviews.iter().map(|item| edge("criteria", item)))
            .collect(),
    };
    let near = run_neighbours(&graph, &monitor::NodeId::artifact(name("criteria")));
    assert_eq!((near.more_uses, near.more_used_by), (0, 10));
    let used_by: Vec<_> = near
        .shape
        .edges
        .iter()
        .filter(|item| String::from(item.source.clone()) == "a:criteria")
        .map(|item| String::from(item.target.clone()))
        .collect();
    assert_eq!(used_by.len(), NEAR);
    // Every RED review comes first, in the tree's order, then the GREEN ones.
    let red = (0..NEAR + 10).filter(|at| at % 3 == 0).count();
    let lit: Vec<_> = used_by
        .iter()
        .map(|id| {
            let found = near
                .shape
                .nodes
                .iter()
                .find(|item| &String::from(item.id.clone()) == id);
            found.unwrap().light
        })
        .collect();
    assert!(lit[..red].iter().all(|light| *light == Light::Red));
    assert!(lit[red..].iter().all(|light| *light == Light::Green));
    assert_eq!(used_by[0], "a:review-0");
    assert_eq!(used_by[1], "a:review-3");
    // The Artifact and every neighbour shown, and nothing more.
    assert_eq!(near.shape.nodes.len(), 1 + 1 + NEAR);
}

#[test]
fn an_artifact_the_run_lacks_has_no_neighbours() {
    let graph = monitor::Graph {
        nodes: vec![node("app", monitor::Light::Green)],
        edges: Vec::new(),
    };
    let near = run_neighbours(&graph, &monitor::NodeId::artifact(name("gone")));
    assert!(near.shape.nodes.is_empty() && near.shape.edges.is_empty());
    assert_eq!((near.more_uses, near.more_used_by), (0, 0));
}

#[test]
fn a_shared_part_is_drawn_inside_each_drawing_of_each_of_its_wholes() {
    // `site` and `app` share `kit`; `suite` holds `app` and `site`, and `docs` holds `site`,
    // so `site` is drawn twice and `kit` three times.
    let whole = |text: &str, parts: &[&str]| monitor::GraphNode {
        parts: parts.iter().map(|part| name(part)).collect(),
        ..node(text, monitor::Light::Green)
    };
    let graph = monitor::Graph {
        nodes: vec![
            whole("suite", &["app", "site"]),
            whole("docs", &["site"]),
            whole("app", &["kit"]),
            whole("site", &["kit"]),
            node("kit", monitor::Light::Green),
        ],
        edges: Vec::new(),
    };
    let drawn = |text: &str| {
        let near = run_neighbours(&graph, &monitor::NodeId::artifact(name(text)));
        (near.wholes, near.copies)
    };
    assert_eq!(drawn("suite"), (Vec::new(), 1));
    assert_eq!(drawn("app"), (vec!["suite".to_owned()], 1));
    assert_eq!(
        drawn("site"),
        (vec!["suite".to_owned(), "docs".to_owned()], 2)
    );
    assert_eq!(drawn("kit"), (vec!["app".to_owned(), "site".to_owned()], 3));
}

#[test]
fn current_neighbours_keep_would_run_and_checking_ahead_of_blocked_and_waiting() {
    use artifactize_core::{
        config::ArtifactKind,
        monitor::live::{Current, CurrentArtifact, CurrentEdge, Plan},
        project::ArtifactCondition,
    };
    let artifact = |text: &str, plan| CurrentArtifact {
        name: name(text),
        kind: ArtifactKind::Folder,
        plan,
        runs: usize::from(plan == Plan::Run),
        path: text.into(),
        state: match plan {
            Plan::Run => ArtifactCondition::Stale,
            Plan::Check => ArtifactCondition::Unreviewed,
            Plan::Blocked => ArtifactCondition::Blocked,
            Plan::Wait => ArtifactCondition::WaitDependency,
            Plan::Reuse => ArtifactCondition::Pass,
        },
        reason: String::new(),
        evals: Vec::new(),
        parts: Vec::new(),
    };
    let waiting: Vec<_> = (0..NEAR).map(|at| format!("waiting-{at}")).collect();
    let users: Vec<_> = waiting
        .iter()
        .map(|text| artifact(text, Plan::Wait))
        .chain([
            artifact("blocked", Plan::Blocked),
            artifact("checking", Plan::Check),
            artifact("would-run", Plan::Run),
        ])
        .collect();
    let current = Current {
        scanned_at: time::OffsetDateTime::UNIX_EPOCH,
        fingerprint: "v1:neighbours".parse().unwrap(),
        structure: "".into(),
        edges: users
            .iter()
            .map(|user| CurrentEdge {
                from: name("criteria"),
                to: user.name.clone(),
                plan: Plan::Reuse,
            })
            .collect(),
        artifacts: std::iter::once(artifact("criteria", Plan::Reuse))
            .chain(users)
            .collect(),
    };
    let near = current_neighbours(&current, &monitor::NodeId::artifact(name("criteria")));
    let shown: Vec<_> = near
        .shape
        .edges
        .iter()
        .map(|edge| String::from(edge.target.clone()))
        .collect();
    let expected: Vec<_> = ["a:would-run", "a:checking", "a:blocked"]
        .into_iter()
        .map(str::to_owned)
        .chain(
            waiting
                .iter()
                .take(NEAR - 3)
                .map(|text| format!("a:{text}")),
        )
        .collect();
    assert_eq!(shown, expected);
    assert_eq!((near.more_uses, near.more_used_by), (0, 3));
}
