# Changelog

All notable changes to the crate are in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the crate adheres to
[Semantic Versioning](https://semver.org/spec/v2.0.0.html). Before 1.0.0, a new minor version, such as 0.3.0, may
break the API; Cargo takes it for an incompatible one, so a requirement of `"0.2"` does not pick up 0.3.0.

## [0.3.0] - 2026-11-26

Upgrading from 0.2: the breaking changes come first, under Changed, each with what replaces the old call.

### Changed

- `YearOutOfWindow` is replaced by `Error`, a `#[non_exhaustive]` enum of `NotResponding`, `Halted`, `InvalidContents`
  and `YearOutOfRange`. `set_date_time` returns `Result<(), Error>`: `Err(YearOutOfWindow)` becomes
  `Err(Error::YearOutOfRange)`.
- `date_time` and `alarm` return a `Result` instead of an `Option`: `if let Some(now) = rtc.date_time(century)` becomes
  `if let Ok(now) = rtc.date_time(century)`. `None` becomes `Err(Error::InvalidContents)`, and a chip that never leaves
  an update gives `Err(Error::NotResponding)`.
- `set_alarm` and `wait_for_next_second` return `Result<(), Error>` instead of `()`: `rtc.set_alarm(alarm)` becomes
  `rtc.set_alarm(alarm)?`, and `rtc.wait_for_next_second()` becomes `rtc.wait_for_next_second()?`.
- `set_periodic_rate` takes a `PeriodicRate` instead of an `Option`: `set_periodic_rate(Some(rate))` becomes
  `set_periodic_rate(rate)`, and `set_periodic_rate(None)` becomes `stop_periodic()`.
- NVRAM is addressed by register number, in the range `NVRAM`, `0x0e..0x80`, instead of by offset from its start:
  `read_nvram(n)` becomes `read_cmos(0x0e + n)`, `write_nvram(n, value)` becomes `write_cmos(0x0e + n, value)`, and
  `Rtc::NVRAM_LEN` becomes `NVRAM.len()`, or `NVRAM.end - NVRAM.start` for a `u8` const.
- With `Century::register`, setting a year before 1900 fails with `Error::YearOutOfRange`, since its century would read
  back as a lost register. A `Century::window` still takes such years.
- Not breaking: `Rtc::new`, `Rtc::into_bus`, `Rtc::periodic_rate`, `Rtc::enabled` and `Rtc::read_cmos` are
  `#[must_use]`. `Debug` shows `Interrupts` by source name, as in `Interrupts(PERIODIC | ALARM)`, `PeriodicRate` in
  hertz, as in `PeriodicRate { hz: 128 }`, and `Century` as its register in hex or its window's first year, as in
  `Century { register: 0x32 }` and `Century { window: 2000 }`.

### Added

- `DateTime::unix_timestamp` and `DateTime::from_unix_timestamp`, which take the clock's time as UTC.
- `Display` for `DateTime`, in ISO 8601: `2026-11-26T13:05:09`.
- `Rtc::read_cmos_into` and `Rtc::write_cmos_from`, for a run of consecutive NVRAM registers.
- `Interrupts::intersects`, `Interrupts::intersection` and `Interrupts::difference`, with `&`, `&=`, `-` and `-=`.
- `Error`, `NVRAM`, `Rtc::read_cmos`, `Rtc::write_cmos` and `Rtc::stop_periodic`, in place of what Changed lists.
- Examples for `Rtc::date_time`, `Rtc::set_alarm`, `Rtc::read_cmos_into` and `DateTime::unix_timestamp`, and one in
  the README, all run as doctests.

### Fixed

- `date_time` waited without end on a chip that stays in an update, as a missing one seems to, and read the time until
  two reads matched. It now reads the time once between updates, as Linux does: it reads the seconds, then checks that
  no update is in progress and that the seconds held, both before and after reading the time. It gives up with
  `Error::NotResponding` after a bounded number of reads.
- `alarm` read the alarm registers even during an update, when their contents are undefined, and `set_alarm` wrote
  them even then, when some chipsets drop the writes. Both now wait for a time between updates, as `date_time` does,
  and `set_alarm` writes again if an update may have come in between.
- `wait_for_next_second` never returned on a halted clock or a missing chip. It now fails at once with `Error::Halted`
  if SET is set or the divider chain is held in reset, and with `Error::NotResponding` after a bounded number of reads.
  It reads in Linux's order, so a stall after a check, such as an SMI, can no longer let an update end the wait early.
- With `Century::register`, a century below 19, as in a register lost or never kept, read as a year before 1900. Reading
  it now fails with `Error::InvalidContents`, as in Linux.
- The docs of `is_valid` say that any read of status D sets VRT, so that after VRT is cleared only the first call
  returns false, and those of `enable` that a stale flag raises the interrupt at once unless acknowledged first.

### Development

- `Cargo.toml` warns on its lints instead of denying them, and CI denies every warning, on a pinned Rust, 1.99.0.
- CI checks the formatting and the docs, runs the tests, packages the crate and builds it for `x86_64-unknown-none`
  on each PR, and builds the library on Rust 1.87, the `rust-version`.
- The PR checks run again when a PR is reopened or edited. Publishing tests the release in one job, then publishes it
  in another, in the `release` environment, one release at a time.
- The tests fake update cycles, torn reads and dropped writes, and check the register layout against the datasheet.

## [0.2.0] - 2026-11-26

The library code is the same as in 0.1.0.

### Added

- Workflows that check the title and body of each PR, and the source branch and raised version of a PR into `main`,
  and that publish the crate to crates.io and tag the release when a PR from an `rc/*` branch is merged.

## [0.1.0] - 2026-11-26

### Added

- First release: a `no_std` driver for the MC146818, reached through a `Bus` the caller implements, with the date and
  time, the century from a register or a window, the periodic, alarm and update-ended interrupts, the alarm, waiting
  for the next second, and the NVRAM.

[0.3.0]: https://github.com/llp42/mc146818/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/llp42/mc146818/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/llp42/mc146818/releases/tag/v0.1.0
