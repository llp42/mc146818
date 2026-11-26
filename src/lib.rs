// SPDX-FileCopyrightText: 2026 Leonardo Lopes Pereira <leonardolopespereira@outlook.com>
// SPDX-FileContributor: Leonardo Lopes Pereira <leonardolopespereira@outlook.com>
// SPDX-License-Identifier: MIT

//! Driver for the MC146818 real-time clock and its battery-backed memory, the
//! CMOS clock of PCs.
//!
//! [`Rtc`] reaches the chip through a [`Bus`]. Each register access is an
//! index write followed by a data access, so two of them must not interleave:
//! code that shares the chip with its interrupt handler keeps the [`Rtc`]
//! behind a lock that masks that interrupt.
//!
//! ```no_run
//! use mc146818::{Bus, Interrupts, PeriodicRate, Rtc};
//!
//! /// Index port 0x70, data port 0x71.
//! struct Cmos;
//!
//! impl Bus for Cmos {
//!     fn read(&mut self, register: u8) -> u8 {
//!         // outb(0x70, register); inb(0x71)
//! #       let _ = register;
//! #       0
//!     }
//!
//!     fn write(&mut self, register: u8, value: u8) {
//!         // outb(0x70, register); outb(0x71, value)
//! #       let _ = (register, value);
//!     }
//! }
//!
//! let mut rtc = Rtc::new(Cmos);
//! rtc.set_periodic_rate(Some(PeriodicRate::from_hz(128).unwrap()));
//! rtc.enable(Interrupts::PERIODIC);
//!
//! // In the interrupt handler:
//! let pending = rtc.acknowledge();
//! ```
//!
//! # Left to the firmware
//!
//! The firmware normally sets up the following before the kernel runs. The
//! driver leaves each as it finds it and offers no way to change it:
//!
//! - The oscillator and divider chain (`DV2` to `DV0` of status A), so the
//!   driver neither starts the oscillator nor selects the time base.
//! - The square-wave output and daylight-saving enables (`SQWE` and `DSE` of
//!   status B), which PCs do not use.
//! - The data mode, binary or BCD and 12- or 24-hour. The driver reads it from
//!   status B on each access and follows it.
//!
//! Features that go beyond the MC146818, such as the date and month alarms
//! ACPI describes or the extended memory some chipsets add, are not covered
//! either.

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]

mod alarm;
mod date_time;
#[cfg(test)]
mod fake;
mod interrupts;
mod mode;
mod rate;
mod register;

use core::hint::spin_loop;

pub use alarm::Alarm;
pub use date_time::{Century, DateTime, Weekday, YearOutOfWindow};
pub use interrupts::Interrupts;
pub use rate::PeriodicRate;

use date_time::Raw;
use mode::Mode;
use register::{Nvram, Register, StatusA, StatusB, StatusD};

/// Access to the chip's registers, `0x00` to `0x7f`.
///
/// On PCs the chip sits behind I/O ports `0x70` (index) and `0x71` (data), and
/// bit 7 of the index masks NMI; the implementation chooses that bit.
pub trait Bus {
    /// Reads `register`.
    fn read(&mut self, register: u8) -> u8;

    /// Writes `value` to `register`.
    fn write(&mut self, register: u8, value: u8);
}

impl<B: Bus + ?Sized> Bus for &mut B {
    fn read(&mut self, register: u8) -> u8 {
        (**self).read(register)
    }

    fn write(&mut self, register: u8, value: u8) {
        (**self).write(register, value);
    }
}

/// The clock, reached through `B`.
#[derive(Debug)]
pub struct Rtc<B> {
    bus: B,
}

impl<B: Bus> Rtc<B> {
    /// Battery-backed bytes after the clock registers, as on PCs. The firmware
    /// owns most of them and checksums some.
    pub const NVRAM_LEN: u8 = Nvram::END - Nvram::START;

    /// The clock behind `bus`.
    pub const fn new(bus: B) -> Self {
        Self { bus }
    }

    /// Gives the bus back.
    pub fn into_bus(self) -> B {
        self.bus
    }

