use std::env;
use std::fs;
use std::path::{Path, PathBuf};

const FORBIDDEN_PATTERNS: &[&str] = &[
    "tokio::spawn",
    "tokio::task::spawn",
    "tokio::task::spawn_blocking",
    "tokio::task::yield_now",
    "tokio::time::sleep",
    "tokio::time::timeout",
    "tokio::time::interval",
    "tokio::runtime::Runtime::block_on",
    "tokio::runtime::Runtime::spawn",
    "tokio::runtime::Runtime::spawn_blocking",
    "tokio::runtime::Builder::build",
    "tokio::runtime::Builder::enable_all",
    "tokio::runtime::Builder::new_current_thread",
    "tokio::runtime::Builder::new_multi_thread",
    "tokio::select!",
];

const FORBIDDEN_ALLOWLIST: &[&str] = &[
    "crates/runtime/",
    "crates/light-client/",
    "vendor/",
    "tools/refcheck.rs",
];

fn check_file(
    path: &Path,
    root: &Path,
    bad: &mut Vec<String>,
    forbidden: &mut Vec<String>,
    is_code: bool,
) {
    if let Ok(content) = fs::read_to_string(path) {
        for line in content.lines() {
            let line = if is_code {
                let trimmed = line.trim_start();
                if !trimmed.starts_with("//") {
                    continue;
                }
                trimmed.trim_start_matches('/').trim_start()
            } else {
                line
            };
            let mut rest = line;
            while let Some(pos) = rest.find("](") {
                if let Some(end) = rest[pos + 2..].find(')') {
                    let link = &rest[pos + 2..pos + 2 + end];
                    if !link.starts_with("http") && !link.starts_with('#') && link.contains('/') {
                        let path_part = link.split('#').next().unwrap();
                        let target = if path_part.starts_with("docs/") {
                            root.join(path_part)
                        } else {
                            path.parent().unwrap_or(root).join(path_part)
                        };
                        if target.extension().map(|e| e == "md").unwrap_or(false)
                            && !target.exists()
                        {
                            bad.push(format!("{} -> {}", path.display(), link));
                        }
                    }
                    rest = &rest[pos + 2 + end + 1..];
                } else {
                    break;
                }
            }
        }
        if is_code {
            check_forbidden_tokens(path, &content, forbidden);
        }
    }
}

fn check_forbidden_tokens(path: &Path, content: &str, forbidden: &mut Vec<String>) {
    let path_str = path.to_string_lossy();
    if FORBIDDEN_ALLOWLIST
        .iter()
        .any(|allow| path_str.contains(allow))
    {
        return;
    }
    for (idx, line) in content.lines().enumerate() {
        if line.contains("clippy::disallowed_methods") || line.contains("clippy::disallowed_types")
        {
            continue;
        }
        if let Some(pattern) = FORBIDDEN_PATTERNS.iter().find(|pat| line.contains(*pat)) {
            forbidden.push(format!(
                "{}:{} contains forbidden Tokio usage `{}`",
                path.display(),
                idx + 1,
                pattern
            ));
        }
    }
}

fn main() {
    let root = env::args().nth(1).unwrap_or_else(|| ".".into());
    let root_path = PathBuf::from(&root);
    let mut bad = Vec::new();
    let mut forbidden = Vec::new();
    let mut stack = vec![root_path.clone()];
    while let Some(p) = stack.pop() {
        if p.is_dir() {
            for entry in fs::read_dir(&p).unwrap() {
                let e = entry.unwrap().path();
                if e.is_dir() {
                    stack.push(e);
                } else if e.extension().map(|s| s == "md").unwrap_or(false) {
                    check_file(&e, &root_path, &mut bad, &mut forbidden, false);
                } else if e.extension().map(|s| s == "rs").unwrap_or(false) {
                    check_file(&e, &root_path, &mut bad, &mut forbidden, true);
                }
            }
        }
    }
    if !bad.is_empty() {
        for b in bad {
            eprintln!("Missing reference: {}", b);
        }
        std::process::exit(1);
    }
    if !forbidden.is_empty() {
        for entry in forbidden {
            eprintln!("Forbidden Tokio usage: {}", entry);
        }
        std::process::exit(1);
    }
}
