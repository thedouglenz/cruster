//! Placeholder — full clap schema lands in Task 2.

#![allow(unused)]

use std::ffi::OsString;

pub struct Cli;

impl Cli {
    pub fn try_parse<I, T>(_argv: I) -> Result<Self, clap::Error>
    where
        I: IntoIterator<Item = T>,
        T: Into<OsString> + Clone,
    {
        Ok(Self)
    }
}