    /// Whether the clock and memory kept their contents; false once the
    /// battery runs down.
    pub fn is_valid(&mut self) -> bool {
        self.read(Register::StatusD) & StatusD::VALID != 0
    }

    /// Returns once the seconds register changes. Never returns while the
    /// clock is halted.
    pub fn wait_for_next_second(&mut self) {
        let second = self.seconds();
        while self.seconds() == second {
            spin_loop();
        }
    }

    /// Sets the rate of [`Interrupts::PERIODIC`]. `None` turns the periodic
    /// flag off, which disabling the interrupt does not: the flag is set
    /// whether or not the interrupt is enabled.
    pub fn set_periodic_rate(&mut self, rate: Option<PeriodicRate>) {
        let a = self.read(Register::StatusA) & !StatusA::RATE_SELECT;
        self.write(Register::StatusA, a | rate.map_or(0, PeriodicRate::bits));
    }

    /// The rate of [`Interrupts::PERIODIC`]; `None` if it is off.
    pub fn periodic_rate(&mut self) -> Option<PeriodicRate> {
        PeriodicRate::from_select(self.read(Register::StatusA) & StatusA::RATE_SELECT)
    }

    /// Lets `interrupts` raise the interrupt line.
    pub fn enable(&mut self, interrupts: Interrupts) {
        let b = self.read(Register::StatusB);
        self.write(Register::StatusB, b | interrupts.bits());
    }

    /// Stops `interrupts` from raising the interrupt line.
    pub fn disable(&mut self, interrupts: Interrupts) {
        let b = self.read(Register::StatusB);
        self.write(Register::StatusB, b & !interrupts.bits());
    }

    /// The enabled interrupts.
    pub fn enabled(&mut self) -> Interrupts {
        Interrupts::from_status(self.read(Register::StatusB))
    }

    /// Clears and returns the pending flags, which are set whether or not
    /// their interrupt is enabled. The chip raises no further interrupt until
    /// this is called.
    pub fn acknowledge(&mut self) -> Interrupts {
        Interrupts::from_status(self.read(Register::StatusC))
    }

    /// The current date and time; `None` if the chip holds none.
    pub fn date_time(&mut self, century: Century) -> Option<DateTime> {
        let mut raw = self.read_raw(century);
        loop {
            let again = self.read_raw(century);
            if again == raw {
                break;
            }
            raw = again;
        }
        raw.decode(self.mode(), century)
    }

    /// Sets the date, the time and the day of the week, and keeps the clock
    /// running.
    ///
    /// # Errors
    ///
    /// [`YearOutOfWindow`] if `century` is a window that excludes the year; the
    /// chip is left untouched.
    pub fn set_date_time(
        &mut self,
        date_time: DateTime,
        century: Century,
    ) -> Result<(), YearOutOfWindow> {
        let b = self.read(Register::StatusB);
        let mode = Mode::new(b);
        let raw = Raw::encode(date_time, mode, century)?;
        let weekday = mode.encode(date_time.weekday().register());
        self.write(Register::StatusB, b | StatusB::SET);
        self.write(Register::Seconds, raw.second);
        self.write(Register::Minutes, raw.minute);
        self.write(Register::Hours, raw.hour);
        self.write(Register::DayOfWeek, weekday);
        self.write(Register::DayOfMonth, raw.day);
        self.write(Register::Month, raw.month);
        self.write(Register::Year, raw.year);
        if let Some(index) = century.index() {
            self.bus.write(index, raw.century);
        }
        self.write(Register::StatusB, b & !StatusB::SET);
        Ok(())
    }

    /// Sets the time [`Interrupts::ALARM`] fires at.
    pub fn set_alarm(&mut self, alarm: Alarm) {
        let [hour, minute, second] = alarm.encode(self.mode());
        self.write(Register::HoursAlarm, hour);
        self.write(Register::MinutesAlarm, minute);
        self.write(Register::SecondsAlarm, second);
    }

    /// The time [`Interrupts::ALARM`] fires at; `None` if the alarm registers
    /// hold neither a time nor a match-all.
    pub fn alarm(&mut self) -> Option<Alarm> {
        let mode = self.mode();
        let raw = [
            self.read(Register::HoursAlarm),
            self.read(Register::MinutesAlarm),
            self.read(Register::SecondsAlarm),
        ];
        Alarm::decode(raw, mode)
    }

