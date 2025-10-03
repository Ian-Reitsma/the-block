use std::path::PathBuf;

use cli_core::parse::Matches;

pub fn take_string(matches: &Matches, name: &str) -> Option<String> {
    matches.get_string(name)
}

pub fn require_string(matches: &Matches, name: &str) -> Result<String, String> {
    take_string(matches, name).ok_or_else(|| format!("missing required option '--{name}'"))
}

pub fn positional(matches: &Matches, name: &str) -> Option<String> {
    matches
        .get_positional(name)
        .and_then(|values| values.first().cloned())
}

pub fn require_positional(matches: &Matches, name: &str) -> Result<String, String> {
    positional(matches, name).ok_or_else(|| format!("missing positional argument '{name}'"))
}

pub fn optional_path(matches: &Matches, name: &str) -> Option<PathBuf> {
    take_string(matches, name).map(PathBuf::from)
}

pub fn parse_u64(value: Option<String>, name: &str) -> Result<Option<u64>, String> {
    value
        .map(|raw| {
            raw.parse::<u64>()
                .map_err(|_| format!("invalid value '{raw}' for '--{name}'"))
        })
        .transpose()
}

pub fn parse_u64_required(value: Option<String>, name: &str) -> Result<u64, String> {
    match parse_u64(value, name)? {
        Some(parsed) => Ok(parsed),
        None => Err(format!("missing required option '--{name}'")),
    }
}

pub fn parse_usize(value: Option<String>, name: &str) -> Result<Option<usize>, String> {
    value
        .map(|raw| {
            raw.parse::<usize>()
                .map_err(|_| format!("invalid value '{raw}' for '--{name}'"))
        })
        .transpose()
}

pub fn parse_usize_required(value: Option<String>, name: &str) -> Result<usize, String> {
    match parse_usize(value, name)? {
        Some(parsed) => Ok(parsed),
        None => Err(format!("missing required option '--{name}'")),
    }
}

pub fn parse_bool(value: Option<String>, default: bool, name: &str) -> Result<bool, String> {
    match value {
        Some(raw) => match raw.as_str() {
            "true" | "1" => Ok(true),
            "false" | "0" => Ok(false),
            _ => Err(format!("invalid value '{raw}' for '--{name}'")),
        },
        None => Ok(default),
    }
}

#[cfg_attr(not(feature = "quantum"), allow(dead_code))]
pub fn parse_required<T: std::str::FromStr>(value: Option<String>, name: &str) -> Result<T, String>
where
    T::Err: std::fmt::Display,
{
    match value {
        Some(raw) => raw
            .parse::<T>()
            .map_err(|err| format!("invalid value '{raw}' for '--{name}': {err}")),
        None => Err(format!("missing required option '--{name}'")),
    }
}

pub fn parse_optional<T: std::str::FromStr>(
    value: Option<String>,
    name: &str,
) -> Result<Option<T>, String>
where
    T::Err: std::fmt::Display,
{
    value
        .map(|raw| {
            raw.parse::<T>()
                .map_err(|err| format!("invalid value '{raw}' for '--{name}': {err}"))
        })
        .transpose()
}

pub fn parse_positional_u64(matches: &Matches, name: &str) -> Result<u64, String> {
    let value = require_positional(matches, name)?;
    value
        .parse::<u64>()
        .map_err(|_| format!("invalid value '{value}' for '{name}'"))
}

pub fn parse_positional_u32(matches: &Matches, name: &str) -> Result<u32, String> {
    let value = require_positional(matches, name)?;
    value
        .parse::<u32>()
        .map_err(|_| format!("invalid value '{value}' for '{name}'"))
}

pub fn parse_vec_u64(values: Vec<String>, name: &str) -> Result<Vec<u64>, String> {
    let mut parsed = Vec::with_capacity(values.len());
    for value in values {
        let item = value
            .parse::<u64>()
            .map_err(|_| format!("invalid value '{value}' for '--{name}'"))?;
        parsed.push(item);
    }
    Ok(parsed)
}
