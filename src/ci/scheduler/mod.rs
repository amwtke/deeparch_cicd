use anyhow::{bail, Result};
use petgraph::algo::toposort;
use petgraph::graph::{DiGraph, NodeIndex};
use std::collections::HashMap;

use crate::ci::parser::Pipeline;

/// Schedule is a list of batches; each batch contains step names that can run in parallel
pub type Schedule = Vec<Vec<String>>;

/// Builds a DAG from pipeline steps and resolves execution order
pub struct Scheduler {
    graph: DiGraph<String, ()>,
    node_map: HashMap<String, NodeIndex>,
}

impl Scheduler {
    /// Build the DAG from a validated pipeline
    pub fn new(pipeline: &Pipeline) -> Result<Self> {
        let mut graph = DiGraph::new();
        let mut node_map = HashMap::new();

        // Add all steps as nodes
        for step in &pipeline.steps {
            let idx = graph.add_node(step.name.clone());
            node_map.insert(step.name.clone(), idx);
        }

        // Add dependency edges (dep -> step, meaning dep must run first)
        for step in &pipeline.steps {
            let step_idx = node_map[&step.name];
            for dep in &step.depends_on {
                let dep_idx = node_map[dep];
                graph.add_edge(dep_idx, step_idx, ());
            }
        }

        // Check for cycles
        if toposort(&graph, None).is_err() {
            bail!("Pipeline contains a dependency cycle");
        }

        Ok(Self { graph, node_map })
    }

    /// Resolve execution schedule: returns batches of steps that can run in parallel.
    /// If step_filter is provided, only run that step and its transitive dependencies.
    pub fn resolve(&self, step_filter: Option<&str>) -> Result<Schedule> {
        let mut schedule = Vec::new();

        // Get topological order
        let topo = toposort(&self.graph, None)
            .map_err(|_| anyhow::anyhow!("Cycle detected in pipeline DAG"))?;

        // If filtering to a single step, find its transitive deps
        let included: Option<HashMap<NodeIndex, bool>> = step_filter.map(|name| {
            let target = self
                .node_map
                .get(name)
                .unwrap_or_else(|| panic!("Step '{}' not found", name));
            let mut included = HashMap::new();
            self.collect_deps(*target, &mut included);
            included
        });

        // Group into batches by "depth" in DAG
        // Steps at the same depth have no dependencies on each other → can be parallel
        let mut depth_map: HashMap<NodeIndex, usize> = HashMap::new();

        for &node in &topo {
            if let Some(ref inc) = included {
                if !inc.contains_key(&node) {
                    continue;
                }
            }

            let max_dep_depth = self
                .graph
                .neighbors_directed(node, petgraph::Direction::Incoming)
                .filter_map(|dep| depth_map.get(&dep))
                .max()
                .copied()
                .unwrap_or(0);

            let my_depth = if self
                .graph
                .neighbors_directed(node, petgraph::Direction::Incoming)
                .count()
                == 0
            {
                0
            } else {
                max_dep_depth + 1
            };

            depth_map.insert(node, my_depth);

            // Ensure schedule has enough batches
            while schedule.len() <= my_depth {
                schedule.push(Vec::new());
            }

            let name = &self.graph[node];
            schedule[my_depth].push(name.clone());
        }

        Ok(schedule)
    }

    /// Recursively collect a node and all its transitive dependencies
    fn collect_deps(&self, node: NodeIndex, result: &mut HashMap<NodeIndex, bool>) {
        if result.contains_key(&node) {
            return;
        }
        result.insert(node, true);
        for dep in self
            .graph
            .neighbors_directed(node, petgraph::Direction::Incoming)
        {
            self.collect_deps(dep, result);
        }
    }

