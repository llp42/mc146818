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
const VRT: u8 = 0x80;

pub struct Fake {
    pub registers: [u8; 128],
    /// Reads of the seconds register before it advances.
    pub tick_after: Option<u32>,
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
        let index = usize::from(register);
        let value = self.registers[index];
        if register == STATUS_C {
            self.registers[index] = 0;
        }
        if register == SECONDS {
            match self.tick_after {
                Some(0) => {
                    self.registers[index] += 1;
                    self.tick_after = None;
                }
                Some(reads) => self.tick_after = Some(reads - 1),
                None => {}
            }
        }
        value
    }

    fn write(&mut self, register: u8, value: u8) {
        let index = usize::from(register);
        self.registers[index] = if register == STATUS_A {
            (self.registers[index] & UIP) | (value & !UIP)
        } else {
            value
        };
    }
}
