// SPDX-FileCopyrightText: 2026 Leonardo Lopes Pereira <leonardolopespereira@outlook.com>
// SPDX-FileContributor: Leonardo Lopes Pereira <leonardolopespereira@outlook.com>
// SPDX-License-Identifier: MIT

//! A chip in memory, for tests.

use crate::Bus;
use crate::register::{Register, StatusA, StatusD};

pub struct Fake {
    pub registers: [u8; 128],
    /// Reads of the seconds register before it advances.
    pub tick_after: Option<u32>,
}

impl Fake {
    /// A running chip with a valid battery and `status_b`.
    pub fn new(status_b: u8) -> Self {
        let mut fake = Self {
            registers: [0; 128],
            tick_after: None,
        };
        fake.set(Register::StatusA, 0x26);
        fake.set(Register::StatusB, status_b);
        fake.set(Register::StatusD, StatusD::VALID);
        fake
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
        if register == Register::StatusC as u8 {
            self.registers[index] = 0;
        }
        if register == Register::Seconds as u8 {
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
        self.registers[index] = if register == Register::StatusA as u8 {
            (self.registers[index] & StatusA::UPDATE_IN_PROGRESS)
                | (value & !StatusA::UPDATE_IN_PROGRESS)
        } else {
            value
        };
    }
}
