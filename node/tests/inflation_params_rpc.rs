use the_block::governance::Params;
use the_block::rpc::governance;

#[test]
fn inflation_params_returns_defaults() {
    let params = Params::default();
    let resp = governance::inflation_params(&params);
    assert_eq!(
        resp["beta_storage_sub_ct"].as_i64().unwrap(),
        params.beta_storage_sub_ct
    );
    assert_eq!(
        resp["gamma_read_sub_ct"].as_i64().unwrap(),
        params.gamma_read_sub_ct
    );
    assert_eq!(
        resp["kappa_cpu_sub_ct"].as_i64().unwrap(),
        params.kappa_cpu_sub_ct
    );
    assert_eq!(
        resp["lambda_bytes_out_sub_ct"].as_i64().unwrap(),
        params.lambda_bytes_out_sub_ct
    );
    assert_eq!(
        resp["rent_rate_ct_per_byte"].as_i64().unwrap(),
        params.rent_rate_ct_per_byte
    );
}
