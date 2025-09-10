use std::fs;
use serde_json;
mod gen {
    include!("src/lib.rs");
}

fn main() {
    println!("cargo:rerun-if-changed=metrics.json");
    println!("cargo:rerun-if-changed=dashboard_overrides.json");
    let dash = gen::generate_dashboard("metrics.json", Some("dashboard_overrides.json"));
    fs::create_dir_all("grafana").unwrap();
    fs::write(
        "grafana/dashboard.json",
        serde_json::to_string_pretty(&dash).unwrap(),
    )
    .unwrap();
    for name in ["operator", "dev"] {
        let tmpl_path = format!("templates/{}.json", name);
        if let Ok(tmpl) = fs::read_to_string(&tmpl_path) {
            let merged = dash.clone();
            let tpl: serde_json::Value = serde_json::from_str(&tmpl).unwrap();
            if let serde_json::Value::Object(mut obj) = merged {
                if let serde_json::Value::Object(tobj) = tpl {
                    for (k, v) in tobj.into_iter() {
                        obj.insert(k, v);
                    }
                }
                fs::write(
                    format!("grafana/{}.json", name),
                    serde_json::to_string_pretty(&serde_json::Value::Object(obj)).unwrap(),
                )
                .unwrap();
            }
        }
    }
}
