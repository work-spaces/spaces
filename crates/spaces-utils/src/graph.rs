use crate::suggest;
use anyhow_source_location::format_error;
use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Graph {
    directed_graph: petgraph::graph::DiGraph<Arc<str>, ()>,
    #[serde(skip)]
    node_index_cache: FxHashMap<Arc<str>, petgraph::prelude::NodeIndex>,
}

impl Graph {
    pub fn edge_count(&self) -> usize {
        self.directed_graph.edge_count()
    }

    pub fn capacity(&self) -> (usize, usize) {
        self.directed_graph.capacity()
    }

    pub fn clear(&mut self) {
        self.directed_graph.clear();
        self.node_index_cache.clear();
    }

    pub fn add_task(&mut self, task: Arc<str>) -> petgraph::prelude::NodeIndex {
        let idx = self.directed_graph.add_node(task.clone());
        self.node_index_cache.insert(task, idx);
        idx
    }

    pub fn add_dependency(&mut self, task_name: &str, dep_name: &str) -> anyhow::Result<()> {
        let task_node = self
            .node_index_cache
            .get(task_name)
            .copied()
            .ok_or(format_error!("Rule not found {task_name}"))?;

        let dep_node = self
            .node_index_cache
            .get(dep_name)
            .copied()
            .ok_or(format_error!("Dependency not found {dep_name}"))?;

        self.directed_graph.add_edge(task_node, dep_node, ());

        Ok(())
    }

    pub fn get_task(&self, node: petgraph::prelude::NodeIndex) -> &str {
        self.directed_graph[node].as_ref()
    }

    pub fn get_target_not_found(&self, target: Arc<str>) -> Arc<str> {
        let targets: Vec<Arc<str>> = self
            .directed_graph
            .node_indices()
            .map(|node| self.directed_graph[node].clone())
            .collect();
        let suggestions = suggest::get_suggestions(target.clone(), &targets);

        // get up to 5 suggestions
        let suggestions = suggestions
            .iter()
            .take(10)
            .map(|(_, suggestion)| suggestion.to_string())
            .collect::<Vec<String>>();

        format!(
            "{target} not found. Similar targets include:\n{}",
            suggestions.join("\n")
        )
        .into()
    }

    fn get_target_not_found_error(&self, target: Arc<str>) -> anyhow::Error {
        format_error!("{}", self.get_target_not_found(target).as_ref())
    }

    pub fn get_sorted_tasks(
        &self,
        target: Option<Arc<str>>,
    ) -> anyhow::Result<Vec<petgraph::prelude::NodeIndex>> {
        let mut topo_tasks =
            petgraph::algo::toposort(&self.directed_graph, None).map_err(|err| {
                let description = self.describe_cycle(err.node_id());
                format_error!("Found a circular dependency:\n{description}")
            })?;

        let sorted_tasks = if let Some(target) = target {
            let target_node = self
                .node_index_cache
                .get(target.as_ref())
                .copied()
                .ok_or(self.get_target_not_found_error(target))?;

            let mut tasks: Vec<petgraph::prelude::NodeIndex> = Vec::new();
            let mut dfs = petgraph::visit::DfsPostOrder::new(&self.directed_graph, target_node);
            while let Some(node) = dfs.next(&self.directed_graph) {
                tasks.push(node);
            }
            tasks
        } else {
            topo_tasks.reverse();
            topo_tasks
        };

        Ok(sorted_tasks)
    }

    /// Returns the direct dependencies of a given rule
    pub fn get_dependencies(&self, rule_name: &str) -> anyhow::Result<Vec<Arc<str>>> {
        let node = self.find_node(rule_name)?;
        let neighbors = self.directed_graph.neighbors(node);
        let mut deps: Vec<Arc<str>> = neighbors
            .map(|idx| self.directed_graph[idx].clone())
            .collect();
        deps.sort();
        Ok(deps)
    }

    /// Finds the node index for a given rule name
    pub fn find_node(&self, rule_name: &str) -> anyhow::Result<petgraph::prelude::NodeIndex> {
        self.node_index_cache
            .get(rule_name)
            .copied()
            .ok_or_else(|| format_error!("Rule not found: {}", rule_name))
    }

