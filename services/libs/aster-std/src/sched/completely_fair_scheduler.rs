use core::{
    sync::atomic::{ AtomicIsize, Ordering as AtomicOrdering },
    cmp::Ordering,
};

use crate::prelude::*;
use alloc::{
    collections::{ BinaryHeap, BTreeMap },
    sync::Arc
};
use intrusive_collections::LinkedList;
use jinux_frame::{
    config::NICE_RANGE,
    task::{ Scheduler, Task, TaskAdapter, set_scheduler},
};

pub fn init() {
    let completely_fair_scheduler = Box::new(CompletelyFairScheduler::new());
    let scheduler = Box::<CompletelyFairScheduler>::leak(completely_fair_scheduler);
    set_scheduler(scheduler);
}

pub const fn nice_to_weight(nice: i8) -> isize {
    const NICE_TO_WEIGHT: [isize; 40] = [
        88761, 71755, 56483, 46273, 36291, 29154, 23254, 18705, 14949, 11916, 9548, 7620, 6100,
        4904, 3906, 3121, 2501, 1991, 1586, 1277, 1024, 820, 655, 526, 423, 335, 272, 215, 172,
        137, 110, 87, 70, 56, 45, 36, 29, 23, 18, 15,
    ];
    NICE_TO_WEIGHT[(nice + 20) as usize]
}

/// The virtual runtime
#[derive(Clone)]
pub struct VRuntime {
    vruntime: isize,
    delta: isize,
    nice: i8,

    task: Arc<Task>,
}

impl VRuntime {
    pub fn new(scheduler: &CompletelyFairScheduler, task: Arc<Task>) -> VRuntime {
        VRuntime {
            // BUG: Keeping creating new tasks can cause starvation.
            vruntime: scheduler.min_vruntime.load(AtomicOrdering::Relaxed),
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

impl Ord for VRuntime {
    fn cmp(&self, other: &Self) -> Ordering {
        // Reverse the result for the implementation of min-heap.
        other.get().cmp(&self.get())
    }
}

impl PartialOrd for VRuntime {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Eq for VRuntime {}

impl PartialEq for VRuntime {
    fn eq(&self, other: &Self) -> bool {
        self.get() == other.get()
    }
}

/// The Completely Fair Scheduler(CFS)
/// 
/// Real-time tasks are placed in the `real_time_tasks` queue and
/// are always prioritized during scheduling.
/// Normal tasks are placed in the `normal_tasks` queue and are only
/// scheduled for execution when there are no real-time tasks.
pub struct CompletelyFairScheduler {
    min_vruntime: AtomicIsize,
    // TODO: `vruntimes` currently never shrinks.
    /// `VRuntime`'s created are stored here for looking up.
    vruntimes: SpinLock<BTreeMap<usize, Arc<VRuntime>>>,
    /// Tasks with a priority of less than 100 are regarded as real-time tasks.
    real_time_tasks: SpinLock<LinkedList<TaskAdapter>>,
    /// Tasks with a priority greater than or equal to 100 are regarded as normal tasks.
    normal_tasks: SpinLock<BinaryHeap<Arc<VRuntime>>>,

}

impl CompletelyFairScheduler {
    pub fn new() -> Self {
        Self {
            min_vruntime: AtomicIsize::new(0),
            vruntimes: SpinLock::new(BTreeMap::<usize, Arc<VRuntime>>::new()),
            real_time_tasks: SpinLock::new(LinkedList::new(TaskAdapter::new())),
            normal_tasks: SpinLock::new(BinaryHeap::<Arc<VRuntime>>::new()),
        }
    }
}

impl Scheduler for CompletelyFairScheduler {
    fn enqueue(&self, task: Arc<Task>) {
        if task.is_real_time() {
            self.real_time_tasks
                .lock_irq_disabled()
                .push_back(task.clone());
        } else {
            // BUG: address is not a strictly unique key
            let address = Arc::as_ptr(&task) as usize;
            self.vruntimes.lock_irq_disabled().entry(address).or_insert_with(|| Arc::new(VRuntime::new(self, task)));
            let vruntime = self.vruntimes.lock_irq_disabled().get(&address).unwrap().clone();
            self.normal_tasks
                .lock_irq_disabled()
                .push(vruntime);
        }
    }

    fn dequeue(&self) -> Option<Arc<Task>> {
        if !self.real_time_tasks.lock_irq_disabled().is_empty() {
            self.real_time_tasks.lock_irq_disabled().pop_front()
        } else {
            self.normal_tasks.lock_irq_disabled().pop().as_ref().map(|vruntime| vruntime.task.clone())
        }
    }

    fn should_preempt(&self, task: &Arc<Task>) -> bool {
        // TODO: tick?
        if task.is_real_time() {
            false
        } else if !self.real_time_tasks.lock_irq_disabled().is_empty() {
            true
        } else if self.normal_tasks.lock_irq_disabled().is_empty() {
            false
        } else {
            self.normal_tasks.lock_irq_disabled().peek().unwrap() > self.vruntimes.lock_irq_disabled().get(&(Arc::as_ptr(task) as usize)).unwrap()
        }
    }
}
