#![allow(clippy::unwrap_used, clippy::expect_used)]
use the_block::utxo::script::Op;
use the_block::utxo::{Ledger, OutPoint, Script, Transaction, TxIn, TxOut};

#[test]
fn multi_input_success() {
    let mut ledger = Ledger::default();
    // seed two utxos
    let script = Script(vec![Op::Push(b"secret".to_vec()), Op::Equal]);
    let out1 = TxOut {
        value: 5,
        script_pubkey: script.clone(),
    };
    let out2 = TxOut {
        value: 7,
        script_pubkey: script.clone(),
    };
    let tx_seed = Transaction {
        inputs: vec![],
        outputs: vec![out1.clone(), out2.clone()],
    };
    let txid = tx_seed.txid();
    ledger.apply_tx(&tx_seed).unwrap();
    // spend them
    let sig = Script(vec![Op::Push(b"secret".to_vec())]);
    let tx = Transaction {
        inputs: vec![
            TxIn {
                previous_output: OutPoint { txid, index: 0 },
                script_sig: sig.clone(),
            },
            TxIn {
                previous_output: OutPoint { txid, index: 1 },
                script_sig: sig,
            },
        ],
        outputs: vec![TxOut {
            value: 12,
            script_pubkey: Script(vec![Op::True]),
        }],
    };
    assert!(ledger.apply_tx(&tx).is_ok());
}

#[test]
fn script_failure() {
    let mut ledger = Ledger::default();
    let script = Script(vec![Op::Push(b"secret".to_vec()), Op::Equal]);
    let out = TxOut {
        value: 5,
        script_pubkey: script.clone(),
    };
    let tx_seed = Transaction {
        inputs: vec![],
        outputs: vec![out],
    };
    let txid = tx_seed.txid();
    ledger.apply_tx(&tx_seed).unwrap();
    let bad_sig = Script(vec![Op::Push(b"wrong".to_vec())]);
    let tx = Transaction {
        inputs: vec![TxIn {
            previous_output: OutPoint { txid, index: 0 },
            script_sig: bad_sig,
        }],
        outputs: vec![TxOut {
            value: 5,
            script_pubkey: Script(vec![Op::True]),
        }],
    };
    assert!(ledger.apply_tx(&tx).is_err());
}