    /// Get the execution depth of each step (for display purposes)
    #[allow(dead_code)]
    pub fn step_depths(&self) -> HashMap<String, usize> {
        let schedule = self.resolve(None).unwrap_or_default();
        let mut depths = HashMap::new();
        for (depth, batch) in schedule.iter().enumerate() {
            for name in batch {
                depths.insert(name.clone(), depth);
            }
        }
        depths
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ci::parser::Pipeline;

    #[test]
    fn test_parallel_schedule() {
        let yaml = r#"
name: test
steps:
  - name: build
    image: rust:1.78
    commands: [cargo build]
  - name: lint
    image: rust:1.78
    commands: [cargo clippy]
  - name: test
    image: rust:1.78
    depends_on: [build]
    commands: [cargo test]
"#;
        let pipeline = Pipeline::from_str(yaml).unwrap();
        let scheduler = Scheduler::new(&pipeline).unwrap();
        let schedule = scheduler.resolve(None).unwrap();

        // build and lint should be in the same batch (depth 0)
        assert_eq!(schedule.len(), 2);
        assert!(schedule[0].contains(&"build".to_string()));
        assert!(schedule[0].contains(&"lint".to_string()));
        assert_eq!(schedule[1], vec!["test"]);
    }

    #[test]
    fn test_single_step_filter() {
        let yaml = r#"
name: test
steps:
  - name: build
    image: rust:1.78
    commands: [cargo build]
  - name: test
    image: rust:1.78
    depends_on: [build]
    commands: [cargo test]
  - name: deploy
    image: alpine
    depends_on: [test]
    commands: [echo deploy]
"#;
        let pipeline = Pipeline::from_str(yaml).unwrap();
        let scheduler = Scheduler::new(&pipeline).unwrap();
        let schedule = scheduler.resolve(Some("test")).unwrap();

        // Should include build -> test, but not deploy
        let all_steps: Vec<&str> = schedule.iter().flatten().map(|s| s.as_str()).collect();
        assert!(all_steps.contains(&"build"));
        assert!(all_steps.contains(&"test"));
        assert!(!all_steps.contains(&"deploy"));
    }

    #[test]
    fn test_linear_dependency_chain() {
        let yaml = r#"
name: test
steps:
  - name: a
    image: alpine
    commands: [echo a]
  - name: b
    image: alpine
    depends_on: [a]
    commands: [echo b]
  - name: c
    image: alpine
    depends_on: [b]
    commands: [echo c]
"#;
        let pipeline = Pipeline::from_str(yaml).unwrap();
        let scheduler = Scheduler::new(&pipeline).unwrap();
        let schedule = scheduler.resolve(None).unwrap();
        assert_eq!(schedule.len(), 3); // 3 batches, one per step
        assert_eq!(schedule[0], vec!["a"]);
        assert_eq!(schedule[1], vec!["b"]);
        assert_eq!(schedule[2], vec!["c"]);
    }

    #[test]
    fn test_all_independent_steps() {
        let yaml = r#"
name: test
steps:
  - name: a
    image: alpine
    commands: [echo a]
  - name: b
    image: alpine
    commands: [echo b]
  - name: c
    image: alpine
    commands: [echo c]
"#;
        let pipeline = Pipeline::from_str(yaml).unwrap();
        let scheduler = Scheduler::new(&pipeline).unwrap();
        let schedule = scheduler.resolve(None).unwrap();
        assert_eq!(schedule.len(), 1); // all in one batch
        assert_eq!(schedule[0].len(), 3);
    }

    #[test]
    fn test_diamond_dependency() {
        let yaml = r#"
name: test
steps:
  - name: start
    image: alpine
    commands: [echo start]
  - name: left
    image: alpine
    depends_on: [start]
    commands: [echo left]
  - name: right
    image: alpine
    depends_on: [start]
    commands: [echo right]
  - name: end
    image: alpine
    depends_on: [left, right]
    commands: [echo end]
"#;
        let pipeline = Pipeline::from_str(yaml).unwrap();
        let scheduler = Scheduler::new(&pipeline).unwrap();
        let schedule = scheduler.resolve(None).unwrap();
        assert_eq!(schedule.len(), 3);
        assert_eq!(schedule[0], vec!["start"]);
        assert!(schedule[1].contains(&"left".to_string()));
        assert!(schedule[1].contains(&"right".to_string()));
        assert_eq!(schedule[2], vec!["end"]);
    }

    #[test]
    fn test_step_depths() {
        let yaml = r#"
name: test
steps:
  - name: build
    image: alpine
    commands: [echo build]
  - name: test
    image: alpine
    depends_on: [build]
    commands: [echo test]
"#;
        let pipeline = Pipeline::from_str(yaml).unwrap();
        let scheduler = Scheduler::new(&pipeline).unwrap();
        let depths = scheduler.step_depths();
        assert_eq!(depths["build"], 0);
        assert_eq!(depths["test"], 1);
    }
}
