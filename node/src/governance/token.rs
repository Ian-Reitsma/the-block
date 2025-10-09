use foundation_serialization::{Deserialize, Serialize};
use ledger::{Emission, TokenRegistry};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub enum TokenAction {
    Add { symbol: String, emission: Emission },
    Remove { symbol: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct TokenProposal {
    pub action: TokenAction,
}

impl TokenProposal {
    pub fn apply(self, reg: &mut TokenRegistry) {
        match self.action {
            TokenAction::Add { symbol, emission } => {
                reg.register(&symbol, emission);
            }
            TokenAction::Remove { symbol } => {
                reg.remove(&symbol);
            }
        }
    }
}