    /// The battery-backed byte at `offset`.
    ///
    /// # Panics
    ///
    /// If `offset` is not below [`Self::NVRAM_LEN`].
    pub fn read_nvram(&mut self, offset: u8) -> u8 {
        let register = Self::nvram_register(offset);
        self.bus.read(register)
    }

    /// Writes the battery-backed byte at `offset`.
    ///
    /// # Panics
    ///
    /// If `offset` is not below [`Self::NVRAM_LEN`].
    pub fn write_nvram(&mut self, offset: u8, value: u8) {
        let register = Self::nvram_register(offset);
        self.bus.write(register, value);
    }

    fn nvram_register(offset: u8) -> u8 {
        assert!(
            offset < Self::NVRAM_LEN,
            "NVRAM offset {offset} out of range"
        );
        Nvram::START + offset
    }

    fn read(&mut self, register: Register) -> u8 {
        self.bus.read(register as u8)
    }

    fn write(&mut self, register: Register, value: u8) {
        self.bus.write(register as u8, value);
    }

    fn mode(&mut self) -> Mode {
        Mode::new(self.read(Register::StatusB))
    }

    fn wait_while_updating(&mut self) {
        while self.read(Register::StatusA) & StatusA::UPDATE_IN_PROGRESS != 0 {
            spin_loop();
        }
    }

    fn seconds(&mut self) -> u8 {
        self.wait_while_updating();
        self.read(Register::Seconds)
    }

