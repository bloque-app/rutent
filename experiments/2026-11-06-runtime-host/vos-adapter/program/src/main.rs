//! The program's entry: VOS's actor runtime around [`rutent_vos_program::Rutent`].

#![no_std]
#![no_main]

vos_sdk::actor_program!(rutent_vos_program::rutent);
