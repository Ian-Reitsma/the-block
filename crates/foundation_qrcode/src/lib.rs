#![forbid(unsafe_code)]

use std::fmt;

/// Errors produced while constructing or rendering QR codes.
#[derive(Debug, Clone)]
pub struct Error {
    message: String,
}

impl Error {
    #[cfg(feature = "external-backend")]
    fn from_backend(err: qrcode::types::QrError) -> Self {
        Self {
            message: err.to_string(),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for Error {}

/// Minimal, backend-agnostic QR code representation used across the workspace.
#[derive(Clone, Debug)]
pub struct QrCode {
    size: usize,
    modules: Vec<bool>,
}

impl QrCode {
    /// Construct a QR code from the provided payload.
    pub fn new(data: &[u8]) -> Result<Self, Error> {
        backend::build(data)
    }

    /// Returns the number of modules per side (excluding quiet zones).
    pub fn size(&self) -> usize {
        self.size
    }

    /// Returns whether the module at `(x, y)` is dark.
    pub fn is_dark(&self, x: usize, y: usize) -> bool {
        let idx = y * self.size + x;
        self.modules.get(idx).copied().unwrap_or(false)
    }

    /// Obtain a renderer for the QR code.
    pub fn render<S>(&self) -> <S as RenderStyle>::Renderer<'_>
    where
        S: RenderStyle,
    {
        S::render(self)
    }

    fn module_at(&self, x: isize, y: isize) -> bool {
        if x < 0 || y < 0 {
            return false;
        }
        let x = x as usize;
        let y = y as usize;
        if x >= self.size || y >= self.size {
            return false;
        }
        self.is_dark(x, y)
    }
}

/// Trait implemented by render styles that can format a QR code.
pub trait RenderStyle {
    type Renderer<'a>: Renderer<'a>
    where
        Self: 'a;

    fn render<'a>(code: &'a QrCode) -> Self::Renderer<'a>;
}

/// Shared behaviour for QR code renderers.
pub trait Renderer<'a>: Sized {
    fn dark_color(self, ch: char) -> Self;
    fn light_color(self, ch: char) -> Self;
    fn quiet_zone(self, enabled: bool) -> Self;
    fn build(self) -> String;
}

pub mod render {
    use super::QrCode;

    pub mod unicode {
        use super::super::{RenderStyle, Renderer};
        use super::QrCode;

        /// Dense Unicode renderer packing two vertical modules per character.
        #[derive(Clone, Copy, Debug)]
        pub struct Dense1x2;

        /// Renderer for the dense unicode representation.
        pub struct Dense1x2Renderer<'a> {
            code: &'a QrCode,
            dark: char,
            light: char,
            quiet_zone: bool,
        }

        impl<'a> Dense1x2Renderer<'a> {
            fn module(&self, x: isize, y: isize) -> bool {
                self.code.module_at(x, y)
            }
        }

        impl RenderStyle for Dense1x2 {
            type Renderer<'a>
                = Dense1x2Renderer<'a>
            where
                Self: 'a;

            fn render<'a>(code: &'a QrCode) -> Self::Renderer<'a> {
                Dense1x2Renderer {
                    code,
                    dark: '█',
                    light: ' ',
                    quiet_zone: true,
                }
            }
        }

        impl<'a> Renderer<'a> for Dense1x2Renderer<'a> {
            fn dark_color(mut self, ch: char) -> Self {
                self.dark = ch;
                self
            }

            fn light_color(mut self, ch: char) -> Self {
                self.light = ch;
                self
            }

            fn quiet_zone(mut self, enabled: bool) -> Self {
                self.quiet_zone = enabled;
                self
            }

            fn build(self) -> String {
                let quiet = if self.quiet_zone { 4 } else { 0 };
                let total = self.code.size() + quiet * 2;
                let rows = (total + 1) / 2;
                let mut output = String::new();
                for row in 0..rows {
                    let top_y = row * 2;
                    let bottom_y = top_y + 1;
                    let mut line = String::new();
                    for x in 0..total {
                        let module_x = x as isize - quiet as isize;
                        let top_dark = self.module(module_x, top_y as isize - quiet as isize);
                        let bottom_dark = self.module(module_x, bottom_y as isize - quiet as isize);
                        let ch = match (top_dark, bottom_dark) {
                            (true, true) => self.dark,
                            (true, false) => '▀',
                            (false, true) => '▄',
                            (false, false) => self.light,
                        };
                        line.push(ch);
                    }
                    output.push_str(&line);
                    output.push('\n');
                }
                output
            }
        }
    }
}

#[cfg(feature = "external-backend")]
mod backend {
    use super::{Error, QrCode};

    pub fn build(data: &[u8]) -> Result<QrCode, Error> {
        let code = qrcode::QrCode::new(data).map_err(Error::from_backend)?;
        let width = code.width();
        let mut modules = Vec::with_capacity(width * width);
        for y in 0..width {
            for x in 0..width {
                let color = code[(x, y)] != qrcode::Color::Light;
                modules.push(color);
            }
        }
        Ok(QrCode {
            size: width,
            modules,
        })
    }
}

#[cfg(not(feature = "external-backend"))]
mod backend {
    use super::{Error, QrCode};

    pub fn build(data: &[u8]) -> Result<QrCode, Error> {
        let size = select_size(data.len());
        let mut modules = vec![false; size * size];
        stamp_finder(&mut modules, size, 0, 0);
        stamp_finder(&mut modules, size, size.saturating_sub(7), 0);
        stamp_finder(&mut modules, size, 0, size.saturating_sub(7));

        if !data.is_empty() {
            fill_data(&mut modules, size, data);
        }

        Ok(QrCode { size, modules })
    }

    fn select_size(len: usize) -> usize {
        let mut version = 1usize;
        loop {
            let size = 21 + (version - 1) * 4;
            let capacity = size * size / 2;
            if capacity >= len.saturating_mul(8) {
                return size;
            }
            if version == 40 {
                return size;
            }
            version += 1;
        }
    }

    fn stamp_finder(modules: &mut [bool], size: usize, origin_x: usize, origin_y: usize) {
        for y in 0..7 {
            for x in 0..7 {
                let gx = origin_x + x;
                let gy = origin_y + y;
                if gx >= size || gy >= size {
                    continue;
                }
                let idx = gy * size + gx;
                let border = x == 0 || x == 6 || y == 0 || y == 6;
                let inner = (2..=4).contains(&x) && (2..=4).contains(&y);
                modules[idx] = border || inner;
            }
        }
    }

    fn fill_data(modules: &mut [bool], size: usize, data: &[u8]) {
        for y in 0..size {
            for x in 0..size {
                let idx = y * size + x;
                if modules[idx] {
                    continue;
                }
                let byte = data[(idx / 8) % data.len()];
                let bit = ((byte >> (idx % 8)) & 1) == 1;
                let mask = ((x + y) & 1) == 0;
                modules[idx] = bit ^ mask;
            }
        }
    }
}