    fn read_raw(&mut self, century: Century) -> Raw {
        self.wait_while_updating();
        Raw {
            second: self.read(Register::Seconds),
            minute: self.read(Register::Minutes),
            hour: self.read(Register::Hours),
            day: self.read(Register::DayOfMonth),
            month: self.read(Register::Month),
            year: self.read(Register::Year),
            century: century.index().map_or(0, |index| self.bus.read(index)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Alarm, Century, DateTime, Interrupts, PeriodicRate, Rtc, YearOutOfWindow};
    use crate::fake::Fake;
    use crate::register::{Register, StatusB};

    const CENTURY: u8 = 0x32;

    #[test]
    fn periodic_rate_keeps_divider() {
        let mut fake = Fake::new(0);
        Rtc::new(&mut fake).set_periodic_rate(PeriodicRate::from_hz(128));
        assert_eq!(fake.get(Register::StatusA), 0x29);
    }

    #[test]
    fn periodic_rate_off() {
        let mut fake = Fake::new(0);
        let mut rtc = Rtc::new(&mut fake);
        rtc.set_periodic_rate(PeriodicRate::from_hz(8192));
        assert_eq!(rtc.periodic_rate(), PeriodicRate::from_hz(8192));
        rtc.set_periodic_rate(None);
        assert_eq!(rtc.periodic_rate(), None);
        assert_eq!(fake.get(Register::StatusA), 0x20);
    }

    #[test]
    fn interrupts() {
        let mut fake = Fake::new(StatusB::HOURS_24);
        let mut rtc = Rtc::new(&mut fake);
        rtc.enable(Interrupts::PERIODIC | Interrupts::ALARM);
        rtc.disable(Interrupts::ALARM);
        assert_eq!(rtc.enabled(), Interrupts::PERIODIC);
        assert_eq!(fake.get(Register::StatusB), StatusB::HOURS_24 | 0x40);

        fake.set(Register::StatusC, 0xc0);
        let mut rtc = Rtc::new(&mut fake);
        assert_eq!(rtc.acknowledge(), Interrupts::PERIODIC);
        assert_eq!(rtc.acknowledge(), Interrupts::NONE);
    }

    #[test]
    fn date_time_round_trip() {
        let date_time = DateTime::new(2026, 11, 26, 13, 5, 9).unwrap();
        for status_b in [0, StatusB::BINARY | StatusB::HOURS_24] {
            let mut fake = Fake::new(status_b);
            let mut rtc = Rtc::new(&mut fake);
            let century = Century::register(CENTURY).unwrap();
            rtc.set_date_time(date_time, century).unwrap();
            assert_eq!(rtc.date_time(century), Some(date_time));
            assert_eq!(fake.get(Register::StatusB), status_b);
        }
    }

    #[test]
    fn date_time_layout() {
        let mut fake = Fake::new(0);
        let date_time = DateTime::new(2026, 11, 26, 13, 5, 9).unwrap();
        Rtc::new(&mut fake)
            .set_date_time(date_time, Century::register(CENTURY).unwrap())
            .unwrap();
        assert_eq!(fake.get(Register::Seconds), 0x09);
        assert_eq!(fake.get(Register::Minutes), 0x05);
        assert_eq!(fake.get(Register::Hours), 0x81);
        // A Thursday, with Sunday as 1.
        assert_eq!(fake.get(Register::DayOfWeek), 0x05);
        assert_eq!(fake.get(Register::DayOfMonth), 0x26);
        assert_eq!(fake.get(Register::Month), 0x11);
        assert_eq!(fake.get(Register::Year), 0x26);
        assert_eq!(fake.registers[usize::from(CENTURY)], 0x20);
    }

    #[test]
    fn out_of_window_leaves_chip() {
        let mut fake = Fake::new(0);
        let before = fake.registers;
        let date_time = DateTime::new(2100, 1, 1, 0, 0, 0).unwrap();
        let result = Rtc::new(&mut fake).set_date_time(date_time, Century::window(2000));
        assert_eq!(result, Err(YearOutOfWindow));
        assert_eq!(fake.registers, before);
    }

    #[test]
    fn invalid_date_time() {
        let mut fake = Fake::new(0);
        fake.set(Register::Month, 0x13);
        assert_eq!(Rtc::new(&mut fake).date_time(Century::window(2000)), None);
    }

    #[test]
    fn alarm() {
        let mut fake = Fake::new(StatusB::BINARY | StatusB::HOURS_24);
        Rtc::new(&mut fake).set_alarm(Alarm::new(None, Some(30), Some(0)).unwrap());
        assert_eq!(fake.get(Register::HoursAlarm), 0xff);
        assert_eq!(fake.get(Register::MinutesAlarm), 30);
        assert_eq!(fake.get(Register::SecondsAlarm), 0);
    }

    #[test]
    fn alarm_round_trip() {
        let alarm = Alarm::new(Some(13), None, Some(5)).unwrap();
        for status_b in [0, StatusB::BINARY | StatusB::HOURS_24] {
            let mut fake = Fake::new(status_b);
            let mut rtc = Rtc::new(&mut fake);
            rtc.set_alarm(alarm);
            assert_eq!(rtc.alarm(), Some(alarm));
        }
    }

    #[test]
    fn alarm_invalid() {
        let mut fake = Fake::new(0);
        fake.set(Register::MinutesAlarm, 0x60);
        assert_eq!(Rtc::new(&mut fake).alarm(), None);
    }

    #[test]
    fn nvram() {
        let mut fake = Fake::new(0);
        let mut rtc = Rtc::new(&mut fake);
        rtc.write_nvram(0, 0xaa);
        rtc.write_nvram(Rtc::<Fake>::NVRAM_LEN - 1, 0xbb);
        assert_eq!(rtc.read_nvram(0), 0xaa);
        assert_eq!(fake.registers[0x0e], 0xaa);
        assert_eq!(fake.registers[0x7f], 0xbb);
    }

    #[test]
    #[should_panic(expected = "out of range")]
    fn nvram_bounds() {
        Rtc::new(Fake::new(0)).read_nvram(Rtc::<Fake>::NVRAM_LEN);
    }

    #[test]
    fn valid() {
        let mut fake = Fake::new(0);
        assert!(Rtc::new(&mut fake).is_valid());
        fake.set(Register::StatusD, 0);
        assert!(!Rtc::new(&mut fake).is_valid());
    }

    #[test]
    fn next_second() {
        let mut fake = Fake::new(0);
        fake.set(Register::Seconds, 0x41);
        fake.tick_after = Some(5);
        Rtc::new(&mut fake).wait_for_next_second();
        assert_eq!(fake.get(Register::Seconds), 0x42);
        assert_eq!(fake.tick_after, None);
    }
}
