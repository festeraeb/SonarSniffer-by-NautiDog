use serde_json::Value;
use std::collections::HashMap;
use uuid::Uuid;
use crate::nodes::{Node, NodeId, NodeOutput};

pub type WorkflowId = Uuid;

#[derive(Debug, Clone, PartialEq)]
pub enum WorkflowState {
    Idle,
    Running,
    Error(String),
}

pub struct Workflow {
    pub id: WorkflowId,
    pub name: String,
    pub nodes: Vec<(NodeId, Node)>,
    pub edges: Vec<(NodeId, NodeId)>,
    pub state: WorkflowState,
    pub context: HashMap<NodeId, Value>,
}

pub struct WorkflowContext {
    // Annotate collect types: Vec<(WorkflowId, NodeId, Value)>
    pub broadcasts: Vec<(WorkflowId, NodeId, Value)>,
}

impl WorkflowContext {
    pub fn new() -> Self {
        Self { broadcasts: Vec::new() }
    }
    pub fn record_event(&mut self, wf_id: WorkflowId, node_id: NodeId, data: Value) {
        self.broadcasts.push((wf_id, node_id, data));
    }
}

impl Workflow {
    pub fn new(name: String, nodes: Vec<(NodeId, Node)>, edges: Vec<(NodeId, NodeId)>) -> Self {
        Self {
            id: Uuid::new_v4(),
            name,
            nodes,
            edges,
            state: WorkflowState::Idle,
            context: HashMap::new(),
        }
    }

    // Topological sort (Kahn's algorithm) for DAG execution order
    pub fn topological_order(&self) -> Vec<NodeId> {
        let mut in_degree: HashMap<NodeId, usize> = HashMap::new();
        let mut adj: HashMap<NodeId, Vec<NodeId>> = HashMap::new();
        
        for (from, to) in &self.edges {
            in_degree.entry(*to).or_insert(0);
            adj.entry(*from).or_insert_with(Vec::new).push(*to);
        }
        for (id, _) in &self.nodes {
            in_degree.entry(*id).or_insert(0);
        }

        let mut queue: Vec<NodeId> = self.nodes.iter()
            .filter(|(id, _)| in_degree.get(id) == Some(&0))
            .map(|(id, _)| *id)
            .collect();
        
        let mut order: Vec<NodeId> = Vec::new();
        while let Some(node_id) = queue.pop() {
            order.push(node_id);
            if let Some(neighbors) = adj.get(&node_id) {
                for neighbor in neighbors {
                    let deg = in_degree.get_mut(neighbor).unwrap();
                    *deg -= 1;
                    if *deg == 0 {
                        queue.push(*neighbor);
                    }
                }
            }
        }
        order
    }

    pub async fn execute(&mut self, ctx: &mut WorkflowContext) -> Result<(), String> {
        self.state = WorkflowState::Running;
        let order = self.topological_order();
        
        for node_id in order {
            let node = self.nodes.iter()
                .find(|(id, _)| *id == node_id)
                .map(|(_, n)| n)
                .ok_or_else(|| format!("Node {node_id} not found in DAG"))?;
            
            // Compute before move: evaluate node, capture output, then move into context
            let output = node.execute(&self.context).await?;
            self.context.insert(node_id, output.data.clone());
            ctx.record_event(self.id, node_id, output.data);
        }
        
        self.state = WorkflowState::Idle;
        Ok(())
    }
}
