// SPDX-FileCopyrightText: 2026 Leonardo Lopes Pereira <leonardolopespereira@outlook.com>
// SPDX-FileContributor: Leonardo Lopes Pereira <leonardolopespereira@outlook.com>
// SPDX-License-Identifier: MIT

//! The chip's interrupt sources.

use core::ops::{BitAnd, BitAndAssign, BitOr, BitOrAssign, Sub, SubAssign};

/// A set of interrupt sources.
///
/// Each bit is both the source's enable in status B and its flag in status C.
/// A set holds no other bit, so it has no `!`: `Interrupts::ALL - set` is its
/// complement.
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

    /// Whether `self` and `other` have a source in common.
    #[must_use]
    pub const fn intersects(self, other: Self) -> bool {
        self.0 & other.0 != 0
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

    /// The sources in both `self` and `other`.
    #[must_use]
    pub const fn intersection(self, other: Self) -> Self {
        Self(self.0 & other.0)
    }

    /// The sources in `self` but not in `other`.
    #[must_use]
    pub const fn difference(self, other: Self) -> Self {
        Self(self.0 & !other.0)
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

impl BitAnd for Interrupts {
    type Output = Self;

    fn bitand(self, other: Self) -> Self {
        self.intersection(other)
    }
}

impl BitAndAssign for Interrupts {
    fn bitand_assign(&mut self, other: Self) {
        *self = self.intersection(other);
    }
}

impl Sub for Interrupts {
    type Output = Self;

    fn sub(self, other: Self) -> Self {
        self.difference(other)
    }
}

impl SubAssign for Interrupts {
    fn sub_assign(&mut self, other: Self) {
        *self = self.difference(other);
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

    #[test]
    fn intersection_and_difference() {
        let both = Interrupts::PERIODIC | Interrupts::ALARM;
        let others = Interrupts::ALARM | Interrupts::UPDATE_ENDED;
        assert_eq!(both & others, Interrupts::ALARM);
        assert_eq!(both.intersection(others), Interrupts::ALARM);
        assert_eq!(both - others, Interrupts::PERIODIC);
        assert_eq!(others.difference(both), Interrupts::UPDATE_ENDED);
        assert_eq!(Interrupts::ALL - both, Interrupts::UPDATE_ENDED);
        assert!(both.intersects(others));
        assert!(!both.intersects(Interrupts::UPDATE_ENDED));
        assert!(!both.intersects(Interrupts::NONE));
        assert!(!Interrupts::NONE.intersects(Interrupts::NONE));
        let mut set = both;
        set &= others;
        assert_eq!(set, Interrupts::ALARM);
        set -= Interrupts::ALARM;
        assert_eq!(set, Interrupts::NONE);
    }

    #[test]
    fn within_all() {
        // Bits 0x70 of status B: PIE, AIE and UIE. No set, however made, holds
        // another bit, which enable and disable would write to status B.
        assert_eq!(Interrupts::ALL.bits(), 0x70);
        let within = |set: Interrupts| set.bits() & !0x70 == 0;
        for set in [
            Interrupts::NONE,
            Interrupts::PERIODIC,
            Interrupts::ALARM,
            Interrupts::UPDATE_ENDED,
            Interrupts::ALL,
            Interrupts::default(),
        ] {
            assert!(within(set), "{:#04x}", set.bits());
        }
        for status in 0..=u8::MAX {
            assert_eq!(Interrupts::from_status(status).bits(), status & 0x70);
        }
        let sets: Vec<_> = (0..8).map(|n| Interrupts::from_status(n << 4)).collect();
        for &a in &sets {
            for &b in &sets {
                let (mut or, mut and, mut minus) = (a, a, a);
                or |= b;
                and &= b;
                minus -= b;
                for set in [a | b, a & b, a - b, or, and, minus] {
                    assert!(within(set), "{:#04x}", set.bits());
                }
                assert_eq!((a | b).bits(), a.bits() | b.bits());
                assert_eq!((a & b).bits(), a.bits() & b.bits());
                assert_eq!((a - b).bits(), a.bits() & !b.bits());
                assert_eq!((or, and, minus), (a | b, a & b, a - b));
                assert_eq!(a.intersects(b), !(a & b).is_empty());
                assert_eq!((a & b) | (a - b), a);
            }
        }
    }
}
