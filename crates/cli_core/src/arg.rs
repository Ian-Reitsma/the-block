use std::borrow::Cow;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FlagSpec {
    pub name: &'static str,
    pub long: &'static str,
    pub help: &'static str,
    pub default: bool,
}

impl FlagSpec {
    pub const fn new(name: &'static str, long: &'static str, help: &'static str) -> Self {
        Self {
            name,
            long,
            help,
            default: false,
        }
    }

    pub const fn with_default(mut self, default: bool) -> Self {
        self.default = default;
        self
    }
}

#[derive(Clone, Debug)]
pub struct OptionSpec {
    pub name: &'static str,
    pub long: &'static str,
    pub help: &'static str,
    pub takes_value: bool,
    pub multiple: bool,
    pub default: Option<&'static str>,
    pub required: bool,
    pub value_enum: Option<&'static [&'static str]>,
    pub value_delimiter: Option<char>,
}

impl OptionSpec {
    pub const fn new(name: &'static str, long: &'static str, help: &'static str) -> Self {
        Self {
            name,
            long,
            help,
            takes_value: true,
            multiple: false,
            default: None,
            required: false,
            value_enum: None,
            value_delimiter: None,
        }
    }

    pub const fn takes_value(mut self, takes: bool) -> Self {
        self.takes_value = takes;
        self
    }

    pub const fn multiple(mut self, multiple: bool) -> Self {
        self.multiple = multiple;
        self
    }

    pub const fn default(mut self, value: &'static str) -> Self {
        self.default = Some(value);
        self
    }

    pub const fn required(mut self, required: bool) -> Self {
        self.required = required;
        self
    }

    pub const fn value_enum(mut self, values: &'static [&'static str]) -> Self {
        self.value_enum = Some(values);
        self
    }

    pub const fn value_delimiter(mut self, delimiter: char) -> Self {
        self.value_delimiter = Some(delimiter);
        self
    }
}

#[derive(Clone, Debug)]
pub struct PositionalSpec {
    pub name: &'static str,
    pub help: &'static str,
    pub required: bool,
    pub multiple: bool,
}

impl PositionalSpec {
    pub const fn new(name: &'static str, help: &'static str) -> Self {
        Self {
            name,
            help,
            required: true,
            multiple: false,
        }
    }

    pub const fn optional(mut self) -> Self {
        self.required = false;
        self
    }

    pub const fn multiple(mut self) -> Self {
        self.multiple = true;
        self
    }
}

#[derive(Clone, Debug)]
pub enum ArgSpec {
    Flag(FlagSpec),
    Option(OptionSpec),
    Positional(PositionalSpec),
}

impl ArgSpec {
    pub const fn long(&self) -> Option<&'static str> {
        match self {
            ArgSpec::Flag(spec) => Some(spec.long),
            ArgSpec::Option(spec) => Some(spec.long),
            ArgSpec::Positional(_) => None,
        }
    }

    pub const fn name(&self) -> &'static str {
        match self {
            ArgSpec::Flag(spec) => spec.name,
            ArgSpec::Option(spec) => spec.name,
            ArgSpec::Positional(spec) => spec.name,
        }
    }

    pub fn help(&self) -> Cow<'static, str> {
        match self {
            ArgSpec::Flag(spec) => Cow::Borrowed(spec.help),
            ArgSpec::Option(spec) => Cow::Borrowed(spec.help),
            ArgSpec::Positional(spec) => Cow::Borrowed(spec.help),
        }
    }
}
