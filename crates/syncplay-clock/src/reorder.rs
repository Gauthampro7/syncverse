//! Bounded presentation reorder buffer for ordering decode-order frames by ascending presentation timestamps.

use std::collections::BinaryHeap;

#[derive(Debug)]
struct ReorderEntry<T> {
    pts_ms: u64,
    item: T,
}

impl<T> PartialEq for ReorderEntry<T> {
    fn eq(&self, other: &Self) -> bool {
        self.pts_ms == other.pts_ms
    }
}

impl<T> Eq for ReorderEntry<T> {}

impl<T> PartialOrd for ReorderEntry<T> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl<T> Ord for ReorderEntry<T> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // Reverse for min-heap ordering by PTS
        other.pts_ms.cmp(&self.pts_ms)
    }
}

/// Bounded reorder buffer (min-heap) ordering frames by ascending PTS.
pub struct PresentationReorderBuffer<T> {
    capacity: usize,
    heap: BinaryHeap<ReorderEntry<T>>,
}

impl<T> PresentationReorderBuffer<T> {
    /// Constructs a new `PresentationReorderBuffer` with bounded capacity $K$ (e.g. 16 frames).
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            heap: BinaryHeap::with_capacity(capacity + 1),
        }
    }

    /// Pushes a frame with its presentation timestamp into the reorder buffer.
    ///
    /// If the buffer reaches or exceeds its capacity, the frame with the minimum PTS
    /// is popped and returned to yield to the presentation scheduler.
    pub fn push(&mut self, pts_ms: u64, item: T) -> Option<(u64, T)> {
        self.heap.push(ReorderEntry { pts_ms, item });
        if self.heap.len() > self.capacity {
            self.pop_min()
        } else {
            None
        }
    }

    /// Pops the frame having the minimum presentation timestamp.
    pub fn pop_min(&mut self) -> Option<(u64, T)> {
        self.heap.pop().map(|e| (e.pts_ms, e.item))
    }

    /// Drains all remaining frames in ascending PTS order (used on container stream drain or seek).
    pub fn drain_all(&mut self) -> Vec<(u64, T)> {
        let mut results = Vec::with_capacity(self.heap.len());
        while let Some(item) = self.pop_min() {
            results.push(item);
        }
        results
    }

    /// Returns the number of frames currently in the buffer.
    #[inline]
    pub fn len(&self) -> usize {
        self.heap.len()
    }

    /// Returns true if the buffer contains zero frames.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.heap.is_empty()
    }

    /// Returns true if the buffer has reached its capacity.
    #[inline]
    pub fn is_full(&self) -> bool {
        self.heap.len() >= self.capacity
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reorder_buffer_sorting() {
        let mut buf = PresentationReorderBuffer::<&'static str>::new(4);
        // Push frames out of presentation order: PTS 40, 20, 10, 30
        buf.push(40, "frame 40");
        buf.push(20, "frame 20");
        buf.push(10, "frame 10");
        assert_eq!(buf.len(), 3);

        // 4th frame (exceeds capacity 4 on next push)
        let ejected = buf.push(30, "frame 30");
        assert!(ejected.is_none());

        // 5th frame causes min (10) to be ejected
        let (pts, frame) = buf.push(50, "frame 50").unwrap();
        assert_eq!(pts, 10);
        assert_eq!(frame, "frame 10");

        // Drain remaining
        let remaining = buf.drain_all();
        let pts_list: Vec<u64> = remaining.iter().map(|(p, _)| *p).collect();
        assert_eq!(pts_list, vec![20, 30, 40, 50]);
    }

    #[test]
    fn test_bounded_16_frame_reorder_buffer() {
        let mut buf = PresentationReorderBuffer::<u32>::new(16);
        assert_eq!(buf.len(), 0);
        assert!(buf.is_empty());
        assert!(!buf.is_full());

        // Push 16 frames in reverse order (PTS 160 down to 10)
        for i in (1..=16).rev() {
            let pts = (i * 10) as u64;
            let val = i as u32;
            let ejected = buf.push(pts, val);
            assert!(ejected.is_none(), "Should not eject when within capacity 16");
        }
        assert_eq!(buf.len(), 16);
        assert!(buf.is_full());

        // Push 17th frame (PTS 170) -> lowest PTS (10) must be popped
        let (popped_pts, popped_val) = buf.push(170, 17).expect("Capacity 16 exceeded, must eject min");
        assert_eq!(popped_pts, 10);
        assert_eq!(popped_val, 1);
        assert_eq!(buf.len(), 16);

        // Drain all remaining 16 frames: must be strictly ascending (20, 30, ..., 170)
        let drained = buf.drain_all();
        assert_eq!(drained.len(), 16);
        assert!(buf.is_empty());

        let expected_pts: Vec<u64> = (2..=17).map(|i| (i * 10) as u64).collect();
        let actual_pts: Vec<u64> = drained.iter().map(|(pts, _)| *pts).collect();
        assert_eq!(actual_pts, expected_pts);
    }
}
