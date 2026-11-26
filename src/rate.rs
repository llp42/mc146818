// SPDX-FileCopyrightText: 2026 Leonardo Lopes Pereira <leonardolopespereira@outlook.com>
// SPDX-FileContributor: Leonardo Lopes Pereira <leonardolopespereira@outlook.com>
// SPDX-License-Identifier: MIT

//! The periodic interrupt rate.

/// A rate of the periodic interrupt: a power of two from 2 Hz to 8192 Hz.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PeriodicRate(u8);

impl PeriodicRate {
    /// `None` unless `hz` is a power of two from 2 to 8192.
    #[must_use]
    pub const fn from_hz(hz: u32) -> Option<Self> {
        if hz.is_power_of_two() && hz >= 2 && hz <= 8192 {
            Some(Self(Self::select(hz)))
        } else {
            None
        }
    }

    /// Interrupts per second.
    #[must_use]
    pub const fn hz(self) -> u32 {
        1 << (16 - self.0)
    }

    /// The rate-select bits of status A.
    pub(crate) const fn bits(self) -> u8 {
        self.0
    }

    /// The rate the rate-select bits of status A give; `None` for 0, which
    /// turns the periodic flag off.
    pub(crate) const fn from_select(select: u8) -> Option<Self> {
        match select {
            // 256 Hz and 128 Hz again, as selects 8 and 9 already give.
            1 => Some(Self(8)),
            2 => Some(Self(9)),
            3..=15 => Some(Self(select)),
            // 0, or more than the four bits hold.
            _ => None,
        }
    }

    /// The 32.768 kHz time base divided down: `hz = 1 << (16 - select)`.
    #[expect(clippy::cast_possible_truncation, reason = "below 16 for 2..=8192")]
    const fn select(hz: u32) -> u8 {
        16 - hz.trailing_zeros() as u8
    }
}

#[cfg(test)]
mod tests {
    use super::PeriodicRate;

    #[test]
    fn select() {
        assert_eq!(PeriodicRate::from_hz(8192).map(PeriodicRate::bits), Some(3));
        assert_eq!(PeriodicRate::from_hz(128).map(PeriodicRate::bits), Some(9));
        assert_eq!(PeriodicRate::from_hz(2).map(PeriodicRate::bits), Some(15));
    }

    #[test]
    fn from_select() {
        assert_eq!(PeriodicRate::from_select(0), None);
        assert_eq!(PeriodicRate::from_select(1), PeriodicRate::from_hz(256));
        assert_eq!(PeriodicRate::from_select(2), PeriodicRate::from_hz(128));
        assert_eq!(PeriodicRate::from_select(3), PeriodicRate::from_hz(8192));
        assert_eq!(PeriodicRate::from_select(15), PeriodicRate::from_hz(2));
        for select in 0..=15 {
            let rate = PeriodicRate::from_select(select);
            assert_eq!(rate.is_none(), select == 0);
        }
    }

    #[test]
    fn round_trip() {
        for shift in 1..=13 {
            let hz = 1 << shift;
            assert_eq!(PeriodicRate::from_hz(hz).map(PeriodicRate::hz), Some(hz));
        }
    }

    #[test]
    fn rejects() {
        for hz in [0, 1, 3, 100, 16_384] {
            assert_eq!(PeriodicRate::from_hz(hz), None);
        }
    }
}
