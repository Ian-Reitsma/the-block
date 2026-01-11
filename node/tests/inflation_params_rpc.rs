#![cfg(feature = "integration-tests")]
use the_block::governance::Params;
use the_block::rpc::governance;

#[test]
fn inflation_params_returns_defaults() {
    let params = Params::default();
    let resp = governance::inflation_params(&params);
    assert_eq!(resp.beta_storage_sub, params.beta_storage_sub);
    assert_eq!(resp.gamma_read_sub, params.gamma_read_sub);
    assert_eq!(resp.kappa_cpu_sub, params.kappa_cpu_sub);
    assert_eq!(resp.lambda_bytes_out_sub, params.lambda_bytes_out_sub);
    assert_eq!(resp.rent_rate_per_byte, params.rent_rate_per_byte);
}
