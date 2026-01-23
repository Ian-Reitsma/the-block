#![forbid(unsafe_code)]

use crate::compute_market::settlement::SettleMode;
use crate::config::NodeConfig;
use energy_market::{JurisdictionId, ProviderId};
use foundation_serialization::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub enum ChainType {
    Development,
    Live,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct InitialEnergyProvider {
    pub owner: ProviderId,
    pub meter_address: String,
    pub capacity_kwh: u64,
    pub jurisdiction: JurisdictionId,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct ChainSpec {
    pub name: String,
    pub id: String,
    pub chain_type: ChainType,
    pub validators: Vec<String>,
    pub energy_providers: Vec<InitialEnergyProvider>,
    pub jurisdiction_packs: Vec<String>,
    pub config: NodeConfig,
    pub telemetry_url: Option<String>,
    pub protocol_id: Option<String>,
}

impl ChainSpec {
    pub fn from_genesis(
        name: &str,
        id: &str,
        chain_type: ChainType,
        validators: Vec<String>,
        energy_providers: Vec<InitialEnergyProvider>,
        jurisdiction_packs: Vec<String>,
        config: NodeConfig,
        telemetry_url: Option<String>,
        protocol_id: Option<String>,
    ) -> Self {
        Self {
            name: name.to_string(),
            id: id.to_string(),
            chain_type,
            validators,
            energy_providers,
            jurisdiction_packs,
            config,
            telemetry_url,
            protocol_id,
        }
    }
}

pub fn block_os_testnet_config() -> ChainSpec {
    let validators = vec![
        "energy-validator-0001".to_string(),
        "energy-validator-0002".to_string(),
    ];
    let energy_providers = vec![
        InitialEnergyProvider {
            owner: "provider-alpha".to_string(),
            meter_address: "mock_meter_1".to_string(),
            capacity_kwh: 10_000,
            jurisdiction: "US_CA".to_string(),
        },
        InitialEnergyProvider {
            owner: "provider-beta".to_string(),
            meter_address: "mock_meter_2".to_string(),
            capacity_kwh: 5_000,
            jurisdiction: "US_NY".to_string(),
        },
    ];
    let jurisdiction_packs = vec!["US_CA".to_string(), "US_NY".to_string()];
    let mut config = NodeConfig::default();
    config.compute_market.settle_mode = SettleMode::Real;
    ChainSpec::from_genesis(
        "Block OS Testnet - Energy",
        "block_os_energy_v1",
        ChainType::Live,
        validators,
        energy_providers,
        jurisdiction_packs,
        config,
        Some("wss://telemetry.block_os.network/submit".to_string()),
        Some("block_os-energy".to_string()),
    )
}
