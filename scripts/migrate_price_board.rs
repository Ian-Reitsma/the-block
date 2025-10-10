use std::{collections::VecDeque, env, fs};
use foundation_serialization::binary;
use the_block::util::versioned_blob::{decode_blob, encode_blob, MAGIC_PRICE_BOARD};

#[derive(serde::Deserialize)]
struct V2Entry {
    price: u64,
    multiplier: f64,
}

#[derive(serde::Deserialize)]
struct V2 {
    window: usize,
    consumer: VecDeque<V2Entry>,
    industrial: VecDeque<V2Entry>,
}

#[derive(serde::Serialize)]
struct V3Entry {
    price: u64,
    weighted: u64,
}

#[derive(serde::Serialize)]
struct V3 {
    window: usize,
    consumer: VecDeque<V3Entry>,
    industrial: VecDeque<V3Entry>,
}

fn main() {
    let path = env::args().nth(1).expect("path to price board");
    let bytes = fs::read(&path).expect("read price board");
    let (ver, payload) = decode_blob(&bytes, MAGIC_PRICE_BOARD).expect("decode blob");
    if ver != 2 {
        eprintln!("unsupported version {ver}");
        std::process::exit(1);
    }
    let v2: V2 = binary::decode(payload).expect("deserialize v2");
    let conv = |v: VecDeque<V2Entry>| -> VecDeque<V3Entry> {
        v.into_iter()
            .map(|e| V3Entry {
                price: e.price,
                weighted: (e.price as f64 * e.multiplier).round() as u64,
            })
            .collect()
    };
    let v3 = V3 { window: v2.window, consumer: conv(v2.consumer), industrial: conv(v2.industrial) };
    let payload_new = binary::encode(&v3).expect("serialize v3");
    let blob = encode_blob(MAGIC_PRICE_BOARD, 3, &payload_new).expect("encode blob");
    fs::write(&path, blob).expect("write migrated board");
}
