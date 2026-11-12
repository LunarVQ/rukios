use crate::thread::{Priority, Slot, NIL, NUM_PRIORITIES};

const _: () = assert!(NUM_PRIORITIES <= 64);

/// Intrusive FIFO per priority level plus a bitmap of non-empty levels.
pub(crate) struct ReadyQueues {
    heads: [u16; NUM_PRIORITIES],
    tails: [u16; NUM_PRIORITIES],
    bitmap: u64,
}

impl ReadyQueues {
    pub const fn new() -> Self {
        Self {
            heads: [NIL; NUM_PRIORITIES],
            tails: [NIL; NUM_PRIORITIES],
            bitmap: 0,
        }
    }

    pub fn highest(&self) -> Option<Priority> {
        if self.bitmap == 0 {
            None
        } else {
            Some((63 - self.bitmap.leading_zeros()) as Priority)
        }
    }

    pub fn push_back(&mut self, slots: &mut [Slot], i: u16) {
        let p = slots[i as usize].priority as usize;
        let tail = self.tails[p];
        slots[i as usize].prev = tail;
        slots[i as usize].next = NIL;
        if tail == NIL {
            self.heads[p] = i;
            self.bitmap |= 1 << p;
        } else {
            slots[tail as usize].next = i;
        }
        self.tails[p] = i;
    }

    pub fn push_front(&mut self, slots: &mut [Slot], i: u16) {
        let p = slots[i as usize].priority as usize;
        let head = self.heads[p];
        slots[i as usize].prev = NIL;
        slots[i as usize].next = head;
        if head == NIL {
            self.tails[p] = i;
            self.bitmap |= 1 << p;
        } else {
            slots[head as usize].prev = i;
        }
        self.heads[p] = i;
    }

    pub fn remove(&mut self, slots: &mut [Slot], i: u16) {
        let p = slots[i as usize].priority as usize;
        let Slot { prev, next, .. } = slots[i as usize];
        if prev == NIL {
            self.heads[p] = next;
        } else {
            slots[prev as usize].next = next;
        }
        if next == NIL {
            self.tails[p] = prev;
        } else {
            slots[next as usize].prev = prev;
        }
        if self.heads[p] == NIL {
            self.bitmap &= !(1 << p);
        }
        slots[i as usize].next = NIL;
        slots[i as usize].prev = NIL;
    }

    pub fn pop_highest(&mut self, slots: &mut [Slot]) -> Option<u16> {
        let p = self.highest()? as usize;
        let i = self.heads[p];
        self.remove(slots, i);
        Some(i)
    }
}
