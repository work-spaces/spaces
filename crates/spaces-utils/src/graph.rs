use crate::suggest;
use anyhow_source_location::format_error;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Graph {
    directed_graph: petgraph::graph::DiGraph<Arc<str>, ()>,
    node_index_cache: HashMap<Arc<str>, petgraph::prelude::NodeIndex>,
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
                // get the name of the node
                let name = self.directed_graph[err.node_id()].clone();
                format_error!("Found a circular dependency involving {name}")
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
