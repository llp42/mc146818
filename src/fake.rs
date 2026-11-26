// SPDX-FileCopyrightText: 2026 Leonardo Lopes Pereira <leonardolopespereira@outlook.com>
// SPDX-FileContributor: Leonardo Lopes Pereira <leonardolopespereira@outlook.com>
// SPDX-License-Identifier: MIT

//! A chip in memory, for tests.

use crate::Bus;
use crate::register::Register;

// The datasheet's numbers, kept apart from the driver's so that a wrong one
// there shows up.
const SECONDS: u8 = 0x00;
const STATUS_A: u8 = 0x0a;
const STATUS_B: u8 = 0x0b;
const STATUS_C: u8 = 0x0c;
const STATUS_D: u8 = 0x0d;
const UIP: u8 = 0x80;
const SET: u8 = 0x80;
const UIE: u8 = 0x10;
const VRT: u8 = 0x80;

pub struct Fake {
    pub registers: [u8; 128],
    /// The clock ticking mid-read: once the seconds register has been read
    /// that many more times, the next read of it is followed by these writes.
    pub tick_after: Option<(u32, &'static [(Register, u8)])>,
    /// Reads of status A that find an update in progress. Meanwhile the time,
    /// calendar and alarm registers read 0xff, as the datasheet leaves them
    /// undefined, and drop writes, as some chipsets do. Setting SET ends it.
    pub updating: u32,
    /// An update starting mid-access, as after a stall: once that many more
    /// reads of any register have been made, the next one is followed by an
    /// update seen by this many reads of status A.
    pub update_after: Option<(u32, u32)>,
    /// Every register read, in order.
    pub reads: Vec<u8>,
    /// Every write, in order.
    pub writes: Vec<(u8, u8)>,
}

impl Fake {
    /// A running chip with a valid battery and `status_b`.
    pub fn new(status_b: u8) -> Self {
        let mut registers = [0; 128];
        registers[usize::from(STATUS_A)] = 0x26;
        registers[usize::from(STATUS_B)] = status_b;
        registers[usize::from(STATUS_D)] = VRT;
        Self {
            registers,
            tick_after: None,
            updating: 0,
            update_after: None,
            reads: Vec::new(),
            writes: Vec::new(),
        }
    }

    pub const fn get(&self, register: Register) -> u8 {
        self.registers[register as usize]
    }

    pub const fn set(&mut self, register: Register, value: u8) {
        self.registers[register as usize] = value;
    }
}

impl Bus for Fake {
    fn read(&mut self, register: u8) -> u8 {
        self.reads.push(register);
        let index = usize::from(register);
        let mut value = self.registers[index];
        if self.updating > 0 {
            if register == STATUS_A {
                self.updating -= 1;
                value |= UIP;
            } else if register < STATUS_A {
                value = 0xff;
            }
        }
        if register == STATUS_C {
            self.registers[index] = 0;
        }
        if register == SECONDS {
            match self.tick_after {
                Some((0, writes)) => {
                    for &(target, byte) in writes {
                        self.set(target, byte);
                    }
                    self.tick_after = None;
                }
                Some((reads, writes)) => self.tick_after = Some((reads - 1, writes)),
                None => {}
            }
        }
        match self.update_after {
            Some((0, updating)) => {
                self.updating = updating;
                self.update_after = None;
            }
            Some((reads, updating)) => self.update_after = Some((reads - 1, updating)),
            None => {}
        }
        value
    }

    fn write(&mut self, register: u8, value: u8) {
        self.writes.push((register, value));
        if self.updating > 0 && register < STATUS_A {
            return;
        }
        if register == STATUS_B && value & SET != 0 {
            // SET "inhibits any update cycle and then clears the UIP status
            // bit."
            self.updating = 0;
        }
        let index = usize::from(register);
        self.registers[index] = if register == STATUS_A {
            (self.registers[index] & UIP) | (value & !UIP)
        } else if register == STATUS_B && self.registers[index] & SET == 0 && value & SET != 0 {
            // "The SET bit going high clears the UIE bit."
            value & !UIE
        } else {
            value
        };
    }
}

/// No chip: reads float high and writes go nowhere.
pub struct Missing;

impl Bus for Missing {
    fn read(&mut self, _: u8) -> u8 {
        0xff
    }

    fn write(&mut self, _: u8, _: u8) {}
}