    /// Given a node known to participate in a cycle, return a human-readable
    /// description of the cycle, e.g. "a -> b -> c -> a", along with the full
    /// set of rules in the same strongly-connected component.
    fn describe_cycle(&self, seed: petgraph::prelude::NodeIndex) -> String {
        use petgraph::prelude::NodeIndex;
        use rustc_hash::FxHashSet;

        let name_of = |idx: NodeIndex| self.directed_graph[idx].to_string();

        // Find the strongly-connected component containing `seed`. All nodes
        // mutually reachable from `seed` participate in a cycle with it.
        let sccs = petgraph::algo::tarjan_scc(&self.directed_graph);
        let scc = sccs
            .into_iter()
            .find(|component| component.contains(&seed))
            .unwrap_or_else(|| vec![seed]);
        let scc_set: FxHashSet<NodeIndex> = scc.iter().copied().collect();

        // Handle the self-loop case explicitly.
        if scc.len() == 1 {
            let name = name_of(seed);
            return format!("  cycle: {name} -> {name}");
        }

        // Walk the SCC depth-first starting from `seed` until we revisit a
        // node already on the current path. That closes a concrete cycle.
        let mut path: Vec<NodeIndex> = Vec::new();
        let mut on_path: FxHashSet<NodeIndex> = FxHashSet::default();
        let mut stack: Vec<(NodeIndex, Vec<NodeIndex>)> = vec![(
            seed,
            self.directed_graph
                .neighbors(seed)
                .filter(|n| scc_set.contains(n))
                .collect(),
        )];
        path.push(seed);
        on_path.insert(seed);

        let mut cycle: Vec<NodeIndex> = Vec::new();
        'dfs: while let Some((_node, neighbors)) = stack.last_mut() {
            if let Some(next) = neighbors.pop() {
                if on_path.contains(&next) {
                    // Cycle closed: take the slice of the path from `next`
                    // onward, then append `next` again to make the loop
                    // visually explicit.
                    let start = path.iter().position(|n| *n == next).unwrap();
                    cycle.extend_from_slice(&path[start..]);
                    cycle.push(next);
                    break 'dfs;
                }
                path.push(next);
                on_path.insert(next);
                stack.push((
                    next,
                    self.directed_graph
                        .neighbors(next)
                        .filter(|n| scc_set.contains(n))
                        .collect(),
                ));
            } else {
                let popped = path.pop().expect("path and stack stay in sync");
                on_path.remove(&popped);
                stack.pop();
            }
        }

        // Fall back to listing the SCC if no concrete cycle was extracted
        // (shouldn't happen for a real SCC of size > 1, but stay defensive).
        if cycle.is_empty() {
            cycle = scc.clone();
            if let Some(first) = cycle.first().copied() {
                cycle.push(first);
            }
        }

        let cycle_str = cycle
            .iter()
            .map(|idx| name_of(*idx))
            .collect::<Vec<_>>()
            .join(" -> ");

        // If the SCC contains nodes outside the printed cycle, list them too
        // so the user knows other rules are tangled in the same component.
        let printed: FxHashSet<NodeIndex> = cycle.iter().copied().collect();
        let mut extras: Vec<String> = scc
            .iter()
            .filter(|idx| !printed.contains(idx))
            .map(|idx| name_of(*idx))
            .collect();

