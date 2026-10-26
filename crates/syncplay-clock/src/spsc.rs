//! Lock-free Single-Producer Single-Consumer (SPSC) circular ring buffer.
//!
//! Features:
//! - Power-of-two capacity for fast bitwise masking.
//! - Independent 128-byte cache-line alignment for head and tail to eliminate false sharing.
//! - Monotonic wrapping atomic counter arithmetic without mutexes or locks.
//! - Acquire-Release memory ordering semantics (no SeqCst).

use std::cell::UnsafeCell;
use std::mem::MaybeUninit;
use std::sync::atomic::{AtomicUsize, Ordering};
use crate::error::BufferFullError;

/// Wrapper aligning an inner type to a 128-byte boundary to prevent false sharing.
#[repr(align(128))]
pub struct CacheAligned<T>(pub T);

/// Lock-free Single-Producer Single-Consumer (SPSC) ring buffer.
pub struct SpscRingBuffer<T, const CAPACITY: usize> {
    head: CacheAligned<AtomicUsize>,
    tail: CacheAligned<AtomicUsize>,
    slots: Box<[UnsafeCell<MaybeUninit<T>>]>,
}

// SAFETY: SpscRingBuffer manages synchronized ownership transfer of `T: Send` items between threads.
unsafe impl<T: Send, const CAPACITY: usize> Send for SpscRingBuffer<T, CAPACITY> {}
// SAFETY: Head and tail are atomic, and slots are accessed strictly via SPSC invariant.
unsafe impl<T: Send, const CAPACITY: usize> Sync for SpscRingBuffer<T, CAPACITY> {}

impl<T, const CAPACITY: usize> SpscRingBuffer<T, CAPACITY> {
    /// Constructs a new `SpscRingBuffer`.
    ///
    /// # Panics
    /// Panics if `CAPACITY` is zero or not a power of two.
    pub fn new() -> Self {
        assert!(
            CAPACITY > 0 && CAPACITY.is_power_of_two(),
            "CAPACITY must be a power of two"
        );

        let mut slots_vec = Vec::with_capacity(CAPACITY);
        for _ in 0..CAPACITY {
            slots_vec.push(UnsafeCell::new(MaybeUninit::uninit()));
        }

        Self {
            head: CacheAligned(AtomicUsize::new(0)),
            tail: CacheAligned(AtomicUsize::new(0)),
            slots: slots_vec.into_boxed_slice(),
        }
    }

    /// Attempts to push an item into the ring buffer.
    ///
    /// Returns `Err(BufferFullError)` if the buffer is full without blocking.
    pub fn try_push(&self, item: T) -> Result<(), BufferFullError<T>> {
        let head = self.head.0.load(Ordering::Relaxed);
        let tail = self.tail.0.load(Ordering::Acquire);

        // Buffer full check: head.wrapping_sub(tail) == CAPACITY
        if head.wrapping_sub(tail) == CAPACITY {
            return Err(BufferFullError::new(item));
        }

        let slot_idx = head & (CAPACITY - 1);

        // SAFETY: Only the producer thread modifies the slot indexed by `head`.
        // The consumer cannot read this slot until `head` is published via store(Release).
        unsafe {
            let slot_ptr = self.slots[slot_idx].get();
            (*slot_ptr).write(item);
        }

        // Publish written slot to consumer
        self.head.0.store(head.wrapping_add(1), Ordering::Release);
        Ok(())
    }

    /// Attempts to pop an item from the ring buffer.
    ///
    /// Returns `None` if the buffer is empty without blocking.
    pub fn try_pop(&self) -> Option<T> {
        let tail = self.tail.0.load(Ordering::Relaxed);
        let head = self.head.0.load(Ordering::Acquire);

        // Buffer empty check: head == tail
        if head == tail {
            return None;
        }

        let slot_idx = tail & (CAPACITY - 1);

        // SAFETY: Only the consumer thread reads the slot indexed by `tail`.
        // The producer cannot overwrite this slot until `tail` is updated via store(Release).
        let item = unsafe {
            let slot_ptr = self.slots[slot_idx].get();
            (*slot_ptr).assume_init_read()
        };

        // Publish freed slot to producer
        self.tail.0.store(tail.wrapping_add(1), Ordering::Release);
        Some(item)
    }

    /// Returns the capacity of the buffer.
    #[inline]
    pub const fn capacity(&self) -> usize {
        CAPACITY
    }

    /// Returns the current number of occupied items.
    pub fn len(&self) -> usize {
        let head = self.head.0.load(Ordering::Relaxed);
        let tail = self.tail.0.load(Ordering::Acquire);
        head.wrapping_sub(tail)
    }

    /// Returns true if the buffer contains zero items.
    pub fn is_empty(&self) -> bool {
        self.head.0.load(Ordering::Relaxed) == self.tail.0.load(Ordering::Acquire)
    }

    /// Returns memory offset difference between head and tail to verify 128-byte cache-line separation.
    pub fn cache_line_distance(&self) -> usize {
        let head_addr = &self.head as *const _ as usize;
        let tail_addr = &self.tail as *const _ as usize;
        head_addr.abs_diff(tail_addr)
    }
}

impl<T, const CAPACITY: usize> Drop for SpscRingBuffer<T, CAPACITY> {
    fn drop(&mut self) {
        while self.try_pop().is_some() {}
    }
}

impl<T, const CAPACITY: usize> Default for SpscRingBuffer<T, CAPACITY> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spsc_capacity_and_fifo() {
        let buf = SpscRingBuffer::<i32, 64>::new();
        assert!(buf.is_empty());
        assert_eq!(buf.try_pop(), None);

        // Push 64 items
        for i in 0..64 {
            buf.try_push(i).unwrap();
        }
        assert_eq!(buf.len(), 64);

        // 65th push fails
        let err = buf.try_push(999);
        assert!(err.is_err());
        assert_eq!(err.unwrap_err().item, 999);

        // Pop all 64 items
        for i in 0..64 {
            assert_eq!(buf.try_pop(), Some(i));
        }
        assert!(buf.is_empty());
        assert_eq!(buf.try_pop(), None);
    }

    #[test]
    fn test_cache_line_alignment() {
        let buf = SpscRingBuffer::<usize, 128>::new();
        assert!(
            buf.cache_line_distance() >= 128,
            "Head and tail must be on separate 128-byte cache lines, distance is {}",
            buf.cache_line_distance()
        );
    }

    #[test]
    fn test_concurrent_spsc() {
        use std::sync::Arc;
        use std::thread;

        let buf = Arc::new(SpscRingBuffer::<usize, 128>::new());
        let buf_producer = Arc::clone(&buf);
        let total_items = 50_000usize;

        let producer = thread::spawn(move || {
            for i in 0..total_items {
                let mut val = i;
                loop {
                    match buf_producer.try_push(val) {
                        Ok(()) => break,
                        Err(e) => {
                            val = e.item;
                            std::hint::spin_loop();
                        }
                    }
                }
            }
        });

        let consumer = thread::spawn(move || {
            let mut received = Vec::with_capacity(total_items);
            while received.len() < total_items {
                if let Some(item) = buf.try_pop() {
                    received.push(item);
                } else {
                    std::hint::spin_loop();
                }
            }
            received
        });

        producer.join().unwrap();
        let received = consumer.join().unwrap();
        assert_eq!(received.len(), total_items);
        for (idx, &item) in received.iter().enumerate() {
            assert_eq!(item, idx);
        }
    }
}
