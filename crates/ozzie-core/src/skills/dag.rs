use std::collections::{HashMap, HashSet, VecDeque};

use super::types::StepDef;

/// Directed Acyclic Graph for workflow step execution.
///
/// Built via Kahn's algorithm with cycle detection.
pub struct DAG {
    steps: HashMap<String, StepDef>,
    order: Vec<String>,
}

impl DAG {
    /// Builds a DAG from step definitions. Returns error if cycles detected.
    pub fn new(steps: Vec<StepDef>) -> Result<Self, DagError> {
        if steps.is_empty() {
            return Err(DagError::Empty);
        }

        let step_map: HashMap<String, StepDef> =
            steps.iter().map(|s| (s.id.clone(), s.clone())).collect();

        // Validate that all dependencies exist
        for step in &steps {
            for dep in &step.needs {
                if !step_map.contains_key(dep) {
                    return Err(DagError::MissingDependency {
                        step: step.id.clone(),
                        dep: dep.clone(),
                    });
                }
            }
        }

        // Kahn's algorithm for topological sort
        let mut in_degree: HashMap<String, usize> = HashMap::new();
        let mut adjacency: HashMap<String, Vec<String>> = HashMap::new();

        for step in &steps {
            in_degree.entry(step.id.clone()).or_insert(0);
            for dep in &step.needs {
                adjacency
                    .entry(dep.clone())
                    .or_default()
                    .push(step.id.clone());
                *in_degree.entry(step.id.clone()).or_insert(0) += 1;
            }
        }

        let mut queue: VecDeque<String> = in_degree
            .iter()
            .filter(|&(_, deg)| *deg == 0)
            .map(|(id, _)| id.clone())
            .collect();

        let mut order = Vec::new();

        while let Some(node) = queue.pop_front() {
            order.push(node.clone());
            if let Some(deps) = adjacency.get(&node) {
                for dep in deps {
                    let Some(deg) = in_degree.get_mut(dep) else {
                        continue;
                    };
                    *deg -= 1;
                    if *deg == 0 {
                        queue.push_back(dep.clone());
                    }
                }
            }
        }

        if order.len() != steps.len() {
            return Err(DagError::Cycle);
        }

        Ok(Self {
            steps: step_map,
            order,
        })
    }

    /// Returns step IDs in topological order.
    pub fn topological_order(&self) -> &[String] {
        &self.order
    }

    /// Returns step IDs whose dependencies are all in `completed`.
    pub fn ready_steps(&self, completed: &HashSet<String>) -> Vec<String> {
        self.order
            .iter()
            .filter(|id| {
                !completed.contains(*id)
                    && self.steps[*id].needs.iter().all(|dep| completed.contains(dep))
            })
            .cloned()
            .collect()
    }

    /// Returns a step definition by ID.
    pub fn get_step(&self, id: &str) -> Option<&StepDef> {
        self.steps.get(id)
    }

    /// Returns the total number of steps.
    pub fn len(&self) -> usize {
        self.steps.len()
    }

    /// Returns true if the DAG has no steps.
    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum DagError {
    #[error("DAG is empty")]
    Empty,
    #[error("cycle detected in workflow DAG")]
    Cycle,
    #[error("step '{step}' depends on unknown step '{dep}'")]
    MissingDependency { step: String, dep: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn step(id: &str, needs: Vec<&str>) -> StepDef {
        StepDef {
            id: id.to_string(),
            title: None,
            instruction: format!("Do {id}"),
            tools: Vec::new(),
            model: None,
            needs: needs.into_iter().map(|s| s.to_string()).collect(),
            acceptance: None,
        }
    }

    #[test]
    fn linear_dag() {
        let dag = DAG::new(vec![
            step("a", vec![]),
            step("b", vec!["a"]),
            step("c", vec!["b"]),
        ])
        .unwrap();

        assert_eq!(dag.topological_order(), &["a", "b", "c"]);
    }

    #[test]
    fn diamond_dag() {
        let dag = DAG::new(vec![
            step("a", vec![]),
            step("b", vec!["a"]),
            step("c", vec!["a"]),
            step("d", vec!["b", "c"]),
        ])
        .unwrap();

        let order = dag.topological_order();
        let pos = |id: &str| order.iter().position(|s| s == id).unwrap();

        assert!(pos("a") < pos("b"));
        assert!(pos("a") < pos("c"));
        assert!(pos("b") < pos("d"));
        assert!(pos("c") < pos("d"));
    }

    #[test]
    fn cycle_detected() {
        let result = DAG::new(vec![step("a", vec!["b"]), step("b", vec!["a"])]);
        assert!(matches!(result, Err(DagError::Cycle)));
    }

    #[test]
    fn missing_dep() {
        let result = DAG::new(vec![step("a", vec!["nonexistent"])]);
        assert!(matches!(result, Err(DagError::MissingDependency { .. })));
    }

    #[test]
    fn ready_steps() {
        let dag = DAG::new(vec![
            step("a", vec![]),
            step("b", vec!["a"]),
            step("c", vec!["a"]),
            step("d", vec!["b", "c"]),
        ])
        .unwrap();

        let mut completed = HashSet::new();

        let ready = dag.ready_steps(&completed);
        assert_eq!(ready, vec!["a"]);

        completed.insert("a".to_string());
        let ready = dag.ready_steps(&completed);
        assert!(ready.contains(&"b".to_string()));
        assert!(ready.contains(&"c".to_string()));

        completed.insert("b".to_string());
        completed.insert("c".to_string());
        let ready = dag.ready_steps(&completed);
        assert_eq!(ready, vec!["d"]);
    }

    #[test]
    fn empty_dag() {
        assert!(matches!(DAG::new(vec![]), Err(DagError::Empty)));
    }
}
