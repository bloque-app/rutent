//! A hostile sandboxed part (scoring sheet `B-02`): the reserve stand-in's, also importing a clock, a host
//! function the binding doesn't declare. Admission must refuse it.
#![no_std]
#![no_main]
rutent_pvm_guest::sandboxed_part!(b"reserve", rutent_pvm_guest::hostile::read_a_clock);
