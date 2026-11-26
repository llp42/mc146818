// SPDX-FileCopyrightText: 2026 Leonardo Lopes Pereira <leonardolopespereira@outlook.com>
// SPDX-FileContributor: Leonardo Lopes Pereira <leonardolopespereira@outlook.com>
// SPDX-License-Identifier: MIT

//! The alarm.

use crate::mode::Mode;

/// A time of day for [`Interrupts::ALARM`](crate::Interrupts::ALARM); a
/// `None` field matches every value.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Alarm {
    hour: Option<u8>,
    minute: Option<u8>,
    second: Option<u8>,
}

impl Alarm {
    /// Matches every value: any byte with the two high bits set.
    const ANY: u8 = 0xff;
    const ANY_MASK: u8 = 0xc0;

    /// `None` unless each given field is in range.
    #[must_use]
    pub const fn new(hour: Option<u8>, minute: Option<u8>, second: Option<u8>) -> Option<Self> {
        if below(hour, 24) && below(minute, 60) && below(second, 60) {
            Some(Self {
                hour,
                minute,
                second,
            })
        } else {
            None
        }
    }

    /// Hour, 0–23.
    #[must_use]
    pub const fn hour(self) -> Option<u8> {
        self.hour
    }

    /// Minute, 0–59.
    #[must_use]
    pub const fn minute(self) -> Option<u8> {
        self.minute
    }

    /// Second, 0–59.
    #[must_use]
    pub const fn second(self) -> Option<u8> {
        self.second
    }

    /// The alarm the hour, minute and second alarm registers hold; `None` if
    /// a field is neither a time nor a match-all.
    pub(crate) fn decode(raw: [u8; 3], mode: Mode) -> Option<Self> {
        let [hour, minute, second] = raw;
        let hour = if Self::is_any(hour) {
            None
        } else {
            Some(mode.decode_hour(hour)?)
        };
        let minute = if Self::is_any(minute) {
            None
        } else {
            Some(mode.decode(minute)?)
        };
        let second = if Self::is_any(second) {
            None
        } else {
            Some(mode.decode(second)?)
        };
        Self::new(hour, minute, second)
    }

    const fn is_any(raw: u8) -> bool {
        raw & Self::ANY_MASK == Self::ANY_MASK
    }

    /// The hour, minute and second alarm registers.
    pub(crate) const fn encode(self, mode: Mode) -> [u8; 3] {
        [
            match self.hour {
                Some(hour) => mode.encode_hour(hour),
                None => Self::ANY,
            },
            match self.minute {
                Some(minute) => mode.encode(minute),
                None => Self::ANY,
            },
            match self.second {
                Some(second) => mode.encode(second),
                None => Self::ANY,
            },
        ]
    }
}

const fn below(field: Option<u8>, limit: u8) -> bool {
    match field {
        Some(value) => value < limit,
        None => true,
    }
}

#[cfg(test)]
mod tests {
    use super::Alarm;
    use crate::mode::Mode;

    #[test]
    fn range() {
        assert!(Alarm::new(Some(23), Some(59), Some(59)).is_some());
        assert!(Alarm::new(Some(24), None, None).is_none());
        assert!(Alarm::new(None, Some(60), None).is_none());
        assert!(Alarm::new(None, None, Some(60)).is_none());
    }

    #[test]
    fn encode() {
        let alarm = Alarm::new(Some(13), None, Some(5)).unwrap();
        assert_eq!(alarm.encode(Mode::new(0)), [0x81, 0xff, 0x05]);
    }

    #[test]
    fn decode() {
        let mode = Mode::new(0);
        assert_eq!(
            Alarm::decode([0x81, 0xff, 0x05], mode),
            Alarm::new(Some(13), None, Some(5))
        );
        // Any byte from 0xc0 up matches every value, even in 12-hour mode.
        assert_eq!(
            Alarm::decode([0xc0, 0xc5, 0xff], mode),
            Alarm::new(None, None, None)
        );
        // Not BCD, an hour outside 1-12, and a minute past 59.
        assert_eq!(Alarm::decode([0x1a, 0xff, 0xff], mode), None);
        assert_eq!(Alarm::decode([0x00, 0xff, 0xff], mode), None);
        assert_eq!(Alarm::decode([0xff, 0x60, 0xff], mode), None);
    }
}
