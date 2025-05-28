// SPDX-License-Identifier: MPL-2.0

//! Interrupts.

pub(super) mod chip;
mod ipi;
mod ops;
mod remapping;

pub(crate) use ipi::{send_ipi, HwCpuId};
pub(crate) use ops::{disable_local, enable_local, enable_local_and_halt, is_local_enabled};
pub(crate) use remapping::IrqRemapping;

pub(crate) const IRQ_NUM_MIN: u8 = 0;
pub(crate) const IRQ_NUM_MAX: u8 = 255;

pub(crate) struct HwIrqLine {
    irq_num: u8,
}

impl HwIrqLine {
    pub(super) fn new(irq_num: u8) -> Self {
        Self { irq_num }
    }

    pub(crate) fn irq_num(&self) -> u8 {
        self.irq_num
    }

    pub(crate) fn ack(&self) {
        chip::complete(self.irq_num);
    }
}
