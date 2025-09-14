use serde::Serialize;

#[derive(Serialize)]
pub struct ValidatorShare {
    pub validator: String,
    pub share: String,
}

pub fn list_shares() -> Vec<ValidatorShare> {
    Vec::new()
}
