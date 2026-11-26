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
//! use mc146818::{Bus, Century, Interrupts, PeriodicRate, Rtc};
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
//! # fn main() -> Result<(), mc146818::Error> {
//! let mut rtc = Rtc::new(Cmos);
//! let now = rtc.date_time(Century::window(2000))?;
//!
//! const RATE: PeriodicRate = PeriodicRate::from_hz(128).unwrap();
//! rtc.set_periodic_rate(RATE);
//! // Drops stale flags, which would raise the interrupt at once.
//! rtc.acknowledge();
//! rtc.enable(Interrupts::PERIODIC);
//!
//! // In the interrupt handler:
//! let pending = rtc.acknowledge();
//! # let _ = (now, pending);
//! # Ok(())
//! # }
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
mod error;
#[cfg(test)]
mod fake;
mod interrupts;
mod mode;
mod rate;
mod register;

use core::hint::spin_loop;
use core::ops::Range;

pub use alarm::Alarm;
pub use date_time::{Century, DateTime, Weekday};
pub use error::Error;
pub use interrupts::Interrupts;
pub use rate::PeriodicRate;

use date_time::Raw;
use mode::Mode;
use register::{Register, StatusA, StatusB, StatusD};

/// The waits count reads, for want of a timer, and take none to be quicker
/// than this many nanoseconds: a hundredth of the original part's 953 ns bus
/// cycle. On PCs and hypervisors a read takes a microsecond or more.
const MIN_READ_NS: u32 = 10;

/// The longest UIP stays set, in nanoseconds: from 244 µs before an update to
/// the end of the update, 1984 µs later (datasheet, Table 6).
const MAX_UPDATE_NS: u32 = 244_000 + 1_984_000;

/// Tries [`Rtc::between_updates`] makes at an access. Each reads at least
/// twice, so even at [`MIN_READ_NS`] they outlast the longest update twice
/// over. On a PC, a missing chip fails after half a second or more.
const UPDATE_TRIES: u32 = MAX_UPDATE_NS / MIN_READ_NS;

/// Tries at the next second, up to a second and an update away, counted as
/// [`UPDATE_TRIES`] are. A last resort for a clock that stops with nothing in
/// status A or B to show it, as when the oscillator fails; on a PC, giving up
/// then takes minutes.
#[cfg(not(test))]
const SECOND_TRIES: u32 = (1_000_000_000 + MAX_UPDATE_NS) / MIN_READ_NS;

/// Few enough for tests to reach, and still more than [`UPDATE_TRIES`].
#[cfg(test)]
const SECOND_TRIES: u32 = 2 * UPDATE_TRIES;

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

/// The battery-backed bytes after the clock registers, by register number:
/// `0x0e` to `0x7f`, as on PCs. The firmware owns most of them and checksums
/// some.
pub const NVRAM: Range<u8> = 0x0e..0x80;

/// The clock, reached through `B`.
#[derive(Debug)]
pub struct Rtc<B> {
    bus: B,
}

impl<B: Bus> Rtc<B> {
    /// The clock behind `bus`.
    #[must_use]
    pub const fn new(bus: B) -> Self {
        Self { bus }
    }

    /// Gives the bus back.
    #[must_use]
    pub fn into_bus(self) -> B {
        self.bus
    }

    /// Whether the clock and memory kept their contents; false once the
    /// battery runs down.
    pub fn is_valid(&mut self) -> bool {
        self.read(Register::StatusD) & StatusD::VALID != 0
    }

    /// Returns once the seconds register changes.
    ///
    /// # Errors
    ///
    /// [`Error::Halted`] if SET is set or the divider chain is held in reset.
    /// [`Error::NotResponding`] if the chip never leaves an update, or if the
    /// seconds keep still all the same, as when the oscillator has stopped;
    /// with no timer to go by, giving up on that takes minutes on a PC.
    pub fn wait_for_next_second(&mut self) -> Result<(), Error> {
        // After the first read, so that a missing chip, whose 0xff looks
        // halted too, still fails as one.
        let first = self.between_updates(UPDATE_TRIES, |_, second| Some(second))?;
        if self.is_halted() {
            return Err(Error::Halted);
        }
        self.between_updates(SECOND_TRIES, |_, second| (second != first).then_some(()))
    }

