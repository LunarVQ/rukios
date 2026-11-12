//! rukios scheduler. See `docs/scheduler.md`.

#![cfg_attr(not(test), no_std)]

mod queue;
mod scheduler;
mod thread;

pub use scheduler::{Decision, SchedError, Scheduler};
pub use thread::{
    DomainId, ExecKind, Priority, ThreadId, ThreadInfo, ThreadParams, ThreadState, NUM_PRIORITIES,
};
