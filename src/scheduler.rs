use crate::queue::ReadyQueues;
use crate::thread::{
    Priority, Slot, ThreadId, ThreadInfo, ThreadParams, ThreadState, NIL, NUM_PRIORITIES,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SchedError {
    Full,
    InvalidThread,
    InvalidPriority,
    InvalidState,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    Continue(ThreadId),
    Switch {
        from: Option<ThreadId>,
        to: ThreadId,
    },
    Idle {
        from: Option<ThreadId>,
    },
}

pub struct Scheduler<const N: usize> {
    slots: [Slot; N],
    ready: ReadyQueues,
    free_head: u16,
    /// Context loaded on the CPU; may be `Blocked` or `Ready` until the next
    /// `schedule()`.
    current: u16,
    need_resched: bool,
    live: usize,
}

impl<const N: usize> Default for Scheduler<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> Scheduler<N> {
    pub const fn new() -> Self {
        assert!(N < NIL as usize, "Scheduler capacity must be below 65535");
        let mut slots = [Slot::FREE; N];
        let mut i = 0;
        while i < N {
            slots[i].next = if i + 1 < N { (i + 1) as u16 } else { NIL };
            i += 1;
        }
        Self {
            slots,
            ready: ReadyQueues::new(),
            free_head: if N > 0 { 0 } else { NIL },
            current: NIL,
            need_resched: false,
            live: 0,
        }
    }

    pub fn spawn(&mut self, params: ThreadParams) -> Result<ThreadId, SchedError> {
        check_priority(params.priority)?;
        let i = self.free_head;
        if i == NIL {
            return Err(SchedError::Full);
        }
        let slot = &mut self.slots[i as usize];
        self.free_head = slot.next;
        let timeslice = params.timeslice.max(1);
        *slot = Slot {
            generation: slot.generation,
            state: Some(ThreadState::Ready),
            domain: params.domain,
            kind: params.kind,
            priority: params.priority,
            timeslice,
            remaining: timeslice,
            next: NIL,
            prev: NIL,
        };
        self.ready.push_back(&mut self.slots, i);
        self.live += 1;
        self.check_preempt(params.priority);
        Ok(self.id_of(i))
    }

    pub fn exit(&mut self, id: ThreadId) -> Result<(), SchedError> {
        let i = self.lookup(id)?;
        if self.slots[i as usize].state == Some(ThreadState::Ready) {
            self.ready.remove(&mut self.slots, i);
        }
        if i == self.current {
            self.current = NIL;
            self.need_resched = true;
        }
        let slot = &mut self.slots[i as usize];
        *slot = Slot {
            generation: slot.generation.wrapping_add(1),
            next: self.free_head,
            ..Slot::FREE
        };
        self.free_head = i;
        self.live -= 1;
        Ok(())
    }

    pub fn block(&mut self, id: ThreadId) -> Result<(), SchedError> {
        let i = self.lookup(id)?;
        match self.slots[i as usize].state {
            Some(ThreadState::Ready) => self.ready.remove(&mut self.slots, i),
            Some(ThreadState::Running) => self.need_resched = true,
            _ => return Err(SchedError::InvalidState),
        }
        self.slots[i as usize].state = Some(ThreadState::Blocked);
        Ok(())
    }

    pub fn wake(&mut self, id: ThreadId) -> Result<(), SchedError> {
        let i = self.lookup(id)?;
        let slot = &mut self.slots[i as usize];
        if slot.state != Some(ThreadState::Blocked) {
            return Err(SchedError::InvalidState);
        }
        slot.state = Some(ThreadState::Ready);
        slot.remaining = slot.timeslice;
        let priority = slot.priority;
        self.ready.push_back(&mut self.slots, i);
        self.check_preempt(priority);
        Ok(())
    }

    pub fn yield_now(&mut self) {
        if let Some(slot) = self.running_slot_mut() {
            slot.remaining = 0;
            self.need_resched = true;
        }
    }

    pub fn set_priority(&mut self, id: ThreadId, priority: Priority) -> Result<(), SchedError> {
        check_priority(priority)?;
        let i = self.lookup(id)?;
        match self.slots[i as usize].state {
            Some(ThreadState::Ready) => {
                self.ready.remove(&mut self.slots, i);
                self.slots[i as usize].priority = priority;
                self.ready.push_back(&mut self.slots, i);
                self.check_preempt(priority);
            }
            Some(ThreadState::Running) => {
                self.slots[i as usize].priority = priority;
                if self.ready.highest().is_some_and(|p| p > priority) {
                    self.need_resched = true;
                }
            }
            _ => self.slots[i as usize].priority = priority,
        }
        Ok(())
    }

    /// Returns whether a reschedule is needed.
    pub fn tick(&mut self) -> bool {
        if let Some(slot) = self.running_slot_mut() {
            slot.remaining = slot.remaining.saturating_sub(1);
            if slot.remaining == 0 {
                self.need_resched = true;
            }
        }
        self.need_resched
    }

    pub fn need_resched(&self) -> bool {
        self.need_resched
    }

    pub fn schedule(&mut self) -> Decision {
        self.need_resched = false;
        let prev = self.current;

        if prev != NIL && self.slots[prev as usize].state == Some(ThreadState::Running) {
            let slot = &mut self.slots[prev as usize];
            let expired = slot.remaining == 0;
            let preempted = self.ready.highest().is_some_and(|p| p > slot.priority);
            if !expired && !preempted {
                return Decision::Continue(self.id_of(prev));
            }
            slot.state = Some(ThreadState::Ready);
            if expired {
                slot.remaining = slot.timeslice;
                self.ready.push_back(&mut self.slots, prev);
            } else {
                self.ready.push_front(&mut self.slots, prev);
            }
        }

        let from = (prev != NIL).then(|| self.id_of(prev));
        match self.ready.pop_highest(&mut self.slots) {
            Some(next) => {
                self.slots[next as usize].state = Some(ThreadState::Running);
                self.current = next;
                if next == prev {
                    Decision::Continue(self.id_of(next))
                } else {
                    Decision::Switch {
                        from,
                        to: self.id_of(next),
                    }
                }
            }
            None => {
                self.current = NIL;
                Decision::Idle { from }
            }
        }
    }

    pub fn current(&self) -> Option<ThreadId> {
        (self.current != NIL).then(|| self.id_of(self.current))
    }

    pub fn info(&self, id: ThreadId) -> Option<ThreadInfo> {
        let i = self.lookup(id).ok()?;
        let s = &self.slots[i as usize];
        Some(ThreadInfo {
            id,
            domain: s.domain,
            kind: s.kind,
            state: s.state?,
            priority: s.priority,
            timeslice: s.timeslice,
            remaining: s.remaining,
        })
    }

    pub fn len(&self) -> usize {
        self.live
    }

    pub fn is_empty(&self) -> bool {
        self.live == 0
    }

    pub const fn capacity(&self) -> usize {
        N
    }

    fn id_of(&self, i: u16) -> ThreadId {
        ThreadId::new(i, self.slots[i as usize].generation)
    }

    fn lookup(&self, id: ThreadId) -> Result<u16, SchedError> {
        match self.slots.get(id.index()) {
            Some(s) if s.state.is_some() && s.generation == id.generation() => {
                Ok(id.index() as u16)
            }
            _ => Err(SchedError::InvalidThread),
        }
    }

    fn running_slot_mut(&mut self) -> Option<&mut Slot> {
        let slot = self.slots.get_mut(self.current as usize)?;
        (slot.state == Some(ThreadState::Running)).then_some(slot)
    }

    fn check_preempt(&mut self, priority: Priority) {
        let should = match self.slots.get(self.current as usize) {
            Some(cur) if cur.state == Some(ThreadState::Running) => priority > cur.priority,
            _ => true,
        };
        if should {
            self.need_resched = true;
        }
    }
}

fn check_priority(priority: Priority) -> Result<(), SchedError> {
    if (priority as usize) < NUM_PRIORITIES {
        Ok(())
    } else {
        Err(SchedError::InvalidPriority)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::thread::{DomainId, ExecKind};

    const HOST: DomainId = DomainId(0);
    const LINUX: DomainId = DomainId(1);

    fn native(priority: Priority, timeslice: u32) -> ThreadParams {
        ThreadParams {
            domain: HOST,
            kind: ExecKind::Native,
            priority,
            timeslice,
        }
    }

    fn vcpu(vcpu: u16, priority: Priority, timeslice: u32) -> ThreadParams {
        ThreadParams {
            domain: LINUX,
            kind: ExecKind::Vcpu { vcpu },
            priority,
            timeslice,
        }
    }

    fn run_ticks<const N: usize>(s: &mut Scheduler<N>, n: u32) {
        for _ in 0..n {
            s.tick();
        }
    }

    #[test]
    fn idle_when_empty() {
        let mut s = Scheduler::<4>::new();
        assert_eq!(s.schedule(), Decision::Idle { from: None });
        assert_eq!(s.current(), None);
    }

    #[test]
    fn single_thread_keeps_cpu_across_slices() {
        let mut s = Scheduler::<4>::new();
        let a = s.spawn(native(5, 2)).unwrap();
        assert!(s.need_resched());
        assert_eq!(s.schedule(), Decision::Switch { from: None, to: a });
        assert!(!s.tick());
        assert!(s.tick());
        assert_eq!(s.schedule(), Decision::Continue(a));
        assert_eq!(s.info(a).unwrap().remaining, 2);
    }

    #[test]
    fn round_robin_within_priority() {
        let mut s = Scheduler::<4>::new();
        let a = s.spawn(native(5, 3)).unwrap();
        let b = s.spawn(vcpu(0, 5, 3)).unwrap();
        let c = s.spawn(vcpu(1, 5, 3)).unwrap();
        assert_eq!(s.schedule(), Decision::Switch { from: None, to: a });

        let mut order = vec![];
        for _ in 0..6 {
            run_ticks(&mut s, 3);
            match s.schedule() {
                Decision::Switch { to, .. } => order.push(to),
                d => panic!("unexpected {d:?}"),
            }
        }
        assert_eq!(order, [b, c, a, b, c, a]);
    }

    #[test]
    fn higher_priority_preempts_and_lower_resumes_first() {
        let mut s = Scheduler::<4>::new();
        let a = s.spawn(native(1, 10)).unwrap();
        let _b = s.spawn(native(1, 10)).unwrap();
        s.schedule();
        run_ticks(&mut s, 4);
        assert!(!s.need_resched());

        let hi = s.spawn(vcpu(0, 9, 10)).unwrap();
        assert!(s.need_resched());
        assert_eq!(
            s.schedule(),
            Decision::Switch {
                from: Some(a),
                to: hi
            }
        );
        assert_eq!(s.info(a).unwrap().state, ThreadState::Ready);

        s.exit(hi).unwrap();
        // `a` was preempted mid-slice so it runs before `b`, with its
        // remaining ticks intact.
        assert_eq!(s.schedule(), Decision::Switch { from: None, to: a });
        assert_eq!(s.info(a).unwrap().remaining, 6);
    }

    #[test]
    fn lower_priority_does_not_preempt() {
        let mut s = Scheduler::<4>::new();
        let a = s.spawn(native(5, 10)).unwrap();
        s.schedule();
        s.spawn(native(2, 10)).unwrap();
        assert!(!s.need_resched());
        run_ticks(&mut s, 10);
        assert_eq!(s.schedule(), Decision::Continue(a));
    }

    #[test]
    fn block_and_wake() {
        let mut s = Scheduler::<4>::new();
        let a = s.spawn(native(5, 4)).unwrap();
        let b = s.spawn(native(3, 4)).unwrap();
        s.schedule();

        s.block(a).unwrap();
        assert!(s.need_resched());
        assert_eq!(
            s.schedule(),
            Decision::Switch {
                from: Some(a),
                to: b
            }
        );
        assert_eq!(s.block(a), Err(SchedError::InvalidState));

        s.wake(a).unwrap();
        assert!(s.need_resched());
        assert_eq!(
            s.schedule(),
            Decision::Switch {
                from: Some(b),
                to: a
            }
        );
        assert_eq!(s.wake(a), Err(SchedError::InvalidState));
    }

    #[test]
    fn blocking_last_thread_idles_and_saves_it() {
        let mut s = Scheduler::<2>::new();
        let a = s.spawn(native(0, 1)).unwrap();
        s.schedule();
        s.block(a).unwrap();
        assert_eq!(s.schedule(), Decision::Idle { from: Some(a) });
        s.wake(a).unwrap();
        assert_eq!(s.schedule(), Decision::Switch { from: None, to: a });
    }

    #[test]
    fn woken_before_switch_out_continues() {
        let mut s = Scheduler::<2>::new();
        let a = s.spawn(native(0, 1)).unwrap();
        s.schedule();
        s.block(a).unwrap();
        s.wake(a).unwrap();
        assert_eq!(s.schedule(), Decision::Continue(a));
        assert_eq!(s.info(a).unwrap().state, ThreadState::Running);
    }

    #[test]
    fn blocking_a_ready_thread_removes_it() {
        let mut s = Scheduler::<4>::new();
        let a = s.spawn(native(1, 1)).unwrap();
        let b = s.spawn(native(1, 1)).unwrap();
        let c = s.spawn(native(1, 1)).unwrap();
        s.schedule();
        s.block(b).unwrap();
        s.tick();
        assert_eq!(
            s.schedule(),
            Decision::Switch {
                from: Some(a),
                to: c
            }
        );
        s.tick();
        assert_eq!(
            s.schedule(),
            Decision::Switch {
                from: Some(c),
                to: a
            }
        );
    }

    #[test]
    fn yield_goes_to_back() {
        let mut s = Scheduler::<4>::new();
        let a = s.spawn(native(1, 5)).unwrap();
        let b = s.spawn(native(1, 5)).unwrap();
        s.schedule();
        s.yield_now();
        assert_eq!(
            s.schedule(),
            Decision::Switch {
                from: Some(a),
                to: b
            }
        );
        assert_eq!(s.info(a).unwrap().remaining, 5);
    }

    #[test]
    fn exit_running_thread() {
        let mut s = Scheduler::<4>::new();
        let a = s.spawn(native(1, 5)).unwrap();
        let b = s.spawn(native(1, 5)).unwrap();
        s.schedule();
        s.exit(a).unwrap();
        assert_eq!(s.current(), None);
        assert_eq!(s.schedule(), Decision::Switch { from: None, to: b });
        assert_eq!(s.len(), 1);
    }

    #[test]
    fn stale_handles_are_rejected_after_reuse() {
        let mut s = Scheduler::<1>::new();
        let a = s.spawn(native(1, 1)).unwrap();
        s.exit(a).unwrap();
        let b = s.spawn(native(1, 1)).unwrap();
        assert_eq!(a.index(), b.index());
        assert_ne!(a, b);
        assert_eq!(s.exit(a), Err(SchedError::InvalidThread));
        assert!(s.info(a).is_none());
        assert!(s.info(b).is_some());
    }

    #[test]
    fn capacity_and_priority_limits() {
        let mut s = Scheduler::<2>::new();
        assert_eq!(s.spawn(native(64, 1)), Err(SchedError::InvalidPriority));
        s.spawn(native(63, 1)).unwrap();
        s.spawn(native(0, 1)).unwrap();
        assert_eq!(s.spawn(native(0, 1)), Err(SchedError::Full));
    }

    #[test]
    fn set_priority_reorders() {
        let mut s = Scheduler::<4>::new();
        let a = s.spawn(native(5, 10)).unwrap();
        let b = s.spawn(native(1, 10)).unwrap();
        s.schedule();

        s.set_priority(b, 9).unwrap();
        assert!(s.need_resched());
        assert_eq!(
            s.schedule(),
            Decision::Switch {
                from: Some(a),
                to: b
            }
        );

        s.set_priority(b, 0).unwrap();
        assert!(s.need_resched());
        assert_eq!(
            s.schedule(),
            Decision::Switch {
                from: Some(b),
                to: a
            }
        );
    }

    #[test]
    fn info_reports_vcpu_kind_and_domain() {
        let mut s = Scheduler::<2>::new();
        let v = s.spawn(vcpu(3, 7, 0)).unwrap();
        let info = s.info(v).unwrap();
        assert_eq!(info.domain, LINUX);
        assert_eq!(info.kind, ExecKind::Vcpu { vcpu: 3 });
        assert_eq!(info.timeslice, 1);
        assert_eq!(info.state, ThreadState::Ready);
    }
}
