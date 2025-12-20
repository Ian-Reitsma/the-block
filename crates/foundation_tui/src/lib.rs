#![allow(clippy::needless_lifetimes)]
//! Terminal user-interface helpers built from first principles.
//!
//! Currently exposes ANSI colour styling with environment-aware detection so
//! command-line tools can present rich output without depending on third-party
//! crates.

use std::env;
use std::fmt;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::OnceLock;
use std::sync::{Arc, Mutex};

pub mod prompt {
    use super::{Arc, Mutex, OnceLock};
    use std::io;

    /// Prompt the user for a passphrase while suppressing terminal echo when supported.
    pub fn passphrase(prompt: &str) -> io::Result<String> {
        dispatch(prompt)
    }

    /// Prompt the user for an optional passphrase, returning `None` when left blank.
    pub fn optional_passphrase(prompt: &str) -> io::Result<Option<String>> {
        let entered = passphrase(prompt)?;
        if entered.is_empty() {
            Ok(None)
        } else {
            Ok(Some(entered))
        }
    }

    type Handler = Arc<dyn Fn(&str) -> io::Result<String> + Send + Sync>;

    fn backend() -> &'static Mutex<Option<Handler>> {
        static BACKEND: OnceLock<Mutex<Option<Handler>>> = OnceLock::new();
        BACKEND.get_or_init(|| Mutex::new(None))
    }

    fn dispatch(prompt: &str) -> io::Result<String> {
        let handler = {
            let guard = backend().lock().expect("prompt backend lock poisoned");
            guard.clone()
        };
        match handler {
            Some(custom) => custom(prompt),
            None => sys::tty::read_passphrase(prompt).map_err(io::Error::from),
        }
    }

    #[cfg(any(test, feature = "test-support"))]
    pub mod testing {
        use super::*;
        use std::panic::{catch_unwind, resume_unwind, AssertUnwindSafe};

        pub fn with_passphrase_override<F, R>(override_fn: F, action: impl FnOnce() -> R) -> R
        where
            F: Fn(&str) -> io::Result<String> + Send + Sync + 'static,
        {
            let previous = {
                let mut guard = backend().lock().expect("prompt backend lock poisoned");
                std::mem::replace(&mut *guard, Some(Arc::new(override_fn)))
            };
            let result = catch_unwind(AssertUnwindSafe(action));
            {
                let mut guard = backend().lock().expect("prompt backend lock poisoned");
                *guard = previous;
            }
            match result {
                Ok(value) => value,
                Err(panic) => resume_unwind(panic),
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn override_returns_value() {
            let value = testing::with_passphrase_override(
                |_| Ok("secret".to_string()),
                || passphrase("Prompt").expect("passphrase"),
            );
            assert_eq!(value, "secret");
        }

        #[test]
        fn optional_returns_whitespace_as_entered() {
            let result = testing::with_passphrase_override(
                |_| Ok("   ".to_string()),
                || optional_passphrase("Prompt").expect("optional"),
            );
            assert_eq!(result, Some("   ".to_string()));
        }

        #[test]
        fn optional_filters_empty_input() {
            let result = testing::with_passphrase_override(
                |_| Ok(String::new()),
                || optional_passphrase("Prompt").expect("optional"),
            );
            assert_eq!(result, None);
        }
    }
}

/// Foreground colour selections supported by the styling helpers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Color {
    Black,
    Red,
    Green,
    Yellow,
    Blue,
    Magenta,
    Cyan,
    White,
    BrightBlack,
    BrightRed,
    BrightGreen,
    BrightYellow,
    BrightBlue,
    BrightMagenta,
    BrightCyan,
    BrightWhite,
}

impl Color {
    const fn ansi_code(self) -> &'static str {
        match self {
            Color::Black => "30",
            Color::Red => "31",
            Color::Green => "32",
            Color::Yellow => "33",
            Color::Blue => "34",
            Color::Magenta => "35",
            Color::Cyan => "36",
            Color::White => "37",
            Color::BrightBlack => "90",
            Color::BrightRed => "91",
            Color::BrightGreen => "92",
            Color::BrightYellow => "93",
            Color::BrightBlue => "94",
            Color::BrightMagenta => "95",
            Color::BrightCyan => "96",
            Color::BrightWhite => "97",
        }
    }
}

/// Style attributes that can be applied to terminal text.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Style {
    foreground: Option<Color>,
    bold: bool,
    dim: bool,
    underline: bool,
}

impl Style {
    /// Create a style with no formatting.
    pub const fn plain() -> Self {
        Self {
            foreground: None,
            bold: false,
            dim: false,
            underline: false,
        }
    }

    /// Apply a foreground colour.
    pub const fn with_foreground(mut self, color: Color) -> Self {
        self.foreground = Some(color);
        self
    }

    /// Enable bold output when supported.
    pub const fn bold(mut self) -> Self {
        self.bold = true;
        self
    }

    /// Enable dim output when supported.
    pub const fn dim(mut self) -> Self {
        self.dim = true;
        self
    }

    /// Underline the text when supported.
    pub const fn underline(mut self) -> Self {
        self.underline = true;
        self
    }

    const fn is_plain(self) -> bool {
        !self.bold && !self.dim && !self.underline && self.foreground.is_none()
    }
}

impl Default for Style {
    fn default() -> Self {
        Self::plain()
    }
}

/// A styled string that renders with ANSI escape sequences when colouring is
/// enabled for the current process.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StyledString {
    text: String,
    style: Style,
}

impl StyledString {
    /// Build a styled string from owned text and a `Style` specification.
    pub fn new(text: String, style: Style) -> Self {
        Self { text, style }
    }

