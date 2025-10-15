use std::fs;
use std::io::{self, Error, ErrorKind};
use std::path::Path;

fn read_trimmed(path: &Path) -> io::Result<String> {
    let contents = fs::read_to_string(path)?;
    Ok(contents.trim().to_string())
}

#[cfg(any(target_os = "android", target_os = "linux", test))]
fn parse_wireless(contents: &str) -> Option<bool> {
    let mut observed = false;
    for line in contents.lines().skip(2) {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let (iface, rest) = match trimmed.split_once(':') {
            Some(split) => split,
            None => continue,
        };
        let _ = iface; // keep iface for future extensions.
        let mut fields = rest.split_whitespace();
        let _status = fields.next();
        let link_field = match fields.next() {
            Some(value) => value.trim_end_matches('.'),
            None => continue,
        };
        observed = true;
        if let Ok(value) = link_field.parse::<f32>() {
            if value > 0.0 {
                return Some(true);
            }
        }
    }
    if observed {
        Some(false)
    } else {
        None
    }
}

#[cfg(not(any(target_os = "android", target_os = "linux")))]
pub fn wifi_connected() -> io::Result<bool> {
    Err(Error::new(
        ErrorKind::Unsupported,
        "wifi connectivity not supported on this platform",
    ))
}

#[cfg(any(target_os = "android", target_os = "linux"))]
pub fn wifi_connected() -> io::Result<bool> {
    if let Ok(contents) = fs::read_to_string("/proc/net/wireless") {
        if let Some(result) = parse_wireless(&contents) {
            return Ok(result);
        }
    }

    let base = Path::new("/sys/class/net");
    let mut saw_wireless = false;
    let entries = match fs::read_dir(base) {
        Ok(entries) => entries,
        Err(err) if err.kind() == ErrorKind::NotFound => {
            return Err(Error::new(
                ErrorKind::NotFound,
                "no network interfaces directory discovered",
            ))
        }
        Err(err) => return Err(err),
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !is_wireless_interface(&path) {
            continue;
        }
        saw_wireless = true;
        let operstate = path.join("operstate");
        match read_trimmed(&operstate) {
            Ok(state) => {
                if state == "up" {
                    return Ok(true);
                }
            }
            Err(err) if err.kind() == ErrorKind::NotFound => {
                continue;
            }
            Err(err) => return Err(err),
        }
    }

    if saw_wireless {
        Ok(false)
    } else {
        Err(Error::new(
            ErrorKind::NotFound,
            "no wireless interfaces detected",
        ))
    }
}

#[cfg(any(target_os = "android", target_os = "linux"))]
fn is_wireless_interface(path: &Path) -> bool {
    path.join("wireless").exists() || path.join("phy80211").exists()
}

#[cfg(test)]
mod tests {
    use super::parse_wireless;

    #[test]
    fn detects_connected_wireless() {
        let sample = "Inter-| sta |   Quality\n face | stat | link level noise\n  wlan0: 0000   45.  -40.  -256  0  0  0  0  0\n";
        assert_eq!(parse_wireless(sample), Some(true));
    }

    #[test]
    fn detects_disconnected_wireless() {
        let sample = "Inter-| sta |   Quality\n face | stat | link level noise\n  wlan0: 0000    0.  -256  -256  0  0  0  0  0\n";
        assert_eq!(parse_wireless(sample), Some(false));
    }

    #[test]
    fn ignores_empty_listing() {
        let sample = "Inter-| sta |   Quality\n face | stat | link level noise\n";
        assert_eq!(parse_wireless(sample), None);
    }
}
