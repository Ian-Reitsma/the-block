use cli_core::{
    arg::{ArgSpec, FlagSpec, OptionSpec, PositionalSpec},
    command::{Command, CommandId},
    config::ConfigReader,
    parse::Parser,
};

fn sample_command() -> Command {
    Command {
        id: CommandId("root"),
        name: "root",
        about: "root command",
        args: vec![
            ArgSpec::Flag(FlagSpec::new("verbose", "verbose", "Enable verbose output")),
            ArgSpec::Option(
                OptionSpec::new("url", "url", "Endpoint URL").default("http://localhost:26658"),
            ),
            ArgSpec::Positional(PositionalSpec::new("action", "Action name")),
        ],
        subcommands: vec![Command {
            id: CommandId("nested"),
            name: "nested",
            about: "nested command",
            args: vec![ArgSpec::Option(
                OptionSpec::new("limit", "limit", "limit").default("10"),
            )],
            subcommands: Vec::new(),
            allow_external_subcommands: false,
        }],
        allow_external_subcommands: false,
    }
}

#[test]
fn parses_flags_and_options() {
    let command = sample_command();
    let parser = Parser::new(&command);
    let args = vec![
        "--verbose".to_string(),
        "--url".to_string(),
        "https://example".to_string(),
        "deploy".to_string(),
    ];
    let matches = parser.parse(&args).expect("parsed");
    assert!(matches.get_flag("verbose"));
    assert_eq!(
        matches.get_string("url").as_deref(),
        Some("https://example")
    );
    assert_eq!(
        matches
            .get_positional("action")
            .and_then(|values| values.first())
            .map(String::as_str),
        Some("deploy")
    );
}

#[test]
fn applies_defaults() {
    let command = sample_command();
    let parser = Parser::new(&command);
    let args = vec!["deploy".to_string()];
    let matches = parser.parse(&args).expect("parsed");
    assert_eq!(
        matches.get_string("url").as_deref(),
        Some("http://localhost:26658")
    );
    assert!(!matches.get_flag("verbose"));
}

#[test]
fn parses_subcommand() {
    let command = sample_command();
    let parser = Parser::new(&command);
    let args = vec![
        "deploy".to_string(),
        "nested".to_string(),
        "--limit".to_string(),
        "42".to_string(),
    ];
    let matches = parser.parse(&args).expect("parsed");
    let Some((name, sub)) = matches.subcommand() else {
        panic!("missing subcommand");
    };
    assert_eq!(name, "nested");
    assert_eq!(sub.get_string("limit").as_deref(), Some("42"));
}

#[test]
fn rejects_unknown_options() {
    let command = sample_command();
    let parser = Parser::new(&command);
    let args = vec!["--unknown".to_string()];
    assert!(parser.parse(&args).is_err());
}

#[test]
fn config_reader_iterates_entries() {
    let reader = ConfigReader::parse("# sample config\nhost = \"node\"\nport = 26658\n# comment\n")
        .expect("parsed config");

    let entries: Vec<(&str, &str)> = reader.entries().collect();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0], ("host", "node"));
    assert_eq!(entries[1], ("port", "26658"));
}
