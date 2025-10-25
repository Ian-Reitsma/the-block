use std::collections::HashMap;

use thiserror::Error;

use crate::{
    arg::{ArgSpec, PositionalSpec},
    command::{Command, CommandPath},
    value::ParsedValue,
};

#[derive(Debug)]
pub struct Parser<'a> {
    command: &'a Command,
}

impl<'a> Parser<'a> {
    pub fn new(command: &'a Command) -> Self {
        Self { command }
    }

    pub fn parse(&self, args: &[String]) -> Result<Matches, ParseError> {
        parse_command(self.command, args, &mut CommandPath::new(self.command.name))
    }
}

#[derive(Debug, Clone)]
pub struct Matches {
    values: HashMap<&'static str, ParsedValue>,
    positionals: HashMap<&'static str, Vec<String>>,
    subcommand: Option<(String, Box<Matches>)>,
}

impl Matches {
    pub fn new() -> Self {
        Self {
            values: HashMap::new(),
            positionals: HashMap::new(),
            subcommand: None,
        }
    }

    pub fn get(&self, name: &str) -> Option<&ParsedValue> {
        self.values.get(name)
    }

    pub fn get_positional(&self, name: &str) -> Option<&[String]> {
        self.positionals.get(name).map(|values| values.as_slice())
    }

    pub fn get_flag(&self, name: &str) -> bool {
        self.get(name).map(|value| value.as_bool()).unwrap_or(false)
    }

    pub fn get_string(&self, name: &str) -> Option<String> {
        self.get(name)
            .and_then(|value| value.as_str())
            .map(ToOwned::to_owned)
    }