    /// Sets the rate of [`Interrupts::PERIODIC`], turning the periodic flag
    /// on if it was off.
    pub fn set_periodic_rate(&mut self, rate: PeriodicRate) {
        self.set_rate_select(rate.bits());
    }

    /// Turns the periodic flag off, which disabling [`Interrupts::PERIODIC`]
    /// does not: the flag is set whether or not the interrupt is enabled.
    pub fn stop_periodic(&mut self) {
        self.set_rate_select(0);
    }

    /// The rate of [`Interrupts::PERIODIC`]; `None` if the periodic flag is
    /// off.
    #[must_use]
    pub fn periodic_rate(&mut self) -> Option<PeriodicRate> {
        PeriodicRate::from_select(self.read(Register::StatusA) & StatusA::RATE_SELECT)
    }

    /// Lets `interrupts` raise the interrupt line. Their flags are set whether
    /// or not they are enabled, so a stale one raises it at once; call
    /// [`Self::acknowledge`] first to drop them.
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
    #[must_use]
    pub fn enabled(&mut self) -> Interrupts {
        Interrupts::from_status(self.read(Register::StatusB))
    }

    /// Clears and returns the pending flags, which are set whether or not
    /// their interrupt is enabled. The chip raises no further interrupt until
    /// this is called.
    pub fn acknowledge(&mut self) -> Interrupts {
        Interrupts::from_status(self.read(Register::StatusC))
    }

    /// The current date and time.
    ///
    /// # Errors
    ///
    /// [`Error::NotResponding`] if the chip never leaves an update, and
    /// [`Error::InvalidContents`] if it holds no date and time.
    pub fn date_time(&mut self, century: Century) -> Result<DateTime, Error> {
        // A read an update tore fails the checks after it, as in Linux.
        let raw = self.between_updates(UPDATE_TRIES, |rtc, _| Some(rtc.read_raw(century)))?;
        raw.decode(self.mode(), century)
            .ok_or(Error::InvalidContents)
    }

