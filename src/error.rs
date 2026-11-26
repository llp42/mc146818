// SPDX-FileCopyrightText: 2026 Leonardo Lopes Pereira <leonardolopespereira@outlook.com>
// SPDX-FileContributor: Leonardo Lopes Pereira <leonardolopespereira@outlook.com>
// SPDX-License-Identifier: MIT

//! What can go wrong.

use core::fmt;

/// A failed access to the clock.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Error {
    /// The chip seemed to stay in an update, as it does when none is there,
    /// or the next second never came, as when the clock is halted.
    NotResponding,
    /// The registers hold no valid date, time or alarm.
    InvalidContents,
    /// The year lies outside what the [`Century`](crate::Century) in use can
    /// keep.
    YearOutOfRange,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::NotResponding => "the clock did not respond",
            Self::InvalidContents => "the clock holds no valid value",
            Self::YearOutOfRange => "year outside the century's range",
        })
    }
}

impl core::error::Error for Error {}

#[cfg(test)]
mod tests {
    use super::Error;

    #[test]
    fn display() {
        assert_eq!(
            Error::NotResponding.to_string(),
            "the clock did not respond"
        );
        assert_eq!(
            Error::InvalidContents.to_string(),
            "the clock holds no valid value"
        );
        assert_eq!(
            Error::YearOutOfRange.to_string(),
            "year outside the century's range"
        );
    }
}
