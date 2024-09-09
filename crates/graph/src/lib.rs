use anyhow_source_location::format_error;

#[derive(Debug)]
pub struct Graph {
    pub directed_graph: petgraph::graph::DiGraph<String, ()>,
}

impl Graph {
    pub fn new() -> Self {
        Graph {
            directed_graph: petgraph::graph::DiGraph::new(),
        }
    }

    pub fn clear(&mut self) {
        self.directed_graph.clear();
    }

    pub fn add_task(&mut self, task: String) {
        self.directed_graph.add_node(task);
    }

    pub fn add_dependency(&mut self, task_name: &str, dep_name: &str) -> anyhow::Result<()> {
        let task_node = self
            .directed_graph
            .node_indices()
            .find(|i| self.directed_graph[*i] == task_name)
            .ok_or(format_error!("Task not found {task_name}"))?;

        let dep_node = self
            .directed_graph
            .node_indices()
            .find(|i| self.directed_graph[*i] == dep_name)
            .ok_or(format_error!("Dependency not found {dep_name}"))?;

        self.directed_graph.add_edge(task_node, dep_node, ());

        Ok(())
    }

    pub fn get_task(&self, node: petgraph::prelude::NodeIndex) -> &str {
        self.directed_graph[node].as_str()
    }

    pub fn get_sorted_tasks(&self) -> Vec<petgraph::prelude::NodeIndex> {
        let mut sorted_tasks =
            petgraph::algo::toposort(&self.directed_graph, None).expect("Cycle detected");

        sorted_tasks.reverse();
        sorted_tasks
    }
}