use core::sync::atomic::AtomicIsize;

use crate::prelude::*;
use alloc::{collections::BinaryHeap, sync::Arc};
use jinux_frame::{
    config::NICE_RANGE,
    task::{Scheduler, Task},
};

pub const fn nice_to_weight(nice: i8) -> isize {
    const NICE_TO_WEIGHT: [isize; 40] = [
        88761, 71755, 56483, 46273, 36291, 29154, 23254, 18705, 14949, 11916, 9548, 7620, 6100,
        4904, 3906, 3121, 2501, 1991, 1586, 1277, 1024, 820, 655, 526, 423, 335, 272, 215, 172,
        137, 110, 87, 70, 56, 45, 36, 29, 23, 18, 15,
    ];
    NICE_TO_WEIGHT[(nice + 20) as usize]
}

#[derive(Clone)]
pub struct VRuntime {
    vruntime: isize,
    delta: isize,
    nice: i8,

    task: Arc<Task>,
}

impl VRuntime {
    pub fn new(task: Arc<Task>) -> VRuntime {
        VRuntime {
            vruntime: 0,
            delta: 0,
            nice: task.priority().as_nice().unwrap(),
            task,
        }
    }

    pub fn weight(&self) -> isize {
        nice_to_weight(self.nice)
    }

    pub fn get(&self) -> isize {
        self.vruntime + (self.delta * 1024 / self.weight())
    }

    pub fn set(&mut self, vruntime: isize) {
        self.vruntime = vruntime;
    }

    pub fn set_nice(&mut self, nice: i8) {
        assert!(NICE_RANGE.contains(&nice));

        self.set(self.get());
        self.delta = 0;
        self.nice = nice;
    }

    pub fn tick(&mut self) {
        self.delta += 1;
    }
}

pub struct CompletelyFairScheduler {
    // min_vruntime: AtomicIsize,
    // rq: SpinLock<BinaryHeap<VRuntime>>,
    // dq: SpinLock<BTreeMap<usize, VRuntime>>,
}

impl Scheduler for CompletelyFairScheduler {
    fn enqueue(&self, task: Arc<Task>) {
        todo!()
    }

    fn dequeue(&self) -> Option<Arc<Task>> {
        todo!()
    }

    fn should_preempt(&self, task: &Arc<Task>) -> bool {
        todo!()
    }
}
