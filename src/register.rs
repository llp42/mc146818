// SPDX-FileCopyrightText: 2026 Leonardo Lopes Pereira <leonardolopespereira@outlook.com>
// SPDX-FileContributor: Leonardo Lopes Pereira <leonardolopespereira@outlook.com>
// SPDX-License-Identifier: MIT

//! Register indices and bits.

#[derive(Clone, Copy)]
#[repr(u8)]
pub enum Register {
    Seconds = 0x00,
    SecondsAlarm = 0x01,
    Minutes = 0x02,
    MinutesAlarm = 0x03,
    Hours = 0x04,
    HoursAlarm = 0x05,
    DayOfWeek = 0x06,
    DayOfMonth = 0x07,
    Month = 0x08,
    Year = 0x09,
    StatusA = 0x0a,
    StatusB = 0x0b,
    StatusC = 0x0c,
    StatusD = 0x0d,
}

pub struct StatusA;

impl StatusA {
    /// Set from 244 µs before an update cycle until it ends; the clock
    /// registers are unreliable meanwhile.
    pub const UPDATE_IN_PROGRESS: u8 = 0x80;
    /// DV2 and DV1, which hold the divider chain in reset when both are set.
    /// DV0 is left out: on AMD it selects a bank instead.
    pub const DIVIDER_RESET: u8 = 0x60;
    pub const RATE_SELECT: u8 = 0x0f;
}

pub struct StatusB;

impl StatusB {
    /// Halts update cycles while the clock is set.
    pub const SET: u8 = 0x80;
    /// Values are binary instead of BCD.
    pub const BINARY: u8 = 0x04;
    /// Hours run 0–23 instead of 1–12 with bit 7 for PM.
    pub const HOURS_24: u8 = 0x02;
}

pub struct StatusD;

impl StatusD {
    /// Cleared once the battery runs down.
    pub const VALID: u8 = 0x80;
}

/// Battery-backed memory: registers `START..END`.
pub struct Nvram;

impl Nvram {
    pub const START: u8 = 0x0e;
    pub const END: u8 = 0x80;
}
