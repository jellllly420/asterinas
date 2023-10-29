// SPDX-License-Identifier: MPL-2.0

use crate::config::{DEFAULT_PRIO, NICE_RANGE, PRIO_RANGE, RT_PRIO_RANGE};

/// The priority of a task.
/// Similar to Linux, a larger value represents a lower priority,
/// with a range of 0 to 139. Priorities ranging from 0 to 99 are considered real-time,
/// while those ranging from 100 to 139 are considered normal.
#[derive(Copy, Clone)]
pub struct Priority(u16);

impl Priority {
    pub fn new(val: u16) -> Self {
        assert!(PRIO_RANGE.contains(&val));
        Self(val)
    }

    pub const fn lowest() -> Self {
        Self(PRIO_RANGE.end - 1)
    }

    pub const fn low() -> Self {
        Self(DEFAULT_PRIO)
    }

    pub const fn normal() -> Self {
        Self(RT_PRIO_RANGE.end)
    }

    pub fn high() -> Self {
        Self::new(10)
    }

    pub const fn highest() -> Self {
        Self(RT_PRIO_RANGE.start)
    }

    pub fn set(&mut self, val: u16) {
        assert!(PRIO_RANGE.contains(&val));
        self.0 = val;
    }

    pub const fn get(self) -> u16 {
        self.0
    }

    pub fn is_real_time(self) -> bool {
        RT_PRIO_RANGE.contains(&self.0)
    }

    pub fn as_nice(self) -> Option<i8> {
        if self.is_real_time() {
            None
        } else {
            Some(self.0.wrapping_sub(DEFAULT_PRIO) as i8)
        }
    }

    pub fn from_nice(nice: i8) -> Self {
        match Self::try_from_nice(nice) {
            Some(prio) => prio,
            None => panic!("invalid nice value: {nice}"),
        }
    }

    pub fn try_from_nice(nice: i8) -> Option<Self> {
        if NICE_RANGE.contains(&nice) {
            Some(Self::new(DEFAULT_PRIO.wrapping_add(nice as u16)))
        } else {
            None
        }
    }
}
