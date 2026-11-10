use super::*;

fn graph(edges: &[(&str, &str)]) -> Graph {
    let mut graph = Graph::new();
    for (from, to) in edges {
        graph.entry((*from).to_string()).or_default().insert((*to).to_string());
        graph.entry((*to).to_string()).or_default();
    }
    graph
}

#[test]
fn a_tree_has_no_cycles() {
    assert_eq!(
        find_cycles(&graph(&[("a", "b"), ("a", "c"), ("b", "d"), ("c", "d")])),
        [] as [Vec<String>; 0]
    );
}

#[test]
fn reports_one_shortest_cycle_per_group_starting_at_its_smallest_node() {
    let cycles = find_cycles(&graph(&[
        // A long way round and a short way back.
        ("b", "c"),
        ("c", "d"),
        ("d", "b"),
        ("c", "b"),
        // A separate pair.
        ("y", "x"),
        ("x", "y"),
        // Reaches a cycle without being part of one.
        ("a", "b"),
        ("d", "z"),
    ]));

    assert_eq!(cycles, [vec!["b", "c"], vec!["x", "y"]]);
}

#[test]
fn a_node_that_points_at_itself_is_a_cycle() {
    assert_eq!(find_cycles(&graph(&[("a", "a"), ("a", "b")])), [vec!["a"]]);
}

#[test]
fn deep_chains_do_not_overflow() {
    let names: Vec<String> = (0..50_000).map(|index| format!("n{index:05}")).collect();
    let mut graph = Graph::new();
    for pair in names.windows(2) {
        graph.entry(pair[0].clone()).or_default().insert(pair[1].clone());
    }
    graph.entry(names[49_999].clone()).or_default().insert(names[0].clone());

    let cycles = find_cycles(&graph);
    assert_eq!(cycles.len(), 1);
    assert_eq!(cycles[0].len(), 50_000);
}