    /// Return the underlying text.
    pub fn as_str(&self) -> &str {
        &self.text
    }

    /// Consume the styled string, yielding the owned text.
    pub fn into_inner(self) -> String {
        self.text
    }

    /// Return the associated style.
    pub fn style(&self) -> Style {
        self.style
    }
}

impl fmt::Display for StyledString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if !colors_enabled() || self.style.is_plain() {
            return f.write_str(&self.text);
        }

        let mut first = true;
        f.write_str("\u{1b}[")?;

        if let Some(color) = self.style.foreground {
            f.write_str(color.ansi_code())?;
            first = false;
        }

        if self.style.bold {
            if !first {
                f.write_str(";")?;
            }
            f.write_str("1")?;
            first = false;
        }

        if self.style.dim {
            if !first {
                f.write_str(";")?;
            }
            f.write_str("2")?;
            first = false;
        }

        if self.style.underline {
            if !first {
                f.write_str(";")?;
            }
            f.write_str("4")?;
        }

        f.write_str("m")?;
        f.write_str(&self.text)?;
        f.write_str("\u{1b}[0m")
    }
}

/// Trait implemented for string types to provide fluent colour helpers similar
/// to the legacy `colored` crate.
pub trait Colorize: Sized {
    /// Convert `self` into an owned `String` for styling purposes.
    fn into_owned(self) -> String;

    /// Apply an arbitrary style to the string, yielding a [`StyledString`].
    fn with_style(self, style: Style) -> StyledString {
        StyledString::new(self.into_owned(), style)
    }

    /// Apply a colour directly using a [`Color`] enum variant.
    fn color(self, color: Color) -> StyledString {
        self.with_style(Style::plain().with_foreground(color))
    }

    /// Paint the string red.
    fn red(self) -> StyledString {
        self.color(Color::Red)
    }

    /// Paint the string green.
    fn green(self) -> StyledString {
        self.color(Color::Green)
    }

    /// Paint the string yellow.
    fn yellow(self) -> StyledString {
        self.color(Color::Yellow)
    }

    /// Paint the string blue.
    fn blue(self) -> StyledString {
        self.color(Color::Blue)
    }

    /// Paint the string magenta.
    fn magenta(self) -> StyledString {
        self.color(Color::Magenta)
    }

    /// Paint the string cyan.
    fn cyan(self) -> StyledString {
        self.color(Color::Cyan)
    }

    /// Paint the string white.
    fn white(self) -> StyledString {
        self.color(Color::White)
    }
}

impl Colorize for String {
    fn into_owned(self) -> String {
        self
    }
}

impl<'a> Colorize for &'a str {
    fn into_owned(self) -> String {
        self.to_owned()
    }
}

/// Shared colour preference across the process.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Preference {
    Always,
    Never,
    Auto,
}

static PREFERENCE: OnceLock<Preference> = OnceLock::new();
static OVERRIDE: AtomicU8 = AtomicU8::new(OVERRIDE_NONE);

const OVERRIDE_FALSE: u8 = 0;
const OVERRIDE_TRUE: u8 = 1;
const OVERRIDE_NONE: u8 = 2;

fn colors_enabled() -> bool {
    match OVERRIDE.load(Ordering::Relaxed) {
        OVERRIDE_FALSE => return false,
        OVERRIDE_TRUE => return true,
        _ => {}
    }

    match *PREFERENCE.get_or_init(detect_preference) {
        Preference::Always => true,
        Preference::Never => false,
        Preference::Auto => sys::tty::stdout_is_terminal(),
    }
}

fn detect_preference() -> Preference {
    if let Some(pref) = env_preference("TB_COLOR") {
        return pref;
    }

    if env_flag_is_true("CLICOLOR_FORCE") {
        return Preference::Always;
    }

    if env::var_os("TB_NO_COLOR").is_some() || env::var_os("NO_COLOR").is_some() {
        return Preference::Never;
    }

    if let Some(pref) = env_preference("CLICOLOR") {
        return pref;
    }

    Preference::Auto
}

fn env_flag_is_true(name: &str) -> bool {
    env::var(name)
        .ok()
        .map(|v| {
            !matches!(
                v.trim(),
                "" | "0" | "false" | "False" | "no" | "off" | "OFF"
            )
        })
        .unwrap_or(false)
}

fn env_preference(name: &str) -> Option<Preference> {
    let value = env::var(name).ok()?;
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }

    let lower = trimmed.to_ascii_lowercase();
    match lower.as_str() {
        "always" | "force" | "forced" | "on" | "true" | "1" => Some(Preference::Always),
        "never" | "off" | "false" | "0" => Some(Preference::Never),
        "auto" | "automatic" | "tty" => Some(Preference::Auto),
        _ => None,
    }
}

/// Force an override for colour handling. Intended for tests.
pub fn set_color_override(force: Option<bool>) {
    let value = match force {
        Some(true) => OVERRIDE_TRUE,
        Some(false) => OVERRIDE_FALSE,
        None => OVERRIDE_NONE,
    };
    OVERRIDE.store(value, Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn styling_applies_when_forced() {
        set_color_override(Some(true));
        let rendered = format!("{}", "alarm".red());
        assert_eq!(rendered, "\u{1b}[31malarm\u{1b}[0m");
        set_color_override(None);
    }

    #[test]
    fn styling_is_suppressed_when_disabled() {
        set_color_override(Some(false));
        let rendered = format!("{}", "alarm".red());
        assert_eq!(rendered, "alarm");
        set_color_override(None);
    }
}
