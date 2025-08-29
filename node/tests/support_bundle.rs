use std::fs;
use std::io::Read;
use std::path::PathBuf;
use std::process::Command;

#[test]
fn support_bundle_redacts_tokens() {
    let tmp = tempfile::tempdir().unwrap();
    let config = tmp.path().join("config.toml");
    fs::write(&config, "admin_token=\"SECRET\"\nprivate_key=\"ABC\"\n").unwrap();
    let datadir = tmp.path().join("data");
    fs::create_dir(&datadir).unwrap();
    let log = datadir.join("node.log");
    fs::write(&log, "hello\n").unwrap();

    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("scripts")
        .join("support_bundle.sh");
    let output = Command::new(&script)
        .env("CONFIG", &config)
        .env("DATADIR", &datadir)
        .env("LOG", &log)
        .current_dir(tmp.path())
        .output()
        .expect("run support_bundle");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let bundle_line = stdout
        .lines()
        .find(|l| l.starts_with("Bundle written to "))
        .unwrap();
    let bundle_name = bundle_line.split_whitespace().last().unwrap();
    let bundle_path = tmp.path().join(bundle_name);

    let file = fs::File::open(bundle_path).unwrap();
    let gz = flate2::read::GzDecoder::new(file);
    let mut archive = tar::Archive::new(gz);
    let mut contents = String::new();
    for entry in archive.entries().unwrap() {
        let mut e = entry.unwrap();
        if e.path().unwrap().ends_with("config.toml") {
            e.read_to_string(&mut contents).unwrap();
        }
    }
    assert!(contents.contains("admin_token=\"REDACTED\""));
    assert!(contents.contains("private_key=\"REDACTED\""));
    assert!(!contents.contains("SECRET"));
    assert!(!contents.contains("ABC"));
}
