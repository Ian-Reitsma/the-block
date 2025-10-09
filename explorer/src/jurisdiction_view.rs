use crate::Explorer;
use foundation_serialization::Serialize;

#[derive(Serialize)]
pub struct JurisdictionTxs {
    pub region: String,
    pub count: usize,
}

pub fn summary(_explorer: &Explorer, region: &str) -> JurisdictionTxs {
    // placeholder: real implementation would filter DB by region tag
    JurisdictionTxs {
        region: region.to_string(),
        count: 0,
    }
}
