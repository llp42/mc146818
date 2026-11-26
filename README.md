# mc146818

A `no_std` driver for the MC146818 real-time clock and its battery-backed memory: the CMOS clock of PCs.

- Date and time in BCD or binary, 12- or 24-hour mode, with the century from a register (the ACPI FADT `CENTURY`
  field) or a fixed window. Setting the time also sets the day of the week, which the chip does not work out.
- Conversion of the date and time to and from Unix timestamps, and display in ISO 8601.
- Periodic, alarm and update-ended interrupts, with the periodic rate and the alarm readable back.
- Waiting for the next second, to calibrate other clocks against it.
- The battery-backed memory after the clock registers, by register number, a byte or a run of registers at a time.
- The time and the alarm read, and the alarm written, between the chip's updates, as Linux does. Each wait gives up
  after a bounded number of reads with an `Error`, and waiting for the next second fails at once on a halted clock.

The chip is reached through a `Bus` the caller implements (on PCs, I/O ports `0x70` and `0x71`), so the crate has no
dependencies, contains no `unsafe` code and its tests run on the host.

The minimum supported Rust version is 1.99.

## Example

Reading the time at boot on a PC, with the port accesses left out:

```rust
use mc146818::{Bus, Century, Rtc};

/// The chip behind I/O ports 0x70 (index) and 0x71 (data).
struct Cmos;

impl Bus for Cmos {
    fn read(&mut self, register: u8) -> u8 {
        // outb(0x70, register); inb(0x71)
        todo!()
    }

    fn write(&mut self, register: u8, value: u8) {
        // outb(0x70, register); outb(0x71, value)
        todo!()
    }
}

/// Seconds since 1970 for a clock kept in UTC, given the `CENTURY` field of the ACPI FADT.
fn boot_time(fadt_century: u8) -> Result<i64, mc146818::Error> {
    let mut rtc = Rtc::new(Cmos);
    // The field is 0 if there is no century register: the years are then taken to be 2000 to 2099.
    let century = Century::register(fadt_century).unwrap_or(Century::window(2000));
    Ok(rtc.date_time(century)?.unix_timestamp())
}
```

## Time zones

The chip keeps no time zone, only whatever time the machine was set to: often local time on PCs that also boot
Windows. `DateTime::unix_timestamp` and `DateTime::from_unix_timestamp` take that time as UTC. For a clock kept in
local time, subtract the zone's UTC offset from the timestamp, or add it to one before converting it back.

## Left to the firmware

The firmware normally sets up the following before the kernel runs. The driver leaves each as it finds it and offers
no way to change it:

- The oscillator and divider chain (`DV2` to `DV0` of status A): the driver neither starts the oscillator nor selects
  the time base. Nor does it reset the divider while it sets the time, so the first update after `set_date_time` comes
  anywhere within the next second: the time set is good to about a second.
- The square-wave output and daylight-saving enables (`SQWE` and `DSE` of status B), which PCs do not use.
- The data mode, binary or BCD and 12- or 24-hour: the driver reads it from status B on each access and follows it.

## Not covered

What chips and chipsets add beyond the MC146818, but for the century:

- The date and month alarms (ACPI FADT `DAY_ALRM` and `MON_ALRM`). The FADT gives their CMOS registers as it gives the
  century's (`CENTURY`), which the driver takes: the two-digit year cannot be read without the century, or a window in
  its place, while these alarms only let an alarm be more than a day away.
- Extended memory, such as the second 128 bytes some chipsets reach through ports `0x72` and `0x73`.

## Development

CI runs these checks on each PR:

```sh
cargo fmt --check
cargo clippy --all-targets --locked
cargo test --locked
cargo doc --no-deps --locked
cargo build --locked --target x86_64-unknown-none
```

`Cargo.toml` denies every warning, `missing_docs` and Clippy's pedantic and nursery groups included, so these checks
need no flags. CI runs them on the toolchain pinned in `.github/actions/setup-rust`. The last check needs its
target: `rustup target add x86_64-unknown-none`.

CI also packages the crate, builds the library on Rust 1.99, the minimum, and lints the Markdown with markdownlint-cli2.

## License

MIT, see [LICENSE](LICENSE).
