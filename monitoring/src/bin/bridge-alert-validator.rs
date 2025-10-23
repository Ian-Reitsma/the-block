fn main() {
    if let Err(err) = monitoring_build::validate_all_alerts() {
        eprintln!("alert validation failed: {err}");
        std::process::exit(1);
    }
}
