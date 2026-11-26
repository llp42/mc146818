// SPDX-FileCopyrightText: 2026 Leonardo Lopes Pereira <leonardolopespereira@outlook.com>
// SPDX-FileContributor: Leonardo Lopes Pereira <leonardolopespereira@outlook.com>
// SPDX-License-Identifier: MIT

//! Dates, and the century the chip keeps them in.

use crate::Error;
use crate::mode::Mode;
use crate::register::Nvram;

/// A Gregorian date and time from year 0 to 9999. The chip keeps no time zone.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DateTime {
    year: u16,
    month: u8,
    day: u8,
    hour: u8,
    minute: u8,
    second: u8,
}

impl DateTime {
    /// `None` unless the fields name a real date and time.
    #[must_use]
    pub const fn new(
        year: u16,
        month: u8,
        day: u8,
        hour: u8,
        minute: u8,
        second: u8,
    ) -> Option<Self> {
        let valid = year <= 9999
            && matches!(month, 1..=12)
            && day != 0
            && day <= days_in_month(year, month)
            && hour < 24
            && minute < 60
            && second < 60;
        if valid {
            Some(Self {
                year,
                month,
                day,
                hour,
                minute,
                second,
            })
        } else {
            None
        }
    }

    /// Year, 0–9999.
    #[must_use]
    pub const fn year(self) -> u16 {
        self.year
    }

    /// Month, 1–12.
    #[must_use]
    pub const fn month(self) -> u8 {
        self.month
    }

    /// Day of the month, from 1.
    #[must_use]
    pub const fn day(self) -> u8 {
        self.day
    }

    /// Hour, 0–23.
    #[must_use]
    pub const fn hour(self) -> u8 {
        self.hour
    }

    /// Minute, 0–59.
    #[must_use]
    pub const fn minute(self) -> u8 {
        self.minute
    }

    /// Second, 0–59.
    #[must_use]
    pub const fn second(self) -> u8 {
        self.second
    }

    /// Day of the week. The chip only steps its day-of-week register along
    /// with the date, so [`Rtc::set_date_time`](crate::Rtc::set_date_time)
    /// writes this one.
    #[must_use]
    pub const fn weekday(self) -> Weekday {
        // Sakamoto's method: the offset each month adds to the day count.
        const OFFSETS: [u32; 12] = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
        // January and February count towards the year before, which has the
        // leap day they come ahead of. Four centuries are a whole number of
        // weeks, so adding them leaves the result alone and keeps year 0 from
        // underflowing.
        let year = self.year as u32 + 400 - (self.month < 3) as u32;
        let days = year + year / 4 - year / 100
            + year / 400
            + OFFSETS[self.month as usize - 1]
            + self.day as u32;
        match days % 7 {
            0 => Weekday::Sunday,
            1 => Weekday::Monday,
            2 => Weekday::Tuesday,
            3 => Weekday::Wednesday,
            4 => Weekday::Thursday,
            5 => Weekday::Friday,
            _ => Weekday::Saturday,
        }
    }
}

/// A day of the week.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Weekday {
    /// Sunday.
    Sunday,
    /// Monday.
    Monday,
    /// Tuesday.
    Tuesday,
    /// Wednesday.
    Wednesday,
    /// Thursday.
    Thursday,
    /// Friday.
    Friday,
    /// Saturday.
    Saturday,
}

impl Weekday {
    /// What the day-of-week register holds: 1 for Sunday to 7 for Saturday,
    /// as the datasheet numbers them.
    pub(crate) const fn register(self) -> u8 {
        self as u8 + 1
    }
}

const fn days_in_month(year: u16, month: u8) -> u8 {
    match month {
        2 if is_leap(year) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

const fn is_leap(year: u16) -> bool {
    year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400))
}

/// Where the century of the chip's two-digit year comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Century(Source);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Source {
    Register(u8),
    Window(u16),
}

impl Century {
    /// Kept in battery-backed register `index`, as the ACPI FADT `CENTURY`
    /// field gives it. `None` unless `index` is such a register; the field's
    /// 0 means there is none.
    ///
    /// The register keeps years from 1900 on. As in Linux, a century below 19
    /// is taken for a register lost or never kept, so reading one fails with
    /// [`Error::InvalidContents`], and setting a year before 1900 with
    /// [`Error::YearOutOfRange`].
    #[must_use]
    pub const fn register(index: u8) -> Option<Self> {
        if matches!(index, Nvram::START..Nvram::END) {
            Some(Self(Source::Register(index)))
        } else {
            None
        }
    }

    /// Not kept: two-digit years fall in `first..first + 100`.
    ///
    /// # Panics
    ///
    /// If `first` is above 9900.
    #[must_use]
    pub const fn window(first: u16) -> Self {
        assert!(first <= 9900, "the window must end by year 9999");
        Self(Source::Window(first))
    }

