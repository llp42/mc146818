// SPDX-FileCopyrightText: 2026 Leonardo Lopes Pereira <leonardolopespereira@outlook.com>
// SPDX-FileContributor: Leonardo Lopes Pereira <leonardolopespereira@outlook.com>
// SPDX-License-Identifier: MIT

//! The value encoding status B selects.

use crate::register::StatusB;

#[derive(Clone, Copy, Debug)]
pub struct Mode {
    binary: bool,
    hours_24: bool,
}

impl Mode {
    /// Hour bit in 12-hour mode.
    const PM: u8 = 0x80;

    pub const fn new(status_b: u8) -> Self {
        Self {
            binary: status_b & StatusB::BINARY != 0,
            hours_24: status_b & StatusB::HOURS_24 != 0,
        }
    }

    /// A value below 100; `None` if `raw` encodes none.
    pub const fn decode(self, raw: u8) -> Option<u8> {
        let value = if self.binary {
            raw
        } else {
            let (tens, ones) = (raw >> 4, raw & 0x0f);
            if tens > 9 || ones > 9 {
                return None;
            }
            tens * 10 + ones
        };
        if value < 100 { Some(value) } else { None }
    }

    /// `value` is below 100.
    pub const fn encode(self, value: u8) -> u8 {
        if self.binary {
            value
        } else {
            ((value / 10) << 4) | (value % 10)
        }
    }

    /// An hour of 0–23 in 24-hour mode; 12-hour values outside 1–12 are `None`.
    pub const fn decode_hour(self, raw: u8) -> Option<u8> {
        if self.hours_24 {
            return self.decode(raw);
        }
        let Some(hour) = self.decode(raw & !Self::PM) else {
            return None;
        };
        if hour == 0 || hour > 12 {
            return None;
        }
        let afternoon = if raw & Self::PM == 0 { 0 } else { 12 };
        Some(hour % 12 + afternoon)
    }

    /// `hour` is below 24.
    pub const fn encode_hour(self, hour: u8) -> u8 {
        if self.hours_24 {
            return self.encode(hour);
        }
        let pm = if hour < 12 { 0 } else { Self::PM };
        let hour = match hour % 12 {
            0 => 12,
            hour => hour,
        };
        self.encode(hour) | pm
    }
}

#[cfg(test)]
mod tests {
    use super::Mode;
    use crate::register::StatusB;

    const BCD_12: Mode = Mode::new(0);
    const BINARY_24: Mode = Mode::new(StatusB::BINARY | StatusB::HOURS_24);

    #[test]
    fn bcd() {
        assert_eq!(BCD_12.decode(0x59), Some(59));
        assert_eq!(BCD_12.decode(0x5a), None);
        assert_eq!(BCD_12.decode(0xa0), None);
        for value in 0..100 {
            assert_eq!(BCD_12.decode(BCD_12.encode(value)), Some(value));
        }
    }

    #[test]
    fn binary() {
        assert_eq!(BINARY_24.decode(59), Some(59));
        assert_eq!(BINARY_24.decode(100), None);
        assert_eq!(BINARY_24.encode(59), 59);
    }

    #[test]
    fn twelve_hour() {
        assert_eq!(BCD_12.decode_hour(0x12), Some(0));
        assert_eq!(BCD_12.decode_hour(0x01), Some(1));
        assert_eq!(BCD_12.decode_hour(0x92), Some(12));
        assert_eq!(BCD_12.decode_hour(0x81), Some(13));
        assert_eq!(BCD_12.decode_hour(0x91), Some(23));
        assert_eq!(BCD_12.decode_hour(0x00), None);
        assert_eq!(BCD_12.decode_hour(0x13), None);
        for hour in 0..24 {
            assert_eq!(BCD_12.decode_hour(BCD_12.encode_hour(hour)), Some(hour));
        }
    }

    #[test]
    fn twenty_four_hour() {
        for hour in 0..24 {
            assert_eq!(BINARY_24.encode_hour(hour), hour);
            assert_eq!(BINARY_24.decode_hour(hour), Some(hour));
        }
    }
}
