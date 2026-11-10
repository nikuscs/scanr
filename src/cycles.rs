//! Cycles in a directed graph of named nodes.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

pub type Graph = BTreeMap<String, BTreeSet<String>>;

/// One shortest cycle per group of nodes that can all reach each other. Each
/// cycle starts at its smallest node and is listed without repeating it at the end.
pub fn find_cycles(graph: &Graph) -> Vec<Vec<String>> {
    let mut cycles: Vec<Vec<String>> = components(graph)
        .into_iter()
        .filter_map(|component| {
            let start = component.iter().next()?;
            shortest_cycle(graph, start, &component)
        })
        .collect();
    cycles.sort();
    cycles
}

/// Strongly connected components (Tarjan), iterative so deep graphs cannot
/// overflow the stack.
fn components(graph: &Graph) -> Vec<BTreeSet<&str>> {
    struct Visit<'a> {
        node: &'a str,
        next: Vec<&'a str>,
    }

    let neighbours = |node: &str| -> Vec<&str> {
        graph.get(node).into_iter().flatten().map(String::as_str).rev().collect()
    };
    let mut index = 0usize;
    let mut indices: BTreeMap<&str, usize> = BTreeMap::new();
    let mut low: BTreeMap<&str, usize> = BTreeMap::new();
    let mut on_stack: BTreeSet<&str> = BTreeSet::new();
    let mut stack: Vec<&str> = Vec::new();
    let mut found = Vec::new();

    for root in graph.keys().map(String::as_str) {
        if indices.contains_key(root) {
            continue;
        }
        let mut work = vec![Visit { node: root, next: neighbours(root) }];
        indices.insert(root, index);
        low.insert(root, index);
        index += 1;
        stack.push(root);
        on_stack.insert(root);

        while let Some(visit) = work.last_mut() {
            let node = visit.node;
            if let Some(neighbour) = visit.next.pop() {
                if !indices.contains_key(neighbour) {
                    indices.insert(neighbour, index);
                    low.insert(neighbour, index);
                    index += 1;
                    stack.push(neighbour);
                    on_stack.insert(neighbour);
                    work.push(Visit { node: neighbour, next: neighbours(neighbour) });
                } else if on_stack.contains(neighbour) {
                    let reached = low[node].min(indices[neighbour]);
                    low.insert(node, reached);
                }
                continue;
            }
            work.pop();
            if let Some(parent) = work.last() {
                let reached = low[parent.node].min(low[node]);
                low.insert(parent.node, reached);
            }
            if low[node] == indices[node] {
                let mut component = BTreeSet::new();
                while let Some(member) = stack.pop() {
                    on_stack.remove(member);
                    component.insert(member);
                    if member == node {
                        break;
                    }
                }
                let self_loop = graph.get(node).is_some_and(|targets| targets.contains(node));
                if component.len() > 1 || self_loop {
                    found.push(component);
                }
            }
        }
    }
    found
}

/// Breadth-first search from `start` back to itself, staying inside `component`.
fn shortest_cycle(graph: &Graph, start: &str, component: &BTreeSet<&str>) -> Option<Vec<String>> {
    let mut previous: BTreeMap<&str, &str> = BTreeMap::new();
    let mut queue = VecDeque::from([start]);
    while let Some(node) = queue.pop_front() {
        for neighbour in graph.get(node).into_iter().flatten().map(String::as_str) {
            if neighbour == start {
                let mut path = vec![node.to_string()];
                let mut current = node;
                while let Some(&earlier) = previous.get(current) {
                    path.push(earlier.to_string());
                    current = earlier;
                }
                path.reverse();
                return Some(path);
            }
            if component.contains(neighbour) && !previous.contains_key(neighbour) {
                previous.insert(neighbour, node);
                queue.push_back(neighbour);
            }
        }
    }
    None
}

#[cfg(test)]
#[path = "cycles_test.rs"]
mod tests;
