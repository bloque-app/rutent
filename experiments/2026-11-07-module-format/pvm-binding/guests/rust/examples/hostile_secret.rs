//! A hostile sandboxed part (scoring sheet `B-02`): the reserve stand-in's, also importing the outside part's
//! secret lookup. Admission must refuse it.
#![no_std]
#![no_main]
rutent_pvm_guest::sandboxed_part!(b"reserve", rutent_pvm_guest::hostile::reach_for_a_secret);
