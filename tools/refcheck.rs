use std::env;
use std::fs;
use std::path::{Path, PathBuf};

fn check_file(path: &Path, root: &Path, bad: &mut Vec<String>) {
    if let Ok(content) = fs::read_to_string(path) {
        for line in content.lines() {
            let mut rest = line;
            while let Some(pos) = rest.find("](") {
                if let Some(end) = rest[pos+2..].find(')') {
                    let link = &rest[pos+2..pos+2+end];
                    if !link.starts_with("http") && !link.starts_with('#') && link.contains('/') {
                        let path_part = link.split('#').next().unwrap();
                        let target = if path_part.starts_with("docs/") {
                            root.join(&path_part[5..])
                        } else {
                            path.parent().unwrap_or(root).join(path_part)
                        };
                        if target.extension().map(|e| e == "md").unwrap_or(false) && !target.exists() {
                            bad.push(format!("{} -> {}", path.display(), link));
                        }
                    }
                    rest = &rest[pos+2+end+1..];
                } else {
                    break;
                }
            }
        }
    }
}

fn main() {
    let root = env::args().nth(1).unwrap_or_else(|| "docs".into());
    let root_path = PathBuf::from(&root);
    let mut bad = Vec::new();
    let mut stack = vec![root_path.clone()];
    while let Some(p) = stack.pop() {
        if p.is_dir() {
            for entry in fs::read_dir(&p).unwrap() {
                let e = entry.unwrap().path();
                if e.is_dir() {
                    stack.push(e);
                } else if e.extension().map(|s| s == "md").unwrap_or(false) {
                    check_file(&e, &root_path, &mut bad);
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
}
