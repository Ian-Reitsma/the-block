use std::env;
use std::fs;
use std::path::{Path, PathBuf};

fn header_from(dir: &Path) -> Option<String> {
    let path = dir.join("rocksdb/c.h");
    fs::read_to_string(path).ok()
}

fn main() {
    println!("cargo:rustc-check-cfg=cfg(rocksdb_plain_table_factory_extra_args)");
    println!("cargo:rerun-if-env-changed=ROCKSDB_INCLUDE_DIR");
    println!("cargo:rerun-if-env-changed=ROCKSDB_LIB_DIR");
    let mut candidates = Vec::new();
    if let Ok(dir) = env::var("ROCKSDB_INCLUDE_DIR") {
        candidates.push(PathBuf::from(dir));
    }
    candidates.extend(
        env::vars()
            .filter(|(key, _)| key.contains("ROCKSDB") && key.contains("INCLUDE"))
            .filter_map(|(_, value)| {
                let path = PathBuf::from(value);
                if path.exists() {
                    Some(path)
                } else {
                    None
                }
            }),
    );

    for dir in candidates {
        if let Some(contents) = header_from(&dir) {
            if contents.contains("rocksdb_options_set_plain_table_factory(")
                && contents.contains("unsigned char, unsigned char)")
            {
                println!("cargo:rustc-cfg=rocksdb_plain_table_factory_extra_args");
                break;
            }
        }
    }
}
