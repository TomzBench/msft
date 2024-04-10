//! util

#[cfg(test)]
mod tests;

pub mod guid;
pub mod hkey;
pub mod macros;
pub mod wchar;

pub(crate) mod sealed {
    pub trait Sealed {}
}