    pub fn get_strings(&self, name: &str) -> Vec<String> {
        self.get(name)
            .map(|value| {
                value
                    .as_strings()
                    .into_iter()
                    .map(ToOwned::to_owned)
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn subcommand(&self) -> Option<(&str, &Matches)> {
        self.subcommand
            .as_ref()
            .map(|(name, matches)| (name.as_str(), matches.as_ref()))
    }
}

impl Default for Matches {
    fn default() -> Self {
        Self::new()
    }
}

fn parse_command(
    command: &Command,
    args: &[String],
    path: &mut CommandPath<'_>,
) -> Result<Matches, ParseError> {
    let mut matches = Matches::new();
    let positional_specs: Vec<&PositionalSpec> = command
        .args
        .iter()
        .filter_map(|spec| match spec {
            ArgSpec::Positional(positional) => Some(positional),
            _ => None,
        })
        .collect();
    let mut positional_index = 0;
    let mut index = 0;

    while index < args.len() {
        let token = &args[index];

        if token == "--help" || token == "-h" {
            return Err(ParseError::HelpRequested(path.display()));
        }

        if token == "help" {
            let requested = if index + 1 < args.len() {
                args[index + 1].clone()
            } else {
                command.name.to_string()
            };
            let mut requested_path = path.display();
            if requested != command.name {
                requested_path = format!("{} {}", requested_path, requested);
            }
            return Err(ParseError::HelpRequested(requested_path));
        }

        if token.starts_with("--") {
            parse_option_token(command, args, &mut index, &mut matches)?;
            continue;
        }

        if let Some(spec) = positional_specs.get(positional_index) {
            matches
                .positionals
                .entry(spec.name)
                .or_default()
                .push(token.clone());
            index += 1;
            if !spec.multiple {
                positional_index += 1;
            }
            continue;
        }

        if let Some(subcommand) = command
            .subcommands
            .iter()
            .find(|cmd| cmd.name == token.as_str())
        {
            let rest = &args[index + 1..];
            path.segments.push(subcommand.name);
            let sub_matches = parse_command(subcommand, rest, path)?;
            path.segments.pop();
            matches.subcommand = Some((subcommand.name.to_string(), Box::new(sub_matches)));
            break;
        }

        if command.allow_external_subcommands {
            matches.subcommand = Some((token.clone(), Box::new(Matches::new())));
            break;
        } else {
            return Err(ParseError::UnknownSubcommand(token.clone()));
        }
    }

    apply_defaults(command, &mut matches);
    validate_required(command, &matches)?;
    Ok(matches)
}

fn parse_option_token(
    command: &Command,
    args: &[String],
    index: &mut usize,
    matches: &mut Matches,
) -> Result<(), ParseError> {
    let token = &args[*index];
    let (key, inline_value) = if let Some(eq) = token.find('=') {
        (&token[2..eq], Some(token[eq + 1..].to_string()))
    } else {
        (&token[2..], None)
    };

    let spec = command
        .args
        .iter()
        .find(|spec| matches!(spec.long(), Some(long) if long == key))
        .ok_or_else(|| ParseError::UnknownOption(key.to_string()))?;

    match spec {
        ArgSpec::Flag(flag) => {
            matches.values.insert(flag.name, ParsedValue::Flag(true));
            *index += 1;
        }
        ArgSpec::Option(option) => {
            let mut consumed = 1;
            let mut raw_values: Vec<String> = Vec::new();

            if option.takes_value {
                let mut first = inline_value;
                if first.is_none() {
                    let next_index = *index + consumed;
                    first = args.get(next_index).cloned();
                    if first.is_none() {
                        return Err(ParseError::MissingValue(key.to_string()));
                    }
                    consumed += 1;
                }

                if let Some(value) = first {
                    raw_values.push(value);
                }

                if let Some(required) = option.required_values {
                    while raw_values.len() < required {
                        let next_index = *index + consumed;
                        if let Some(value) = args.get(next_index).cloned() {
                            raw_values.push(value);
                            consumed += 1;
                        } else {
                            return Err(ParseError::MissingValue(key.to_string()));
                        }
                    }
                }
            } else if let Some(value) = inline_value {
                raw_values.push(value);
            } else {
                raw_values.push(String::new());
            }

            let mut values: Vec<String> = Vec::new();
            if let Some(delim) = option.value_delimiter {
                for raw in raw_values {
                    values.extend(
                        raw.split(delim)
                            .filter(|segment| !segment.is_empty())
                            .map(|segment| segment.trim().to_string()),
                    );
                }
            } else {
                values = raw_values;
            }

            if let Some(required) = option.required_values {
                if values.len() < required {
                    return Err(ParseError::MissingValue(key.to_string()));
                }
            }

            if let Some(allowed) = option.value_enum {
                for value in &values {
                    if !allowed.contains(&value.as_str()) {
                        return Err(ParseError::InvalidChoice {
                            option: key.to_string(),
                            value: value.clone(),
                        });
                    }
                }
            }

            let parse_as_many =
                option.value_delimiter.is_some() || option.multiple || values.len() > 1;
            let parsed = if parse_as_many {
                ParsedValue::Many(values)
            } else if let Some(value) = values.into_iter().next() {
                ParsedValue::Single(value)
            } else {
                ParsedValue::None
            };

            if option.multiple {
                matches
                    .values
                    .entry(option.name)
                    .and_modify(|existing| match (existing.clone(), parsed.clone()) {
                        (ParsedValue::Many(mut left), ParsedValue::Many(right)) => {
                            left.extend(right);
                            *existing = ParsedValue::Many(left);
                        }
                        (ParsedValue::Many(mut left), ParsedValue::Single(right)) => {
                            left.push(right);
                            *existing = ParsedValue::Many(left);
                        }
                        (ParsedValue::Single(left), ParsedValue::Many(mut right)) => {
                            let mut collected = vec![left];
                            collected.append(&mut right);
                            *existing = ParsedValue::Many(collected);
                        }
                        (ParsedValue::Single(left), ParsedValue::Single(right)) => {
                            *existing = ParsedValue::Many(vec![left, right]);
                        }
                        (_, ParsedValue::None) => {}
                        (ParsedValue::None, other) => {
                            *existing = other;
                        }
                        _ => {
                            *existing = parsed.clone();
                        }
                    })
                    .or_insert(parsed);
            } else {
                matches.values.insert(option.name, parsed);
            }

            *index += consumed;
        }
        ArgSpec::Positional(_) => unreachable!("positional spec matched as option"),
    }

    Ok(())
}

fn apply_defaults(command: &Command, matches: &mut Matches) {
    for spec in &command.args {
        match spec {
            ArgSpec::Flag(flag) => {
                matches
                    .values
                    .entry(flag.name)
                    .or_insert(ParsedValue::Flag(flag.default));
            }
            ArgSpec::Option(option) => {
                if let Some(default) = option.default {
                    matches
                        .values
                        .entry(option.name)
                        .or_insert_with(|| ParsedValue::Single(default.to_string()));
                }
            }
            ArgSpec::Positional(_) => {}
        }
    }
}

fn validate_required(command: &Command, matches: &Matches) -> Result<(), ParseError> {
    for spec in &command.args {
        match spec {
            ArgSpec::Option(option) if option.required => {
                if !matches.values.contains_key(option.name) {
                    return Err(ParseError::MissingOption(option.name.to_string()));
                }
            }
            ArgSpec::Positional(positional) if positional.required => {
                if matches
                    .positionals
                    .get(positional.name)
                    .map(|values| values.is_empty())
                    .unwrap_or(true)
                {
                    return Err(ParseError::MissingPositional(positional.name.to_string()));
                }
            }
            _ => {}
        }
    }
    Ok(())
}

#[derive(Debug, Error)]
pub enum ParseError {
    #[error("unknown option '--{0}'")]
    UnknownOption(String),
    #[error("missing value for '--{0}'")]
    MissingValue(String),
    #[error("invalid choice '{value}' for '--{option}'")]
    InvalidChoice { option: String, value: String },
    #[error("invalid value '{value}' for '--{option}': expected {expected}")]
    InvalidValue {
        option: String,
        value: String,
        expected: &'static str,
    },
    #[error("unknown subcommand '{0}'")]
    UnknownSubcommand(String),
    #[error("missing required option '{0}'")]
    MissingOption(String),
    #[error("missing positional argument '{0}'")]
    MissingPositional(String),
    #[error("help requested for '{0}'")]
    HelpRequested(String),
}
