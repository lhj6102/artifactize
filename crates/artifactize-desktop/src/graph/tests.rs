use super::{Light, NEAR, neighbours_in, shape};
use artifactize_core::{monitor, types::ArtifactName};

fn name(text: &str) -> ArtifactName {
    text.parse().unwrap()
}

fn node(text: &str, light: monitor::Light) -> monitor::GraphNode {
    monitor::GraphNode {
        id: monitor::NodeId::artifact(name(text)),
        name: name(text),
        light,
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
    let near = neighbours_in(&shape(&graph), &monitor::NodeId::artifact(name("criteria")));
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
    let near = neighbours_in(&shape(&graph), &monitor::NodeId::artifact(name("gone")));
    assert!(near.shape.nodes.is_empty() && near.shape.edges.is_empty());
    assert_eq!((near.more_uses, near.more_used_by), (0, 0));
}

#[test]
fn a_node_names_its_space_and_shows_the_rest_of_its_name() {
    let graph = monitor::Graph {
        nodes: vec![
            node("combat::Code", monitor::Light::Red),
            node("docs", monitor::Light::Green),
        ],
        edges: vec![edge("combat::Code", "docs")],
    };
    let shown = shape(&graph);
    let nodes: Vec<_> = shown
        .nodes
        .iter()
        .map(|item| {
            (
                item.name.as_str(),
                item.space.as_deref(),
                item.label.as_str(),
            )
        })
        .collect();
    assert_eq!(
        nodes,
        [
            ("combat::Code", Some("combat"), "Code"),
            ("docs", None, "docs")
        ]
    );
    assert_eq!(String::from(shown.nodes[0].id.clone()), "a:combat::Code");
}
