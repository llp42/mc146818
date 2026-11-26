# mc146818

A `no_std` driver for the MC146818 real-time clock and its battery-backed memory: the CMOS clock of PCs.

- Periodic, alarm and update-ended interrupts, with the periodic rate and the alarm readable back.
- Date and time in BCD or binary, 12- or 24-hour mode, with the century from a register (the ACPI FADT `CENTURY`
  field) or a fixed window. Setting the time also sets the day of the week, which the chip does not work out.
- Waiting for the next second, to calibrate other clocks against it.
- The battery-backed memory after the clock registers.

The chip is reached through a `Bus` the caller implements (on PCs, I/O ports `0x70` and `0x71`), so the crate has no
dependencies, contains no `unsafe` code and its tests run on the host.

## Left to the firmware

The firmware normally sets up the following before the kernel runs. The driver leaves each as it finds it and offers
no way to change it:

- The oscillator and divider chain (`DV2` to `DV0` of status A): the driver neither starts the oscillator nor selects
  the time base.
- The square-wave output and daylight-saving enables (`SQWE` and `DSE` of status B), which PCs do not use.
- The data mode, binary or BCD and 12- or 24-hour: the driver reads it from status B on each access and follows it.

## Not covered

What chips and chipsets add beyond the MC146818, but for the century:

- The date and month alarms (ACPI FADT `DAY_ALRM` and `MON_ALRM`). The FADT gives their CMOS registers as it gives the
  century's (`CENTURY`), which the driver takes: the two-digit year cannot be read without the century, or a window in
  its place, while these alarms only let an alarm be more than a day away.
- Extended memory, such as the second 128 bytes some chipsets reach through ports `0x72` and `0x73`.

## License

MIT, see [LICENSE](LICENSE).
