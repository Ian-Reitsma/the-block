use crate::Explorer;
use axum::{extract::Path, Json};
use serde::Serialize;
use std::sync::Arc;

#[derive(Serialize)]
pub struct JurisdictionTxs {
    pub region: String,
    pub count: usize,
}

pub async fn route(explorer: Arc<Explorer>, Path(region): Path<String>) -> Json<JurisdictionTxs> {
    // placeholder: real implementation would filter DB by region tag
    Json(JurisdictionTxs { region, count: 0 })
}
