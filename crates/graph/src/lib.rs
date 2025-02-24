use anyhow_source_location::format_error;
use std::sync::Arc;

#[derive(Debug, Default)]
pub struct Graph {
    pub directed_graph: petgraph::graph::DiGraph<Arc<str>, ()>,
}

impl Graph {
    pub fn clear(&mut self) {
        self.directed_graph.clear();
    }

    pub fn add_task(&mut self, task: Arc<str>) {
        self.directed_graph.add_node(task);
    }

    pub fn add_dependency(&mut self, task_name: &str, dep_name: &str) -> anyhow::Result<()> {
        let task_node = self
            .directed_graph
            .node_indices()
            .find(|i| self.directed_graph[*i].as_ref() == task_name)
            .ok_or(format_error!("Task not found {task_name}"))?;

        let dep_node = self
            .directed_graph
            .node_indices()
            .find(|i| self.directed_graph[*i].as_ref() == dep_name)
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
        let sorted_tasks = if let Some(target) = target {
            let target_node = self
                .directed_graph
                .node_indices()
                .find(|&node| {
                    let value = &self.directed_graph[node];
                    value.as_ref() == target.as_ref()
                })
                .ok_or(self.get_target_not_found_error(target))?;

            let mut tasks: Vec<petgraph::prelude::NodeIndex> = Vec::new();
            let mut dfs = petgraph::visit::DfsPostOrder::new(&self.directed_graph, target_node);
            while let Some(node) = dfs.next(&self.directed_graph) {
                tasks.push(node);
            }
            tasks
        } else {
            let mut tasks = petgraph::algo::toposort(&self.directed_graph, None)
                .map_err(|err| format_error!("Found a circular dependency in the graph {err:?}"))?;
            tasks.reverse();
            tasks
        };

        Ok(sorted_tasks)
    }
}
