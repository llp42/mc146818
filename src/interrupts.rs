// SPDX-FileCopyrightText: 2026 Leonardo Lopes Pereira <leonardolopespereira@outlook.com>
// SPDX-FileContributor: Leonardo Lopes Pereira <leonardolopespereira@outlook.com>
// SPDX-License-Identifier: MIT

//! The chip's interrupt sources.

use core::ops::{BitOr, BitOrAssign};

/// A set of interrupt sources.
///
/// Each bit is both the source's enable in status B and its flag in status C.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Interrupts(u8);

impl Interrupts {
    /// No source.
    pub const NONE: Self = Self(0);
    /// At the [`PeriodicRate`](crate::PeriodicRate).
    pub const PERIODIC: Self = Self(0x40);
    /// When the clock matches the [`Alarm`](crate::Alarm).
    pub const ALARM: Self = Self(0x20);
    /// After each update cycle, once a second.
    pub const UPDATE_ENDED: Self = Self(0x10);
    /// Every source.
    pub const ALL: Self = Self(0x70);

    /// Whether every source in `other` is in `self`.
    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// Whether `self` has no source.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// The sources in `self` or `other`.
    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    pub(crate) const fn bits(self) -> u8 {
        self.0
    }

    /// The sources in a status B or C value.
    pub(crate) const fn from_status(status: u8) -> Self {
        Self(status & Self::ALL.0)
    }
}

impl BitOr for Interrupts {
    type Output = Self;

    fn bitor(self, other: Self) -> Self {
        self.union(other)
    }
}

impl BitOrAssign for Interrupts {
    fn bitor_assign(&mut self, other: Self) {
        *self = self.union(other);
    }
}

#[cfg(test)]
mod tests {
    use super::Interrupts;

    #[test]
    fn sets() {
        let both = Interrupts::PERIODIC | Interrupts::ALARM;
        assert!(both.contains(Interrupts::PERIODIC));
        assert!(!both.contains(Interrupts::UPDATE_ENDED));
        assert!(!Interrupts::PERIODIC.contains(both));
        assert!(Interrupts::ALL.contains(both));
        assert!(Interrupts::NONE.is_empty());
        assert!(!Interrupts::ALARM.is_empty());
        assert_eq!(Interrupts::from_status(0xff), Interrupts::ALL);
        let mut set = Interrupts::PERIODIC;
        set |= Interrupts::ALARM;
        assert_eq!(set, both);
    }
}
