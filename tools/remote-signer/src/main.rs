use clap::Parser;
#[cfg(feature = "hid")]
use hidapi::HidApi;
use qrcode::{render::unicode, QrCode};
use std::fs;
use wallet::{psbt::Psbt, Wallet, WalletSigner};

/// Simple remote signer CLI supporting air-gapped PSBT workflows.
#[derive(Parser)]
struct Args {
    /// Input PSBT file path
    #[arg(long)]
    input: String,
    /// Output PSBT file path
    #[arg(long)]
    output: String,
    /// Render signed payload as QR to stdout
    #[arg(long)]
    qr: bool,
}

fn main() {
    let args = Args::parse();
    let data = fs::read(&args.input).expect("read input");
    let mut psbt: Psbt = serde_json::from_slice(&data).expect("parse psbt");
    let wallet = Wallet::generate();
    let sig = wallet.sign(&psbt.payload).expect("sign");
    psbt.add_signature(sig);
    let out = serde_json::to_vec(&psbt).expect("serialize");
    fs::write(&args.output, &out).expect("write");
    if args.qr {
        if let Ok(code) = QrCode::new(&out) {
            let image = code.render::<unicode::Dense1x2>().build();
            println!("{}", image);
        }
    }
}
