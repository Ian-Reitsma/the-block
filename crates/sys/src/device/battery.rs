#[cfg(any(target_os = "android", target_os = "linux"))]
use std::fs;
use std::io::{self, Error, ErrorKind};
#[cfg(any(target_os = "android", target_os = "linux"))]
use std::path::Path;

#[cfg(any(target_os = "android", target_os = "linux"))]
fn read_trimmed(path: &Path) -> io::Result<String> {
    let contents = fs::read_to_string(path)?;
    Ok(contents.trim().to_string())
}

#[cfg(any(target_os = "android", target_os = "linux"))]
const CAPACITY_PATHS: &[&str] = &[
    "/sys/class/power_supply/battery/capacity",
    "/sys/class/power_supply/Battery/capacity",
    "/sys/class/power_supply/bms/capacity",
    "/sys/class/power_supply/max170xx_battery/capacity",
];

#[cfg(any(target_os = "android", target_os = "linux"))]
const STATUS_PATHS: &[&str] = &[
    "/sys/class/power_supply/battery/status",
    "/sys/class/power_supply/Battery/status",
    "/sys/class/power_supply/bms/status",
];

#[cfg(not(any(target_os = "android", target_os = "linux")))]
pub fn capacity_percent() -> io::Result<u8> {
    Err(Error::new(
        ErrorKind::Unsupported,
        "battery capacity not supported on this platform",
    ))
}

#[cfg(any(target_os = "android", target_os = "linux"))]
pub fn capacity_percent() -> io::Result<u8> {
    for candidate in CAPACITY_PATHS {
        let path = Path::new(candidate);
        match read_trimmed(path) {
            Ok(value) => match value.parse::<i32>() {
                Ok(percent) => {
                    let clamped = percent.clamp(0, 100) as u8;
                    return Ok(clamped);
                }
                Err(err) => {
                    return Err(Error::new(
                        ErrorKind::InvalidData,
                        format!(
                            "failed to parse battery capacity `{value}` from {candidate}: {err}"
                        ),
                    ));
                }
            },
            Err(err) if err.kind() == ErrorKind::NotFound => continue,
            Err(err) => return Err(err),
        }
    }
    Err(Error::new(
        ErrorKind::NotFound,
        "no battery capacity source discovered",
    ))
}

#[cfg(not(any(target_os = "android", target_os = "linux")))]
pub fn is_charging() -> io::Result<bool> {
    Err(Error::new(
        ErrorKind::Unsupported,
        "charging status not supported on this platform",
    ))
}

#[cfg(any(target_os = "android", target_os = "linux"))]
pub fn is_charging() -> io::Result<bool> {
    let mut last_error: Option<io::Error> = None;
    for candidate in STATUS_PATHS {
        let path = Path::new(candidate);
        match read_trimmed(path) {
            Ok(value) => {
                let lower = value.to_ascii_lowercase();
                if lower.contains("charging") || lower.contains("full") {
                    return Ok(true);
                }
                if lower.contains("discharging")
                    || lower.contains("not charging")
                    || lower.contains("unknown")
                {
                    return Ok(false);
                }
                return Ok(false);
            }
            Err(err) if err.kind() == ErrorKind::NotFound => {
                last_error = Some(err);
                continue;
            }
            Err(err) => return Err(err),
        }
    }
    Err(last_error
        .unwrap_or_else(|| Error::new(ErrorKind::NotFound, "no battery status source discovered")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tempfile::TempDir;
    use std::fs::File;
    use std::io::Write;

    #[test]
    #[cfg(any(target_os = "android", target_os = "linux"))]
    fn parses_capacity_values() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("capacity");
        let mut file = File::create(&path).unwrap();
        writeln!(file, "85").unwrap();
        file.flush().unwrap();
        let value = read_trimmed(&path).unwrap();
        assert_eq!(value, "85");
    }
}
