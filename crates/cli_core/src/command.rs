use crate::arg::ArgSpec;

#[derive(Clone, Debug)]
pub struct CommandId(pub &'static str);

#[derive(Clone, Debug)]
pub struct CommandPath<'a> {
    pub segments: Vec<&'a str>,
}

impl<'a> CommandPath<'a> {
    pub fn new(root: &'a str) -> Self {
        Self {
            segments: vec![root],
        }
    }

    pub fn with(mut self, segment: &'a str) -> Self {
        self.segments.push(segment);
        self
    }

    pub fn display(&self) -> String {
        self.segments.join(" ")
    }
}

#[derive(Clone, Debug)]
pub struct Command {
    pub id: CommandId,
    pub name: &'static str,
    pub about: &'static str,
    pub args: Vec<ArgSpec>,
    pub subcommands: Vec<Command>,
    pub allow_external_subcommands: bool,
}

impl Command {
    pub const fn new(id: CommandId, name: &'static str, about: &'static str) -> Self {
        Self {
            id,
            name,
            about,
            args: Vec::new(),
            subcommands: Vec::new(),
            allow_external_subcommands: false,
        }
    }

    pub fn with_args(mut self, args: Vec<ArgSpec>) -> Self {
        self.args = args;
        self
    }

    pub fn with_subcommands(mut self, subcommands: Vec<Command>) -> Self {
        self.subcommands = subcommands;
        self
    }

    pub fn allow_external_subcommands(mut self, allow: bool) -> Self {
        self.allow_external_subcommands = allow;
        self
    }
}

pub struct CommandBuilder {
    command: Command,
}

impl CommandBuilder {
    pub fn new(id: CommandId, name: &'static str, about: &'static str) -> Self {
        Self {
            command: Command::new(id, name, about),
        }
    }

    pub fn arg(mut self, arg: ArgSpec) -> Self {
        self.command.args.push(arg);
        self
    }

    pub fn subcommand(mut self, command: Command) -> Self {
        self.command.subcommands.push(command);
        self
    }

    pub fn allow_external_subcommands(mut self, allow: bool) -> Self {
        self.command.allow_external_subcommands = allow;
        self
    }

    pub fn build(self) -> Command {
        self.command
    }
}
