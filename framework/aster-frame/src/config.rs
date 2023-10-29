// SPDX-License-Identifier: MPL-2.0

#![allow(unused)]

use log::Level;

pub const USER_STACK_SIZE: usize = PAGE_SIZE * 4;
pub const KERNEL_STACK_SIZE: usize = PAGE_SIZE * 64;
pub const KERNEL_HEAP_SIZE: usize = PAGE_SIZE * 256;

pub const KERNEL_OFFSET: usize = 0xffffffff80000000;

pub const PHYS_OFFSET: usize = 0xFFFF800000000000;
pub const ENTRY_COUNT: usize = 512;

pub const PAGE_SIZE: usize = 0x1000;
pub const PAGE_SIZE_BITS: usize = 0xc;

pub const KVA_START: usize = (usize::MAX) << PAGE_SIZE_BITS;

pub const DEFAULT_LOG_LEVEL: Level = Level::Error;
/// This value represent the base timer frequency in Hz
pub const TIMER_FREQ: u64 = 500;

pub const MIN_NICE: i8 = -20;
pub const MAX_NICE: i8 = 19;
pub const NICE_COUNT: i8 = MAX_NICE - MIN_NICE + 1;

pub const NICE_RANGE: core::ops::Range<i8> = MIN_NICE..(MAX_NICE + 1);
pub const RT_PRIO_RANGE: core::ops::Range<u16> = 0..100;
pub const PRIO_RANGE: core::ops::Range<u16> = 0..(RT_PRIO_RANGE.end + (NICE_COUNT as u16));
pub const DEFAULT_PRIO: u16 = RT_PRIO_RANGE.end + ((NICE_COUNT / 2) as u16);
