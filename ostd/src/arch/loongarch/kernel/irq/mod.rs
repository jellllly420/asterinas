// SPDX-License-Identifier: MPL-2.0

mod eiointc;

use loongArch64::register::ecfg::LineBasedInterrupt;

use crate::{
    arch::{irq, kernel::irq::eiointc::Eiointc},
    irq::IrqHandle,
};

pub(in crate::arch) fn init() {
    // FIXME: Support SMP in LoongArch
    Eiointc::init(1);
    for i in irq::IRQ_NUM_MIN..=irq::IRQ_NUM_MAX {
        Eiointc::enable(i);
    }
    loongArch64::register::ecfg::set_lie(
        LineBasedInterrupt::HWI0
            | LineBasedInterrupt::HWI1
            | LineBasedInterrupt::HWI2
            | LineBasedInterrupt::HWI3
            | LineBasedInterrupt::HWI4
            | LineBasedInterrupt::HWI5
            | LineBasedInterrupt::HWI6
            | LineBasedInterrupt::HWI7,
    );
}

pub(in crate::arch) fn claim() -> Option<InterruptHandle> {
    Eiointc::claim().map(|irq_num| InterruptHandle { irq_num })
}

fn complete(irq: u8) {
    Eiointc::complete(irq);
}

pub(in crate::arch) struct InterruptHandle {
    irq_num: u8,
}

impl IrqHandle for InterruptHandle {
    fn irq_num(&self) -> u8 {
        self.irq_num
    }

    fn ack(&self) {
        complete(self.irq_num);
    }
}
