use crypto_suite::signatures::ed25519::SigningKey;
use explorer::{did_view, DidDocumentView, Explorer, MetricPoint};
use foundation_serialization::json;
use hex;
use rand::{rngs::StdRng, Rng};
use std::convert::TryInto;
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};
use the_block::generate_keypair;
use the_block::governance::GovStore;
use the_block::identity::DidRegistry;
use the_block::transaction::TxDidAnchor;

struct Account {
    secret: [u8; 32],
    public: Vec<u8>,
    address: String,
    nonce: u64,
}

impl Account {
    fn new(secret: Vec<u8>, public: Vec<u8>) -> Self {
        let secret_arr: [u8; 32] = secret.try_into().expect("secret length");
        let address = hex::encode(&public);
        Self {
            secret: secret_arr,
            public,
            address,
            nonce: 0,
        }
    }

    fn signing_key(&self) -> SigningKey {
        SigningKey::from_bytes(&self.secret)
    }
}

fn build_anchor(account: &Account, document: String, nonce: u64) -> TxDidAnchor {
    let sk = account.signing_key();
    let mut tx = TxDidAnchor {
        address: account.address.clone(),
        public_key: account.public.clone(),
        document,
        nonce,
        signature: Vec::new(),
        remote_attestation: None,
    };
    let sig = sk.sign(tx.owner_digest().as_ref());
    tx.signature = sig.to_bytes().to_vec();
    tx
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let out_dir = args.get(1).cloned().unwrap_or_else(|| "did_sim".into());
    let account_count = args
        .get(2)
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(32);
    let total_updates = args
        .get(3)
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(account_count * 4);

    fs::create_dir_all(&out_dir).expect("create output dir");
    let did_path = format!("{out_dir}/did.db");
    let gov_path = format!("{out_dir}/gov.db");
    let explorer_path = format!("{out_dir}/explorer.db");

    std::env::set_var("TB_DID_DB_PATH", &did_path);
    let mut registry = DidRegistry::open(&did_path);
    let gov = GovStore::open(&gov_path);
    let explorer = Explorer::open(&explorer_path).expect("open explorer");

    let mut accounts = Vec::with_capacity(account_count);
    for _ in 0..account_count {
        let (sk_bytes, pk_bytes) = generate_keypair();
        accounts.push(Account::new(sk_bytes, pk_bytes));
    }

    let mut rng = StdRng::seed_from_u64(0xd1d5_cafe);
    let mut anchored = 0u64;
    let base_ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;

    for step in 0..total_updates {
        let idx = rng.gen_range(0..accounts.len());
        let account = &mut accounts[idx];
        account.nonce += 1;
        let nonce = account.nonce;
        let document_value = foundation_serialization::json!({
            "id": format!("did:tb:{}", account.address),
            "sequence": nonce,
            "updated": base_ts + step as i64,
        });
        let document = json::to_string_value(&document_value);

        let tx = build_anchor(account, document, nonce);
        match registry.anchor(&tx, Some(&gov)) {
            Ok(record) => {
                anchored += 1;
                let view: DidDocumentView = record.into();
                let _ = explorer.record_did_anchor(&view);
                let ts = base_ts + anchored as i64;
                let _ = explorer.archive_metric(&MetricPoint {
                    name: "did_anchor_total".to_string(),
                    ts,
                    value: anchored as f64,
                });
            }
            Err(err) => {
                eprintln!("anchor failed for {}: {:?}", account.address, err);
                account.nonce -= 1;
            }
        }
    }

    println!(
        "anchored {anchored} DID updates across {account_count} accounts into {}",
        explorer_path
    );

    match explorer.recent_did_records(10) {
        Ok(entries) => {
            println!("recent anchors:");
            for rec in entries {
                println!("  {} -> {} @{}", rec.address, rec.hash, rec.anchored_at);
            }
        }
        Err(err) => eprintln!("failed to load recent anchors: {err}"),
    }

    if let Ok(rate) = did_view::anchor_rate(&explorer) {
        if let Some(last) = rate.last() {
            println!(
                "latest simulated anchor rate: {:.4} anchors/sec",
                last.value
            );
        }
    }
}
