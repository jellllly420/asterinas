// SPDX-License-Identifier: MPL-2.0

#![no_std]
#![deny(unsafe_code)]

use component::{init_component, ComponentInitError};

#[cfg(target_arch = "riscv64")]
mod sifive;

pub fn placeholder() {}

#[init_component]
fn uart_init() -> Result<(), ComponentInitError> {
    #[cfg(target_arch = "riscv64")]
    sifive::init();

    Ok(())
}

trait Uart {
    fn init(&self, clock_hz: u32);

    fn transmit(&self, byte: u8) -> ostd::Result<()>;

    fn receive(&self) -> Option<u8>;
}
