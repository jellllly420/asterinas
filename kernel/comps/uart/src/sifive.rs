// SPDX-License-Identifier: MPL-2.0

extern crate alloc;
use alloc::{string::ToString, sync::Arc, vec::Vec};
use core::{hint::spin_loop, mem::offset_of};

use aster_console::{AnyConsoleDevice, ConsoleCallback};
use ostd::{
    arch::{
        boot::DEVICE_TREE,
        irq::{InterruptSourceInFdt, MappedIrqLine, IRQ_CHIP},
    },
    io::IoMem,
    irq::IrqLine,
    mm::{VmIoOnce, VmReader},
    sync::{LocalIrqDisabled, SpinLock},
};
use spin::Once;

use crate::Uart;

pub fn init() {
    let fdt = DEVICE_TREE.get().unwrap();
    let uart_nodes = fdt.all_nodes().filter(|n| {
        n.compatible()
            .is_some_and(|c| c.all().any(|s| s == SifiveUart::FDT_COMPATIBLE))
    });

    let mut sifive_uarts = Vec::new();
    uart_nodes.enumerate().for_each(|(index, uart_node)| {
        if index > 0 {
            return;
        }
        let reg = uart_node.reg().unwrap().next().unwrap();
        let io_mem = IoMem::acquire(
            reg.starting_address as usize..reg.starting_address as usize + reg.size.unwrap(),
        )
        .unwrap();

        let interrupt_source_in_fdt = InterruptSourceInFdt {
            interrupt: uart_node.interrupts().unwrap().next().unwrap() as u32,
            interrupt_parent: uart_node
                .property("interrupt-parent")
                .and_then(|prop| prop.as_usize())
                .unwrap() as u32,
        };
        let mut mapped_irq_line = IrqLine::alloc()
            .and_then(|irq_line| {
                IRQ_CHIP
                    .get()
                    .unwrap()
                    .map_fdt_pin_to(interrupt_source_in_fdt, irq_line)
            })
            .unwrap();
        mapped_irq_line.on_active(|_trapframe| {
            // TODO: Find a way to route the interrupt to the right UART.
            SIFIVE_UARTS.get().unwrap()[0].handle_rx_irq();
        });

        let uart = Arc::new(SifiveUart {
            io_mem,
            _mapped_irq_line: mapped_irq_line,
            callbacks: SpinLock::new(Vec::new()),
        });
        uart.init(SifiveUart::CLOCK_HZ);

        aster_console::register_device(
            format_args!("SIFIVE_UART_CONSOLE_{}", sifive_uarts.len()).to_string(),
            uart.clone(),
        );
        sifive_uarts.push(uart);
    });

    SIFIVE_UARTS.call_once(|| sifive_uarts);
}

static SIFIVE_UARTS: Once<Vec<Arc<SifiveUart>>> = Once::new();

pub struct SifiveUart {
    io_mem: IoMem,
    _mapped_irq_line: MappedIrqLine,
    callbacks: SpinLock<Vec<&'static ConsoleCallback>, LocalIrqDisabled>,
}

impl SifiveUart {
    fn transmit_blocking(&self, byte: u8) {
        while let Err(_) = self.transmit(byte) {
            spin_loop();
        }
    }

    fn handle_rx_irq(&self) {
        let mut buffer = [0u8; 64];
        let mut count = 0;
        while let Some(byte) = self.receive() {
            buffer[count] = byte;
            count += 1;
            if count >= buffer.len() {
                break;
            }
        }

        if count > 0 {
            let callbacks = self.callbacks.lock();
            for callback in callbacks.iter() {
                callback(VmReader::from(&buffer[..count]));
            }
        }
    }
}

impl SifiveUart {
    const FDT_COMPATIBLE: &'static str = "sifive,uart0";

    const TARGET_BAUD: u32 = 115200;
    // FIXME: We should query the clock frequency from the device tree. Here we
    // hardcode it to 500MHz for SiFive Hifive Unleashed board.
    const CLOCK_HZ: u32 = 500_000_000;

    const TXDATA_FULL: u32 = 0b1 << 31;
    const TXDATA_DATA_MASK: u32 = 0xff;
    const RXDATA_EMPTY: u32 = 0b1 << 31;
    const RXDATA_DATA_MASK: u32 = 0xff;
    const TXCTRL_TXEN: u32 = 0b1;
    const RXCTRL_RXEN: u32 = 0b1;
    const IE_RXWM: u32 = 0b1 << 1;
}

impl Uart for SifiveUart {
    fn init(&self, clock_hz: u32) {
        let div = ((clock_hz as u64 + (Self::TARGET_BAUD as u64 / 2)) / (Self::TARGET_BAUD as u64)
            - 1) as u32;
        self.io_mem
            .write_once(offset_of!(Registers, div), &div)
            .unwrap();
        self.io_mem
            .write_once(offset_of!(Registers, txctrl), &Self::TXCTRL_TXEN)
            .unwrap();
        self.io_mem
            .write_once(offset_of!(Registers, rxctrl), &Self::RXCTRL_RXEN)
            .unwrap();
        self.io_mem
            .write_once(offset_of!(Registers, ie), &Self::IE_RXWM)
            .unwrap();
    }

    fn transmit(&self, byte: u8) -> ostd::Result<()> {
        let offset = offset_of!(Registers, txdata);
        let txdata = self.io_mem.read_once::<u32>(offset).unwrap();
        if txdata & Self::TXDATA_FULL != 0 {
            // ostd::early_println!("UART TX full, cannot transmit byte {}", byte);
            Err(ostd::Error::NotEnoughResources)
        } else {
            self.io_mem
                .write_once(offset, &(byte as u32 & Self::TXDATA_DATA_MASK))
                .unwrap();
            Ok(())
        }
    }

    fn receive(&self) -> Option<u8> {
        let offset = offset_of!(Registers, rxdata);
        let rxdata = self.io_mem.read_once::<u32>(offset).unwrap();
        if rxdata & Self::RXDATA_EMPTY != 0 {
            None
        } else {
            Some((rxdata & Self::RXDATA_DATA_MASK) as u8)
        }
    }
}

impl AnyConsoleDevice for SifiveUart {
    fn send(&self, bytes: &[u8]) {
        for &byte in bytes {
            self.transmit_blocking(byte);
        }
    }

    fn register_callback(&self, callback: &'static ConsoleCallback) {
        self.callbacks.lock().push(callback);
    }
}

impl core::fmt::Debug for SifiveUart {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("SifiveUart").finish_non_exhaustive()
    }
}

struct Registers {
    txdata: u32,
    rxdata: u32,
    txctrl: u32,
    rxctrl: u32,
    ie: u32,
    _ip: u32,
    div: u32,
}
