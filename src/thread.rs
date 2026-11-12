pub const NUM_PRIORITIES: usize = 64;

/// Higher is more urgent.
pub type Priority = u8;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ThreadId {
    index: u16,
    generation: u16,
}

impl ThreadId {
    pub(crate) const fn new(index: u16, generation: u16) -> Self {
        Self { index, generation }
    }

    pub const fn index(self) -> usize {
        self.index as usize
    }

    pub const fn generation(self) -> u16 {
        self.generation
    }
}

/// A native task or an entire guest OS.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DomainId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExecKind {
    Native,
    Vcpu { vcpu: u16 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThreadState {
    Ready,
    Running,
    Blocked,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ThreadParams {
    pub domain: DomainId,
    pub kind: ExecKind,
    pub priority: Priority,
    /// In timer ticks.
    pub timeslice: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ThreadInfo {
    pub id: ThreadId,
    pub domain: DomainId,
    pub kind: ExecKind,
    pub state: ThreadState,
    pub priority: Priority,
    pub timeslice: u32,
    pub remaining: u32,
}

pub(crate) const NIL: u16 = u16::MAX;

/// `state == None` means free; `next` then links the free list.
#[derive(Clone, Copy)]
pub(crate) struct Slot {
    pub generation: u16,
    pub state: Option<ThreadState>,
    pub domain: DomainId,
    pub kind: ExecKind,
    pub priority: Priority,
    pub timeslice: u32,
    pub remaining: u32,
    pub next: u16,
    pub prev: u16,
}

impl Slot {
    pub const FREE: Slot = Slot {
        generation: 0,
        state: None,
        domain: DomainId(0),
        kind: ExecKind::Native,
        priority: 0,
        timeslice: 0,
        remaining: 0,
        next: NIL,
        prev: NIL,
    };
}
