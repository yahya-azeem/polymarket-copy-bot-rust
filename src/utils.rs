use std::collections::{HashSet, VecDeque};
use std::time::{SystemTime, UNIX_EPOCH};

pub fn current_time_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

/// A bounded set that keeps the most recent `capacity` entries.
/// When full, evicts the oldest entry (FIFO) to prevent unbounded memory growth.
pub struct BoundedDedup {
    set: HashSet<String>,
    order: VecDeque<String>,
    capacity: usize,
}

impl BoundedDedup {
    pub fn new(capacity: usize) -> Self {
        Self {
            set: HashSet::with_capacity(capacity),
            order: VecDeque::with_capacity(capacity),
            capacity,
        }
    }

    pub fn contains(&self, key: &str) -> bool {
        self.set.contains(key)
    }

    /// Inserts a key. Returns true if newly inserted, false if already present.
    pub fn insert(&mut self, key: String) -> bool {
        if self.set.contains(&key) {
            return false;
        }
        while self.order.len() >= self.capacity {
            if let Some(evicted) = self.order.pop_front() {
                self.set.remove(&evicted);
            }
        }
        self.set.insert(key.clone());
        self.order.push_back(key);
        true
    }
}
