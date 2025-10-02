use cli_core::{
    arg::{ArgSpec, OptionSpec},
    command::{Command, CommandBuilder, CommandId},
    parse::Matches,
};
use state::difficulty_history;
use std::path::PathBuf;

pub enum DifficultyCmd {
    /// Inspect recent difficulty retarget calculations
    Inspect { path: String, last: usize },
}

impl DifficultyCmd {
    pub fn command() -> Command {
        CommandBuilder::new(
            CommandId("difficulty"),
            "difficulty",
            "Difficulty utilities",
        )
        .subcommand(
            CommandBuilder::new(
                CommandId("difficulty.inspect"),
                "inspect",
                "Inspect recent difficulty retarget calculations",
            )
            .arg(ArgSpec::Option(
                OptionSpec::new("path", "path", "Difficulty history directory")
                    .default("./difficulty"),
            ))
            .arg(ArgSpec::Option(
                OptionSpec::new("last", "last", "Number of entries to print").default("10"),
            ))
            .build(),
        )
        .build()
    }

    pub fn from_matches(matches: &Matches) -> std::result::Result<Self, String> {
        let (name, sub_matches) = matches
            .subcommand()
            .ok_or_else(|| "missing subcommand for 'difficulty'".to_string())?;

        match name {
            "inspect" => {
                let path = sub_matches
                    .get_string("path")
                    .unwrap_or_else(|| "./difficulty".to_string());
                let last = sub_matches
                    .get_string("last")
                    .unwrap_or_else(|| "10".to_string())
                    .parse::<usize>()
                    .map_err(|err| format!("invalid value for '--last': {err}"))?;
                Ok(DifficultyCmd::Inspect { path, last })
            }
            other => Err(format!("unknown subcommand '{other}'")),
        }
    }
}

pub fn handle(cmd: DifficultyCmd) {
    match cmd {
        DifficultyCmd::Inspect { path, last } => {
            let p = PathBuf::from(path);
            for (h, d) in difficulty_history::recent(&p, last) {
                println!("{}: {}", h, d);
            }
        }
    }
}
