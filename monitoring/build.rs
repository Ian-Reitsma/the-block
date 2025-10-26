use foundation_serialization::json::{self, Value};
use std::fs;
mod gen {
    include!("src/dashboard.rs");
}

fn main() {
    println!("cargo:rerun-if-changed=metrics.json");
    println!("cargo:rerun-if-changed=dashboard_overrides.json");
    let dash = gen::generate_dashboard("metrics.json", Some("dashboard_overrides.json"))
        .unwrap_or_else(|err| panic!("failed to build dashboard: {err}"));
    fs::create_dir_all("grafana").unwrap();
    let rendered = gen::render_pretty(&dash).expect("render dashboard");
    fs::write("grafana/dashboard.json", rendered).unwrap();
    if let Ok(entries) = fs::read_dir("templates") {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("json") {
                if let Some(name) = path.file_stem().and_then(|s| s.to_str()) {
                    if let Ok(tmpl) = fs::read_to_string(&path) {
                        let mut merged = dash.clone();
                        let tpl: Value = json::value_from_str(&tmpl).unwrap_or_else(|err| {
                            panic!("invalid template '{}': {err}", path.display())
                        });
                        gen::apply_overrides(&mut merged, tpl).unwrap_or_else(|err| {
                            panic!("failed to merge template '{}': {err}", path.display())
                        });
                        let rendered = gen::render_pretty(&merged).expect("render template");
                        fs::write(format!("grafana/{}.json", name), rendered).unwrap();
                    }
                }
            }
        }
    }
}
