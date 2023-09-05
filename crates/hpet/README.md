# HPET

The HPET (High-Precision Event Timer) is a hardware module intended to replace
the legacy 8254 Programmable Interval Timer and Real Time Clock Periodic
interrupt generation functions that are often used on PCs.

A HEPT contains a monotonically increasing *counter* and between 3 and 32
*timers* which can be used to generate interrupts.

Timers 0 and 1 may be connected to ISRs 0 and 8 (IRQs 2 and 8) respectively,
which allows legacy software to supplement use of the 8254 and RTC hardware.

A specification is available online, though note that it contains a number of
miscellaneous errata:

https://www.intel.com/content/dam/www/public/us/en/documents/technical-specifications/software-developers-hpet-spec-1-0a.pdf

To use a HPET, you will need to load the `"HPET"` section from the ACPI
tables, which contains useful information like the base address of the HPET
in memory and how long each tick takes (in femtoseconds).