    pub(crate) const fn index(self) -> Option<u8> {
        match self.0 {
            Source::Register(index) => Some(index),
            Source::Window(_) => None,
        }
    }
}

/// The lowest century a [`Century::register`] holds when kept.
const FIRST_CENTURY: u8 = 19;

/// The clock registers, encoded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Raw {
    pub second: u8,
    pub minute: u8,
    pub hour: u8,
    pub day: u8,
    pub month: u8,
    pub year: u8,
    /// Zero unless the century is kept in a register.
    pub century: u8,
}

impl Raw {
    pub const fn decode(self, mode: Mode, century: Century) -> Option<DateTime> {
        let (Some(second), Some(minute), Some(hour), Some(day), Some(month), Some(two_digits)) = (
            mode.decode(self.second),
            mode.decode(self.minute),
            mode.decode_hour(self.hour),
            mode.decode(self.day),
            mode.decode(self.month),
            mode.decode(self.year),
        ) else {
            return None;
        };
        let two_digits = two_digits as u16;
        let year = match century.0 {
            Source::Register(_) => match mode.decode(self.century) {
                Some(century) if century >= FIRST_CENTURY => century as u16 * 100 + two_digits,
                _ => return None,
            },
            Source::Window(first) => first + (two_digits + 100 - first % 100) % 100,
        };
        DateTime::new(year, month, day, hour, minute, second)
    }

    pub const fn encode(date_time: DateTime, mode: Mode, century: Century) -> Result<Self, Error> {
        let year = date_time.year;
        let century = match century.0 {
            Source::Register(_) if year >= FIRST_CENTURY as u16 * 100 => {
                mode.encode(two_digits(year / 100))
            }
            Source::Window(first) if first <= year && year < first + 100 => 0,
            Source::Register(_) | Source::Window(_) => return Err(Error::YearOutOfRange),
        };
        Ok(Self {
            second: mode.encode(date_time.second),
            minute: mode.encode(date_time.minute),
            hour: mode.encode_hour(date_time.hour),
            day: mode.encode(date_time.day),
            month: mode.encode(date_time.month),
            year: mode.encode(two_digits(year)),
            century,
        })
    }
}

const fn two_digits(value: u16) -> u8 {
    (value % 100) as u8
}

#[cfg(test)]
mod tests {
    use super::{Century, DateTime, Raw, Weekday, days_in_month};
    use crate::Error;
    use crate::mode::Mode;
    use crate::register::StatusB;

    #[test]
    fn weekday() {
        let weekday =
            |year, month, day| DateTime::new(year, month, day, 0, 0, 0).unwrap().weekday();
        assert_eq!(weekday(2026, 11, 26), Weekday::Thursday);
        assert_eq!(weekday(1970, 1, 1), Weekday::Thursday);
        assert_eq!(weekday(2000, 1, 1), Weekday::Saturday);
        assert_eq!(weekday(2000, 3, 1), Weekday::Wednesday);
        assert_eq!(weekday(1900, 1, 1), Weekday::Monday);
        assert_eq!(weekday(2024, 2, 29), Weekday::Thursday);
        assert_eq!(weekday(0, 1, 1), Weekday::Saturday);
        assert_eq!(weekday(9999, 12, 31), Weekday::Friday);
    }

    #[test]
    fn weekday_every_day() {
        // Day by day from 2024-01-01, through a leap year and a common one.
        let mut expected = Weekday::Monday as u8;
        for year in [2024, 2025] {
            for month in 1..=12 {
                for day in 1..=days_in_month(year, month) {
                    let date_time = DateTime::new(year, month, day, 0, 0, 0).unwrap();
                    assert_eq!(date_time.weekday() as u8, expected, "{year}-{month}-{day}");
                    expected = (expected + 1) % 7;
                }
            }
        }
    }

    #[test]
    fn weekday_register() {
        assert_eq!(Weekday::Sunday.register(), 1);
        assert_eq!(Weekday::Saturday.register(), 7);
    }

    #[test]
    fn accessors() {
        let date_time = DateTime::new(2026, 11, 26, 13, 5, 9).unwrap();
        assert_eq!(
            (
                date_time.year(),
                date_time.month(),
                date_time.day(),
                date_time.hour(),
                date_time.minute(),
                date_time.second(),
            ),
            (2026, 11, 26, 13, 5, 9)
        );
    }