        if extras.is_empty() {
            format!("  cycle: {cycle_str}")
        } else {
            let mut involved: Vec<String> = scc.iter().map(|idx| name_of(*idx)).collect();
            involved.sort();
            extras.sort();
            format!(
                "  cycle: {cycle_str}\n  involved rules: {}",
                involved.join(", ")
            )
        }
    }

    /// Rebuilds the node index cache from the directed graph
    /// This should be called after deserialization since the cache is not serialized
    pub fn rebuild_cache(&mut self) {
        self.node_index_cache.clear();
        for idx in self.directed_graph.node_indices() {
            let task_name = self.directed_graph[idx].clone();
            self.node_index_cache.insert(task_name, idx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_add_task() {
        let mut graph = Graph::default();
        let task1: Arc<str> = "task1".into();
        let task2: Arc<str> = "task2".into();

        graph.add_task(task1.clone());
        graph.add_task(task2.clone());

        // Verify cache is populated
        assert_eq!(graph.node_index_cache.len(), 2);
        assert!(graph.node_index_cache.contains_key(&task1));
        assert!(graph.node_index_cache.contains_key(&task2));
    }

    #[test]
    fn test_cache_add_dependency() {
        let mut graph = Graph::default();
        let task1: Arc<str> = "task1".into();
        let task2: Arc<str> = "task2".into();

        graph.add_task(task1.clone());
        graph.add_task(task2.clone());

        // Should use cache for O(1) lookup
        let result = graph.add_dependency("task1", "task2");
        assert!(result.is_ok());
        assert_eq!(graph.directed_graph.edge_count(), 1);
    }

    #[test]
    fn test_cache_clear() {
        let mut graph = Graph::default();
        let task1: Arc<str> = "task1".into();

        graph.add_task(task1);
        assert_eq!(graph.node_index_cache.len(), 1);

        graph.clear();
        assert_eq!(graph.node_index_cache.len(), 0);
        assert_eq!(graph.directed_graph.node_count(), 0);
    }

    #[test]
    fn test_rebuild_cache() {
        let mut graph = Graph::default();
        let task1: Arc<str> = "task1".into();
        let task2: Arc<str> = "task2".into();

        graph.add_task(task1.clone());
        graph.add_task(task2.clone());

        // Manually clear the cache (simulating deserialization)
        graph.node_index_cache.clear();
        assert_eq!(graph.node_index_cache.len(), 0);

        // Rebuild cache
        graph.rebuild_cache();
        assert_eq!(graph.node_index_cache.len(), 2);
        assert!(graph.node_index_cache.contains_key(&task1));
        assert!(graph.node_index_cache.contains_key(&task2));
    }

    #[test]
    fn test_find_node_with_cache() {
        let mut graph = Graph::default();
        let task1: Arc<str> = "task1".into();

        let idx = graph.add_task(task1.clone());

        // find_node should use cache
        let found_idx = graph.find_node("task1").unwrap();
        assert_eq!(idx, found_idx);
    }

    #[test]
    fn test_cycle_two_nodes() {
        let mut graph = Graph::default();
        graph.add_task("a".into());
        graph.add_task("b".into());
        graph.add_dependency("a", "b").unwrap();
        graph.add_dependency("b", "a").unwrap();

        let err = graph.get_sorted_tasks(None).unwrap_err().to_string();
        assert!(err.contains("circular dependency"), "got: {err}");
        assert!(err.contains("a") && err.contains("b"), "got: {err}");
        // The cycle line should close on itself.
        assert!(
            err.contains("a -> b -> a") || err.contains("b -> a -> b"),
            "got: {err}"
        );
    }

    #[test]
    fn test_cycle_three_nodes() {
        let mut graph = Graph::default();
        graph.add_task("a".into());
        graph.add_task("b".into());
        graph.add_task("c".into());
        graph.add_dependency("a", "b").unwrap();
        graph.add_dependency("b", "c").unwrap();
        graph.add_dependency("c", "a").unwrap();

        let err = graph.get_sorted_tasks(None).unwrap_err().to_string();
        // All three rules must appear and the cycle line must close.
        for name in ["a", "b", "c"] {
            assert!(err.contains(name), "missing {name}: {err}");
        }
        let rotations = ["a -> b -> c -> a", "b -> c -> a -> b", "c -> a -> b -> c"];
        assert!(
            rotations.iter().any(|r| err.contains(r)),
            "expected one of {rotations:?} in: {err}"
        );
    }

    #[test]
    fn test_cycle_self_loop() {
        let mut graph = Graph::default();
        graph.add_task("a".into());
        graph.add_dependency("a", "a").unwrap();

        let err = graph.get_sorted_tasks(None).unwrap_err().to_string();
        assert!(err.contains("a -> a"), "got: {err}");
    }

    #[test]
    fn test_cycle_excludes_unrelated_rules() {
        let mut graph = Graph::default();
        graph.add_task("a".into());
        graph.add_task("b".into());
        graph.add_task("unrelated".into());
        graph.add_dependency("a", "b").unwrap();
        graph.add_dependency("b", "a").unwrap();

        let err = graph.get_sorted_tasks(None).unwrap_err().to_string();
        assert!(!err.contains("unrelated"), "got: {err}");
    }

    #[test]
    fn test_serde_cache_rebuild() {
        let mut graph = Graph::default();
        let task1: Arc<str> = "task1".into();
        let task2: Arc<str> = "task2".into();

        graph.add_task(task1.clone());
        graph.add_task(task2.clone());
        graph.add_dependency("task1", "task2").unwrap();

        // Serialize and deserialize
        let serialized = serde_json::to_string(&graph).unwrap();
        let mut deserialized: Graph = serde_json::from_str(&serialized).unwrap();

        // Cache should be empty after deserialization
        assert_eq!(deserialized.node_index_cache.len(), 0);

        // Rebuild cache
        deserialized.rebuild_cache();
        assert_eq!(deserialized.node_index_cache.len(), 2);

        // Verify functionality still works
        let result = deserialized.find_node("task1");
        assert!(result.is_ok());
        let result = deserialized.find_node("task2");
        assert!(result.is_ok());
    }
}