    /// Sets the date, the time and the day of the week, and keeps the clock
    /// running.
    ///
    /// # Errors
    ///
    /// [`Error::YearOutOfRange`] if `century` cannot keep the year; the chip
    /// is left untouched.
    pub fn set_date_time(&mut self, date_time: DateTime, century: Century) -> Result<(), Error> {
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
    ///
    /// Some chipsets drop writes to the alarm registers during an update, so
    /// they are written between updates, and again if one may have come in
    /// between.
    ///
    /// # Errors
    ///
    /// [`Error::NotResponding`] if the chip never leaves an update; the alarm
    /// may then be left unset.
    pub fn set_alarm(&mut self, alarm: Alarm) -> Result<(), Error> {
        let [hour, minute, second] = alarm.encode(self.mode());
        self.between_updates(UPDATE_TRIES, |rtc, _| {
            rtc.write(Register::HoursAlarm, hour);
            rtc.write(Register::MinutesAlarm, minute);
            rtc.write(Register::SecondsAlarm, second);
            Some(())
        })
    }

    /// The time [`Interrupts::ALARM`] fires at.
    ///
    /// # Errors
    ///
    /// [`Error::NotResponding`] if the chip never leaves an update, and
    /// [`Error::InvalidContents`] if an alarm register holds neither a time
    /// nor a match-all.
    pub fn alarm(&mut self) -> Result<Alarm, Error> {
        let mode = self.mode();
        let raw = self.between_updates(UPDATE_TRIES, |rtc, _| {
            Some([
                rtc.read(Register::HoursAlarm),
                rtc.read(Register::MinutesAlarm),
                rtc.read(Register::SecondsAlarm),
            ])
        })?;
        Alarm::decode(raw, mode).ok_or(Error::InvalidContents)
    }

    /// The battery-backed byte in `register`, numbered as for [`Bus`] and
    /// [`Century::register`].
    ///
    /// # Panics
    ///
    /// If `register` is outside [`NVRAM`].
    #[must_use]
    pub fn read_cmos(&mut self, register: u8) -> u8 {
        check_nvram(register);
        self.bus.read(register)
    }

    /// Writes the battery-backed byte in `register`, numbered as for
    /// [`Self::read_cmos`].
    ///
    /// It can overwrite bytes the firmware owns, such as the century
    /// register, the ACPI alarm registers and the checksummed bytes; a
    /// checksummed byte changed without its checksum may make the firmware
    /// reset its settings at the next boot.
    ///
    /// # Panics
    ///
    /// If `register` is outside [`NVRAM`].
    pub fn write_cmos(&mut self, register: u8, value: u8) {
        check_nvram(register);
        self.bus.write(register, value);
    }

    /// Fills `buf` from consecutive battery-backed registers, `start` first,
    /// numbered as for [`Self::read_cmos`].
    ///
    /// # Panics
    ///
    /// If the registers `start..start + buf.len()` do not lie in [`NVRAM`],
    /// before any is read. They are bounded as a subslice is: an empty `buf`
    /// reads nothing, and `start` may then be `NVRAM.end` too.
    pub fn read_cmos_into(&mut self, start: u8, buf: &mut [u8]) {
        check_nvram_range(start, buf.len());
        for (byte, register) in buf.iter_mut().zip(start..) {
            *byte = self.bus.read(register);
        }
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

    /// Writes the rate-select bits of status A, keeping the divider's.
    fn set_rate_select(&mut self, select: u8) {
        let a = self.read(Register::StatusA) & !StatusA::RATE_SELECT;
        self.write(Register::StatusA, a | select);
    }

    /// Runs `access` between updates, as Linux's `mc146818_avoid_UIP` does:
    /// reads the seconds, checks that no update is in progress and that the
    /// seconds held, then runs `access` with them and checks both again.
    /// Reading the seconds first keeps a stall right after a check, such as
    /// an SMI, from hiding an update. Tries again, up to `tries` times, while
    /// a check fails or `access` returns `None`.
    fn between_updates<T>(
        &mut self,
        tries: u32,
        mut access: impl FnMut(&mut Self, u8) -> Option<T>,
    ) -> Result<T, Error> {
        for _ in 0..tries {
            let second = self.read(Register::Seconds);
            if self.is_steady(second) {
                if let Some(value) = access(self, second) {
                    if self.is_steady(second) {
                        return Ok(value);
                    }
                }
            }
            spin_loop();
        }
        Err(Error::NotResponding)
    }

    /// Whether SET or the divider chain's reset keeps the clock from running.
    fn is_halted(&mut self) -> bool {
        self.read(Register::StatusB) & StatusB::SET != 0
            || self.read(Register::StatusA) & StatusA::DIVIDER_RESET == StatusA::DIVIDER_RESET
    }

    /// Whether no update is in progress and the seconds still read `second`.
    fn is_steady(&mut self, second: u8) -> bool {
        self.read(Register::StatusA) & StatusA::UPDATE_IN_PROGRESS == 0
            && self.read(Register::Seconds) == second
    }

    fn read_raw(&mut self, century: Century) -> Raw {
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

fn check_nvram(register: u8) {
    assert!(
        NVRAM.contains(&register),
        "register {register:#04x} is outside NVRAM"
    );
}

fn check_nvram_range(start: u8, len: usize) {
    // A slice holds at most `isize::MAX` bytes, so this cannot overflow.
    let end = usize::from(start) + len;
    assert!(
        NVRAM.start <= start && end <= usize::from(NVRAM.end),
        "registers {start:#04x}..{end:#04x} run outside NVRAM"
    );
}

#[cfg(test)]
mod tests {
    use std::panic::{AssertUnwindSafe, catch_unwind};

    use super::{
        Alarm, Century, DateTime, Error, Interrupts, NVRAM, PeriodicRate, Rtc, SECOND_TRIES,
        UPDATE_TRIES,
    };
    use crate::fake::{Fake, Missing};
    use crate::register::{Register, StatusB};

    const CENTURY: u8 = 0x32;

    #[test]
    fn periodic_rate_keeps_divider() {
        let mut fake = Fake::new(0);
        Rtc::new(&mut fake).set_periodic_rate(PeriodicRate::from_hz(128).unwrap());
        assert_eq!(fake.get(Register::StatusA), 0x29);
    }

    #[test]
    fn stop_periodic() {
        // Rate select 0 turns the flag off; the divider bits stay.
        let mut fake = Fake::new(0);
        let mut rtc = Rtc::new(&mut fake);
        rtc.set_periodic_rate(PeriodicRate::from_hz(8192).unwrap());
        assert_eq!(rtc.periodic_rate(), PeriodicRate::from_hz(8192));
        rtc.stop_periodic();
        assert_eq!(rtc.periodic_rate(), None);
        assert_eq!(fake.get(Register::StatusA), 0x20);
        // And the next rate turns it back on.
        let mut rtc = Rtc::new(&mut fake);
        rtc.set_periodic_rate(PeriodicRate::from_hz(2).unwrap());
        assert_eq!(rtc.periodic_rate(), PeriodicRate::from_hz(2));
        assert_eq!(fake.get(Register::StatusA), 0x2f);
    }

    #[test]
    fn interrupts() {
        let mut fake = Fake::new(StatusB::HOURS_24);
        let mut rtc = Rtc::new(&mut fake);
        rtc.enable(Interrupts::PERIODIC | Interrupts::ALARM);
        rtc.disable(Interrupts::ALARM);
        rtc.disable(Interrupts::UPDATE_ENDED);
        assert_eq!(rtc.enabled(), Interrupts::PERIODIC);
        assert_eq!(fake.get(Register::StatusB), StatusB::HOURS_24 | 0x40);

        fake.set(Register::StatusC, 0xc0);
        let mut rtc = Rtc::new(&mut fake);
        assert_eq!(rtc.acknowledge(), Interrupts::PERIODIC);
        assert_eq!(rtc.acknowledge(), Interrupts::NONE);
    }

    #[test]
    fn interrupts_keep_status_b() {
        // Only bits 0x70 of status B change, whatever the set: SET (0x80) and
        // SQWE, DM, 24/12 and DSE (0x0f) stay as they are.
        for n in 0..8 {
            let bits = n << 4;
            let set = Interrupts::from_status(bits);
            for (set, bits) in [(set, bits), (Interrupts::ALL - set, 0x70 & !bits)] {
                let mut fake = Fake::new(0x8f);
                let mut rtc = Rtc::new(&mut fake);
                rtc.enable(set);
                assert_eq!(rtc.enabled(), set);
                rtc.disable(set);
                assert_eq!(rtc.enabled(), Interrupts::NONE);
                assert_eq!(fake.writes, [(0x0b, 0x8f | bits), (0x0b, 0x8f)]);
            }
        }
    }

    #[test]
    fn date_time_round_trip() {
        let date_time = DateTime::new(2026, 11, 26, 13, 5, 9).unwrap();
        for status_b in [0, StatusB::BINARY | StatusB::HOURS_24] {
            for century in [Century::register(CENTURY).unwrap(), Century::window(2000)] {
                let mut fake = Fake::new(status_b);
                let mut rtc = Rtc::new(&mut fake);
                rtc.set_date_time(date_time, century).unwrap();
                assert_eq!(rtc.date_time(century), Ok(date_time));
                assert_eq!(fake.get(Register::StatusB), status_b);
            }
        }
    }

    #[test]
    fn date_time_layout() {
        // Register numbers and status B bits from the datasheet; PC firmware
        // sets 0x02, BCD with 24-hour hours. Registers 0x00 to 0x09 hold the
        // seconds, minutes and hours, each followed by its alarm, then the
        // day of the week (Thursday is 5, Sunday being 1), day, month and year.
        let date_time = DateTime::new(2026, 11, 26, 13, 5, 9).unwrap();
        for (status_b, clock, century) in [
            (0x00, [9, 0, 5, 0, 0x81, 0, 5, 0x26, 0x11, 0x26], 0x20),
            (0x02, [9, 0, 5, 0, 0x13, 0, 5, 0x26, 0x11, 0x26], 0x20),
            (0x04, [9, 0, 5, 0, 0x81, 0, 5, 26, 11, 26], 20),
        ] {
            let mut fake = Fake::new(status_b);
            Rtc::new(&mut fake)
                .set_date_time(date_time, Century::register(CENTURY).unwrap())
                .unwrap();
            assert_eq!(fake.registers[..10], clock);
            assert_eq!(fake.registers[usize::from(CENTURY)], century);
        }
    }

    #[test]
    fn date_time_waits_out_update() {
        let mut fake = Fake::new(StatusB::HOURS_24);
        let date_time = DateTime::new(2026, 11, 26, 13, 5, 9).unwrap();
        let century = Century::register(CENTURY).unwrap();
        Rtc::new(&mut fake)
            .set_date_time(date_time, century)
            .unwrap();
        fake.updating = 3;
        assert_eq!(Rtc::new(&mut fake).date_time(century), Ok(date_time));
    }

    #[test]
    fn set_date_time_ends_update() {
        // The clock registers drop writes during an update, which setting SET
        // ends, so it has to come first.
        let mut fake = Fake::new(StatusB::HOURS_24);
        fake.updating = 3;
        let date_time = DateTime::new(2026, 11, 26, 13, 5, 9).unwrap();
        let century = Century::register(CENTURY).unwrap();
        let mut rtc = Rtc::new(&mut fake);
        rtc.set_date_time(date_time, century).unwrap();
        assert_eq!(rtc.date_time(century), Ok(date_time));
    }

    #[test]
    fn date_time_rereads_torn_time() {
        // The clock goes from 12:34:59 to 12:35:00 right after the seconds are
        // read, so the minutes read next are already the new ones. The checks
        // before the read catch a tick after their first read of the seconds;
        // only those after it catch one after the third, the read's own.
        for reads in [0, 2] {
            let mut fake = Fake::new(StatusB::HOURS_24);
            let century = Century::register(CENTURY).unwrap();
            let before = DateTime::new(2026, 11, 26, 12, 34, 59).unwrap();
            Rtc::new(&mut fake).set_date_time(before, century).unwrap();
            fake.tick_after = Some((
                reads,
                &[(Register::Seconds, 0x00), (Register::Minutes, 0x35)],
            ));
            assert_eq!(
                Rtc::new(&mut fake).date_time(century),
                Ok(DateTime::new(2026, 11, 26, 12, 35, 0).unwrap()),
                "{reads}"
            );
        }
    }

    #[test]
    fn stuck_in_update() {
        // UIP stays set in status A, over the divider and rate bits PCs use.
        let mut fake = Fake::new(StatusB::HOURS_24);
        fake.set(Register::StatusA, 0xa6);
        let mut rtc = Rtc::new(&mut fake);
        assert_eq!(
            rtc.date_time(Century::window(2000)),
            Err(Error::NotResponding)
        );
        assert_eq!(rtc.alarm(), Err(Error::NotResponding));
        let alarm = Alarm::new(Some(13), None, None).unwrap();
        assert_eq!(rtc.set_alarm(alarm), Err(Error::NotResponding));
        assert_eq!(rtc.wait_for_next_second(), Err(Error::NotResponding));
        assert_eq!(fake.writes, []);
    }

    #[test]
    fn update_bound() {
        // An update seen by every try but the last still ends in time; one
        // seen by every try does not.
        let mut fake = Fake::new(StatusB::HOURS_24);
        let date_time = DateTime::new(2026, 11, 26, 13, 5, 9).unwrap();
        let century = Century::window(2000);
        Rtc::new(&mut fake)
            .set_date_time(date_time, century)
            .unwrap();
        fake.updating = UPDATE_TRIES - 1;
        assert_eq!(Rtc::new(&mut fake).date_time(century), Ok(date_time));
        fake.updating = UPDATE_TRIES;
        assert_eq!(
            Rtc::new(&mut fake).date_time(century),
            Err(Error::NotResponding)
        );
    }

    #[test]
    fn missing_chip() {
        // Every read gives 0xff, so an update always seems in progress.
        let mut rtc = Rtc::new(Missing);
        assert_eq!(
            rtc.date_time(Century::window(2000)),
            Err(Error::NotResponding)
        );
        assert_eq!(rtc.alarm(), Err(Error::NotResponding));
        let alarm = Alarm::new(Some(13), None, None).unwrap();
        assert_eq!(rtc.set_alarm(alarm), Err(Error::NotResponding));
        assert_eq!(rtc.wait_for_next_second(), Err(Error::NotResponding));
    }

    #[test]
    fn set_date_time_halts_updates() {
        // Datasheet values: status B is 0x0b, SET is 0x80, UIE is 0x10 and
        // 24-hour is 0x02. Setting SET clears UIE, which must come back.
        let date_time = DateTime::new(2026, 11, 26, 13, 5, 9).unwrap();
        let century = Century::register(CENTURY).unwrap();
        for (status_b, halted, running) in
            [(0x02, 0x82, 0x02), (0x82, 0x82, 0x02), (0x12, 0x92, 0x12)]
        {
            let mut fake = Fake::new(status_b);
            Rtc::new(&mut fake)
                .set_date_time(date_time, century)
                .unwrap();
            let (first, rest) = fake.writes.split_first().unwrap();
            let (last, clock) = rest.split_last().unwrap();
            assert_eq!(*first, (0x0b, halted));
            assert_eq!(*last, (0x0b, running));
            let mut written: Vec<u8> = clock.iter().map(|&(register, _)| register).collect();
            written.sort_unstable();
            assert_eq!(written, [0x00, 0x02, 0x04, 0x06, 0x07, 0x08, 0x09, CENTURY]);
        }
    }

    #[test]
    fn out_of_window_leaves_chip() {
        let mut fake = Fake::new(0);
        let before = fake.registers;
        let date_time = DateTime::new(2100, 1, 1, 0, 0, 0).unwrap();
        let result = Rtc::new(&mut fake).set_date_time(date_time, Century::window(2000));
        assert_eq!(result, Err(Error::YearOutOfRange));
        assert_eq!(fake.registers, before);
    }

    #[test]
    fn before_century_register_leaves_chip() {
        // 1899 would read back as a lost century.
        let mut fake = Fake::new(0);
        let before = fake.registers;
        let date_time = DateTime::new(1899, 12, 31, 23, 59, 59).unwrap();
        let century = Century::register(CENTURY).unwrap();
        let result = Rtc::new(&mut fake).set_date_time(date_time, century);
        assert_eq!(result, Err(Error::YearOutOfRange));
        assert_eq!(fake.registers, before);
    }

    #[test]
    fn invalid_date_time() {
        let mut fake = Fake::new(0);
        fake.set(Register::Month, 0x13);
        assert_eq!(
            Rtc::new(&mut fake).date_time(Century::window(2000)),
            Err(Error::InvalidContents)
        );
    }

    #[test]
    fn lost_century() {
        // A century register cleared, or never kept, holds no date.
        let mut fake = Fake::new(StatusB::HOURS_24);
        let date_time = DateTime::new(2026, 11, 26, 13, 5, 9).unwrap();
        let century = Century::register(CENTURY).unwrap();
        Rtc::new(&mut fake)
            .set_date_time(date_time, century)
            .unwrap();
        fake.registers[usize::from(CENTURY)] = 0;
        assert_eq!(
            Rtc::new(&mut fake).date_time(century),
            Err(Error::InvalidContents)
        );
    }

    #[test]
    fn alarm() {
        let mut fake = Fake::new(StatusB::BINARY | StatusB::HOURS_24);
        Rtc::new(&mut fake)
            .set_alarm(Alarm::new(None, Some(30), Some(0)).unwrap())
            .unwrap();
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
            rtc.set_alarm(alarm).unwrap();
            assert_eq!(rtc.alarm(), Ok(alarm));
        }
    }

    #[test]
    fn set_alarm_retries_lost_write() {
        // An update starts once status B, the seconds, status A and the
        // seconds again have been read, so the writes that follow are lost;
        // status A shows the update after them.
        let mut fake = Fake::new(StatusB::BINARY | StatusB::HOURS_24);
        fake.update_after = Some((3, 1));
        let alarm = Alarm::new(Some(13), Some(30), Some(5)).unwrap();
        assert_eq!(Rtc::new(&mut fake).set_alarm(alarm), Ok(()));
        assert_eq!(fake.get(Register::HoursAlarm), 13);
        assert_eq!(fake.get(Register::MinutesAlarm), 30);
        assert_eq!(fake.get(Register::SecondsAlarm), 5);
        let writes = [(0x05, 13), (0x03, 30), (0x01, 5)];
        assert_eq!(fake.writes, [writes, writes].concat());
    }

    #[test]
    fn alarm_waits_out_update() {
        // Read during the update, the alarm registers would give 0xff, a
        // valid alarm that matches every time.
        let mut fake = Fake::new(StatusB::HOURS_24);
        let alarm = Alarm::new(Some(13), Some(30), Some(5)).unwrap();
        Rtc::new(&mut fake).set_alarm(alarm).unwrap();
        fake.updating = 3;
        assert_eq!(Rtc::new(&mut fake).alarm(), Ok(alarm));
        assert_eq!(fake.updating, 0);
    }

    #[test]
    fn alarm_invalid() {
        let mut fake = Fake::new(0);
        fake.set(Register::MinutesAlarm, 0x60);
        assert_eq!(Rtc::new(&mut fake).alarm(), Err(Error::InvalidContents));
    }

    #[test]
    fn cmos() {
        // Registers 0x0e and 0x7f, the first and last after the clock's on a
        // PC, are reached as they are numbered.
        assert_eq!(NVRAM, 0x0e..0x80);
        let mut fake = Fake::new(0);
        fake.registers[0x0e] = 0x11;
        fake.registers[0x7f] = 0x22;
        let mut rtc = Rtc::new(&mut fake);
        assert_eq!(rtc.read_cmos(0x0e), 0x11);
        assert_eq!(rtc.read_cmos(0x7f), 0x22);
        rtc.write_cmos(0x0e, 0xaa);
        rtc.write_cmos(0x7f, 0xbb);
        assert_eq!(rtc.read_cmos(0x0e), 0xaa);
        assert_eq!(fake.registers[0x0e], 0xaa);
        assert_eq!(fake.registers[0x7f], 0xbb);
        assert_eq!(fake.writes, [(0x0e, 0xaa), (0x7f, 0xbb)]);
    }

    #[test]
    #[should_panic(expected = "0x0d is outside NVRAM")]
    fn read_cmos_below_nvram() {
        let _ = Rtc::new(Fake::new(0)).read_cmos(0x0d);
    }

    #[test]
    #[should_panic(expected = "0x80 is outside NVRAM")]
    fn read_cmos_above_nvram() {
        let _ = Rtc::new(Fake::new(0)).read_cmos(0x80);
    }

    #[test]
    #[should_panic(expected = "0x0d is outside NVRAM")]
    fn write_cmos_below_nvram() {
        Rtc::new(Fake::new(0)).write_cmos(0x0d, 0);
    }

    #[test]
    #[should_panic(expected = "0x80 is outside NVRAM")]
    fn write_cmos_above_nvram() {
        Rtc::new(Fake::new(0)).write_cmos(0x80, 0);
    }

    /// A chip whose NVRAM registers each hold their number, inverted so that
    /// none holds 0.
    fn numbered_nvram() -> Fake {
        let mut fake = Fake::new(0);
        for register in NVRAM {
            fake.registers[usize::from(register)] = !register;
        }
        fake
    }

    #[test]
    fn read_cmos_into() {
        // The whole of NVRAM, then two registers at each end and one at the
        // last, each read once and in order.
        for (start, len) in [(0x0e, NVRAM.len()), (0x0e, 2), (0x7e, 2), (0x7f, 1)] {
            let mut fake = numbered_nvram();
            let mut buf = vec![0; len];
            Rtc::new(&mut fake).read_cmos_into(start, &mut buf);
            let registers: Vec<u8> = (start..).take(len).collect();
            let expected: Vec<u8> = registers.iter().map(|register| !register).collect();
            assert_eq!(buf, expected, "{start:#04x}");
            assert_eq!(fake.reads, registers, "{start:#04x}");
        }
    }

    #[test]
    fn read_cmos_into_nothing() {
        // As with a subslice, an empty one may start at either end.
        for start in [0x0e, 0x7f, 0x80] {
            let mut fake = numbered_nvram();
            Rtc::new(&mut fake).read_cmos_into(start, &mut []);
            assert_eq!(fake.reads, [], "{start:#04x}");
        }
    }

    #[test]
    #[should_panic(expected = "registers 0x0d..0x0f run outside NVRAM")]
    fn read_cmos_into_below_nvram() {
        Rtc::new(Fake::new(0)).read_cmos_into(0x0d, &mut [0; 2]);
    }

    #[test]
    #[should_panic(expected = "registers 0x7f..0x81 run outside NVRAM")]
    fn read_cmos_into_above_nvram() {
        Rtc::new(Fake::new(0)).read_cmos_into(0x7f, &mut [0; 2]);
    }

    /// Ranges that start, end or lie outside NVRAM, some of them empty.
    const OUTSIDE_NVRAM: [(u8, usize); 9] = [
        (0x0d, 0),
        (0x0d, 1),
        (0x0d, 0x73),
        (0x00, 0x0e),
        (0x0e, 0x73),
        (0x7f, 2),
        (0x80, 1),
        (0x81, 0),
        (0xff, 1),
    ];

    #[test]
    fn read_cmos_into_outside_nvram_reads_nothing() {
        // The whole range is checked first, so no byte is read, nor filled,
        // before the panic.
        for (start, len) in OUTSIDE_NVRAM {
            let mut fake = numbered_nvram();
            let mut buf = vec![0x55; len];
            let read = catch_unwind(AssertUnwindSafe(|| {
                Rtc::new(&mut fake).read_cmos_into(start, &mut buf);
            }));
            assert!(read.is_err(), "{start:#04x} {len}");
            assert_eq!(fake.reads, [], "{start:#04x} {len}");
            assert_eq!(buf, vec![0x55; len], "{start:#04x} {len}");
        }
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
        fake.tick_after = Some((5, &[(Register::Seconds, 0x42)]));
        assert_eq!(Rtc::new(&mut fake).wait_for_next_second(), Ok(()));
        assert_eq!(fake.get(Register::Seconds), 0x42);
        assert!(fake.tick_after.is_none());
    }

    #[test]
    fn next_second_skips_update() {
        // A stall after a read of status A that finds no update lets one
        // start before the seconds are read, which then read 0xff; that must
        // not end the wait. Five reads find the first second and two more
        // find the clock running, the update starts after the next read of
        // status A, and the clock ticks in it.
        let mut fake = Fake::new(0);
        fake.set(Register::Seconds, 0x41);
        fake.update_after = Some((8, 2));
        fake.tick_after = Some((4, &[(Register::Seconds, 0x42)]));
        assert_eq!(Rtc::new(&mut fake).wait_for_next_second(), Ok(()));
        assert!(fake.update_after.is_none() && fake.tick_after.is_none());
        assert_eq!(fake.updating, 0);
    }

    #[test]
    fn next_second_bound() {
        // The first second takes three reads of the seconds, and each try
        // that finds it again takes two more. A tick after every try but the
        // last still ends the wait; one after every try is too late, as if
        // the oscillator had stopped with nothing in status A or B to show it.
        for (reads, result) in [
            (2 * SECOND_TRIES, Ok(())),
            (2 * SECOND_TRIES + 2, Err(Error::NotResponding)),
        ] {
            let mut fake = Fake::new(0);
            fake.set(Register::Seconds, 0x41);
            fake.tick_after = Some((reads, &[(Register::Seconds, 0x42)]));
            assert_eq!(Rtc::new(&mut fake).wait_for_next_second(), result);
        }
    }

    #[test]
    fn next_second_halted() {
        // Datasheet values: SET is 0x80 of status B, and DV2 to DV0 are 0x40,
        // 0x20 and 0x10 of status A, which PCs set to 0x20. Either DV2 and DV1
        // both set, which hold the divider in reset whatever DV0 says, or SET
        // fails the wait at once.
        for (status_a, status_b) in [(0x26, 0x82), (0x66, 0x02), (0x76, 0x02)] {
            let mut fake = Fake::new(status_b);
            fake.set(Register::StatusA, status_a);
            fake.set(Register::Seconds, 0x41);
            fake.tick_after = Some((5, &[(Register::Seconds, 0x42)]));
            assert_eq!(
                Rtc::new(&mut fake).wait_for_next_second(),
                Err(Error::Halted)
            );
            assert!(fake.tick_after.is_some());
        }
        // DV0 next to DV1 alone is AMD's bank select on a running clock.
        let mut fake = Fake::new(0x02);
        fake.set(Register::StatusA, 0x36);
        fake.set(Register::Seconds, 0x41);
        fake.tick_after = Some((5, &[(Register::Seconds, 0x42)]));
        assert_eq!(Rtc::new(&mut fake).wait_for_next_second(), Ok(()));
    }
}
