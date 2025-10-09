use governance::{
    decode_runtime_backend_policy, decode_storage_engine_policy, decode_transport_provider_policy,
    DependencyPolicyRecord, DEFAULT_RUNTIME_BACKEND_POLICY, DEFAULT_STORAGE_ENGINE_POLICY,
    DEFAULT_TRANSPORT_PROVIDER_POLICY,
};
use std::env;
use std::path::{Path, PathBuf};
use std::{fs, process};

fn history_root(path: &Path) -> PathBuf {
    if path.is_dir() {
        path.to_path_buf()
    } else {
        path.parent()
            .map(|parent| parent.to_path_buf())
            .unwrap_or_else(|| Path::new(".").to_path_buf())
    }
}

fn usage() -> ! {
    eprintln!("usage: bootstrap_dependency_policy <gov-store-path> [--force]");
    process::exit(1);
}

fn main() {
    let mut args = env::args().skip(1);
    let Some(target) = args.next() else {
        usage();
    };
    let mut force = false;
    for arg in args {
        if arg == "--force" {
            force = true;
        } else {
            eprintln!("unknown argument: {arg}");
            usage();
        }
    }

    let history_dir = history_root(Path::new(&target)).join("governance/history");
    if let Err(err) = fs::create_dir_all(&history_dir) {
        eprintln!(
            "failed to create history directory {}: {err}",
            history_dir.display()
        );
        process::exit(1);
    }
    let output = history_dir.join("dependency_policy.json");
    if output.exists() && !force {
        eprintln!(
            "{} already exists; pass --force to overwrite",
            output.display()
        );
        process::exit(1);
    }

    let records = vec![
        DependencyPolicyRecord {
            epoch: 0,
            proposal_id: 0,
            kind: "runtime_backend".into(),
            allowed: decode_runtime_backend_policy(DEFAULT_RUNTIME_BACKEND_POLICY),
        },
        DependencyPolicyRecord {
            epoch: 0,
            proposal_id: 0,
            kind: "transport_provider".into(),
            allowed: decode_transport_provider_policy(DEFAULT_TRANSPORT_PROVIDER_POLICY),
        },
        DependencyPolicyRecord {
            epoch: 0,
            proposal_id: 0,
            kind: "storage_engine".into(),
            allowed: decode_storage_engine_policy(DEFAULT_STORAGE_ENGINE_POLICY),
        },
    ];

    match json::to_vec_pretty(&records) {
        Ok(bytes) => {
            if let Err(err) = fs::write(&output, bytes) {
                eprintln!("failed to write {}: {err}", output.display());
                process::exit(1);
            }
        }
        Err(err) => {
            eprintln!("failed to serialize dependency policy records: {err}");
            process::exit(1);
        }
    }

    println!(
        "seeded dependency policy history with current defaults at {}",
        output.display()
    );
}
