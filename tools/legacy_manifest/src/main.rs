use std::env;
use std::path::PathBuf;
use std::process;

use legacy_manifest::{run, Config, EngineKind};

fn usage() {
    eprintln!(
        "usage: legacy_manifest <source_dir> [--output <file>] [--engine <auto|inhouse|rocksdb|memory>]"
    );
}

fn main() {
    let mut args = env::args().skip(1);
    let mut source: Option<PathBuf> = None;
    let mut output: Option<PathBuf> = None;
    let mut engine = EngineKind::Auto;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--output" => {
                let Some(path) = args.next() else {
                    eprintln!("missing value for --output");
                    usage();
                    process::exit(1);
                };
                output = Some(PathBuf::from(path));
            }
            "--engine" => {
                let Some(label) = args.next() else {
                    eprintln!("missing value for --engine");
                    usage();
                    process::exit(1);
                };
                match EngineKind::parse(&label) {
                    Some(kind) => engine = kind,
                    None => {
                        eprintln!("unsupported engine `{label}`");
                        usage();
                        process::exit(1);
                    }
                }
            }
            "--help" | "-h" => {
                usage();
                return;
            }
            value => {
                if source.is_none() {
                    source = Some(PathBuf::from(value));
                } else {
                    eprintln!("unexpected argument `{value}`");
                    usage();
                    process::exit(1);
                }
            }
        }
    }

    let Some(source) = source else {
        usage();
        process::exit(1);
    };

    let mut config = Config::new(source);
    if let Some(output) = output {
        config = config.with_output(output);
    }
    config = config.with_engine(engine.clone());

    match run(config) {
        Ok(path) => println!("wrote legacy manifest to {}", path.display()),
        Err(err) => {
            eprintln!("error: {err}");
            process::exit(1);
        }
    }
}