    #[test]
    fn calendar() {
        assert!(DateTime::new(2024, 2, 29, 0, 0, 0).is_some());
        assert!(DateTime::new(2023, 2, 29, 0, 0, 0).is_none());
        assert!(DateTime::new(2000, 2, 29, 0, 0, 0).is_some());
        assert!(DateTime::new(1900, 2, 29, 0, 0, 0).is_none());
        assert!(DateTime::new(2026, 4, 31, 0, 0, 0).is_none());
        assert!(DateTime::new(2026, 13, 1, 0, 0, 0).is_none());
        assert!(DateTime::new(2026, 1, 0, 0, 0, 0).is_none());
        assert!(DateTime::new(2026, 1, 1, 24, 0, 0).is_none());
        assert!(DateTime::new(10_000, 1, 1, 0, 0, 0).is_none());
        assert!(DateTime::new(2026, 0, 1, 0, 0, 0).is_none());
        assert!(DateTime::new(2026, 1, 1, 0, 60, 0).is_none());
        assert!(DateTime::new(2026, 1, 1, 0, 0, 60).is_none());
        for month in [6, 9, 11] {
            assert!(DateTime::new(2026, month, 31, 0, 0, 0).is_none());
        }
    }

    #[test]
    fn century_register() {
        assert_eq!(Century::register(0), None);
        assert_eq!(Century::register(0x0d), None);
        assert_eq!(Century::register(0x80), None);
        assert_eq!(Century::register(0x32).and_then(Century::index), Some(0x32));
    }

    #[test]
    fn decode_bcd_12_hour() {
        let raw = Raw {
            second: 0x58,
            minute: 0x59,
            hour: 0x91,
            day: 0x26,
            month: 0x11,
            year: 0x26,
            century: 0x20,
        };
        let date_time = raw.decode(Mode::new(0), Century::register(0x32).unwrap());
        assert_eq!(date_time, DateTime::new(2026, 11, 26, 23, 59, 58));
    }

    #[test]
    fn decode_invalid_century() {
        let raw = Raw {
            second: 0,
            minute: 0,
            hour: 0x12,
            day: 1,
            month: 1,
            year: 0x26,
            century: 0xff,
        };
        assert_eq!(
            raw.decode(Mode::new(0), Century::register(0x32).unwrap()),
            None
        );
    }

    #[test]
    fn decode_lost_century() {
        // A century below 19, 0 above all, is taken for a lost register.
        let in_register = Century::register(0x32).unwrap();
        for (status_b, lost, first) in [
            (0, [0x00, 0x01, 0x18], 0x19),
            (StatusB::BINARY, [0, 1, 18], 19),
        ] {
            let mode = Mode::new(status_b);
            let raw = |century| Raw {
                second: 0,
                minute: 0,
                hour: 1,
                day: 1,
                month: 1,
                year: 0,
                century,
            };
            for century in lost {
                assert_eq!(raw(century).decode(mode, in_register), None);
            }
            assert_eq!(
                raw(first).decode(mode, in_register).map(DateTime::year),
                Some(1900)
            );
        }
    }

    #[test]
    fn century_register_range() {
        // Years before 1900 would read back as a lost century, so they cannot
        // be set; every later one reads back.
        let in_register = Century::register(0x32).unwrap();
        for mode in [Mode::new(0), Mode::new(StatusB::BINARY)] {
            for year in 0..=9999 {
                let date_time = DateTime::new(year, 1, 1, 0, 0, 0).unwrap();
                let raw = Raw::encode(date_time, mode, in_register);
                if year < 1900 {
                    assert_eq!(raw, Err(Error::YearOutOfRange), "{year}");
                } else {
                    let raw = raw.unwrap();
                    assert_eq!(raw.decode(mode, in_register), Some(date_time), "{year}");
                }
            }
        }
    }

    #[test]
    fn window() {
        let mode = Mode::new(StatusB::BINARY | StatusB::HOURS_24);
        let raw = |year| Raw {
            second: 0,
            minute: 0,
            hour: 0,
            day: 1,
            month: 1,
            year,
            century: 0,
        };
        let since_1980 = Century::window(1980);
        assert_eq!(
            raw(80).decode(mode, since_1980).map(DateTime::year),
            Some(1980)
        );
        assert_eq!(
            raw(79).decode(mode, since_1980).map(DateTime::year),
            Some(2079)
        );
        let encode = |year| {
            let date_time = DateTime::new(year, 1, 1, 0, 0, 0).unwrap();
            Raw::encode(date_time, mode, since_1980).map(|raw| raw.year)
        };
        assert_eq!(encode(1979), Err(Error::YearOutOfRange));
        assert_eq!(encode(1980), Ok(80));
        assert_eq!(encode(2080), Err(Error::YearOutOfRange));
    }

    #[test]
    #[should_panic(expected = "window must end")]
    fn window_too_late() {
        let _ = Century::window(9901);
    }

    #[test]
    fn round_trip() {
        let date_time = DateTime::new(1999, 12, 31, 12, 30, 7).unwrap();
        let in_register = Century::register(0x32).unwrap();
        for status_b in [
            0,
            StatusB::BINARY,
            StatusB::HOURS_24,
            StatusB::BINARY | StatusB::HOURS_24,
        ] {
            let mode = Mode::new(status_b);
            for century in [in_register, Century::window(1950)] {
                let raw = Raw::encode(date_time, mode, century).unwrap();
                assert_eq!(raw.decode(mode, century), Some(date_time));
            }
        }
    }
}
