#[derive(Default)]
pub struct NodeCatalog;

impl NodeCatalog {
    pub fn new() -> Self { Self }
    pub fn healthy_nodes(&self) -> Vec<String> { vec!["local".into()] }
}
