use crate::{arg::ArgSpec, command::Command};

pub struct HelpGenerator<'a> {
    command: &'a Command,
}

impl<'a> HelpGenerator<'a> {
    pub const fn new(command: &'a Command) -> Self {
        Self { command }
    }

    pub fn render(&self) -> String {
        let mut output = String::new();
        output.push_str(&format!("{}\n\n", self.command.about));
        if !self.command.args.is_empty() {
            output.push_str("OPTIONS:\n");
            for arg in &self.command.args {
                match arg {
                    ArgSpec::Flag(flag) => {
                        output.push_str(&format!("  --{:<16} {}\n", flag.long, flag.help));
                    }
                    ArgSpec::Option(option) => {
                        output.push_str(&format!("  --{:<16} {}\n", option.long, option.help));
                    }
                    ArgSpec::Positional(positional) => {
                        output
                            .push_str(&format!("  {:<18} {}\n", positional.name, positional.help));
                    }
                }
            }
        }

        if !self.command.subcommands.is_empty() {
            output.push_str("\nSUBCOMMANDS:\n");
            for sub in &self.command.subcommands {
                output.push_str(&format!("  {:<18} {}\n", sub.name, sub.about));
            }
        }
        output
    }
}
