use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Repeat {
    Off,
    All,
    One,
}

/// Play order over a list of items, with shuffle and repeat. Knows nothing about audio.
pub struct Queue<T> {
    /// Items in their original (unshuffled) order.
    source: Vec<T>,
    /// Play order: indices into `source`.
    order: Vec<usize>,
    /// Position within `order`.
    cursor: Option<usize>,
    pub shuffle: bool,
    pub repeat: Repeat,
    rng: u64,
}

/// A queue as saved between launches.
#[derive(Serialize, Deserialize)]
pub struct Saved<T> {
    source: Vec<T>,
    order: Vec<usize>,
    cursor: Option<usize>,
    shuffle: bool,
    repeat: Repeat,
}

/// What to do after a track ends or the user skips.
#[derive(Debug, PartialEq, Eq)]
pub enum Advance {
    Play,
    Stop,
}

impl<T: Clone> Queue<T> {
    pub fn new() -> Self {
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0x9e37_79b9, |d| d.as_nanos() as u64);
        Self { source: Vec::new(), order: Vec::new(), cursor: None, shuffle: false, repeat: Repeat::Off, rng: seed | 1 }
    }

    pub fn save(&self) -> Saved<T> {
        Saved {
            source: self.source.clone(),
            order: self.order.clone(),
            cursor: self.cursor,
            shuffle: self.shuffle,
            repeat: self.repeat,
        }
    }

    /// Restores a saved queue. One that doesn't hold together (a hand-edited or truncated
    /// file) is refused and the queue is left as it was.
    pub fn restore(&mut self, saved: Saved<T>) -> bool {
        let n = saved.source.len();
        let mut seen = vec![false; n];
        let permutation =
            saved.order.len() == n && saved.order.iter().all(|&i| i < n && !std::mem::replace(&mut seen[i], true));
        if n == 0 || !permutation || saved.cursor.is_some_and(|c| c >= n) {
            return false;
        }
        self.source = saved.source;
        self.order = saved.order;
        self.cursor = saved.cursor;
        self.shuffle = saved.shuffle;
        self.repeat = saved.repeat;
        true
    }

    pub fn current(&self) -> Option<&T> {
        self.cursor.map(|c| &self.source[self.order[c]])
    }

    pub fn set(&mut self, items: Vec<T>, start: usize, shuffle: bool) {
        if items.is_empty() {
            return;
        }
        let start = start.min(items.len() - 1);
        self.source = items;
        self.shuffle = shuffle;
        self.order = (0..self.source.len()).collect();
        if shuffle {
            self.shuffle_order(start);
        } else {
            self.cursor = Some(start);
        }
    }

    /// Inserts `item` right after the current one (or starts a queue with it).
    pub fn play_next(&mut self, item: T) {
        self.source.push(item);
        let ix = self.source.len() - 1;
        match self.cursor {
            Some(c) => self.order.insert(c + 1, ix),
            None => {
                self.order.push(ix);
                self.cursor = Some(self.order.len() - 1);
            }
        }
    }

    /// What `next()` would play, without moving. `None` at the end of the queue (unless
    /// repeating everything) and under repeat-one, where the same song plays again.
    pub fn peek_next(&self) -> Option<&T> {
        let cursor = self.cursor?;
        match self.repeat {
            Repeat::One => None,
            _ if cursor + 1 < self.order.len() => Some(&self.source[self.order[cursor + 1]]),
            Repeat::All => Some(&self.source[self.order[0]]),
            Repeat::Off => None,
        }
    }

    pub fn up_next(&self) -> Vec<T> {
        let start = self.cursor.map_or(0, |c| c + 1);
        self.order[start.min(self.order.len())..].iter().map(|&i| self.source[i].clone()).collect()
    }

    pub fn jump_ahead(&mut self, n: usize) -> bool {
        match self.cursor {
            Some(c) if c + n + 1 < self.order.len() => {
                self.cursor = Some(c + n + 1);
                true
            }
            _ => false,
        }
    }

    pub fn toggle_shuffle(&mut self) {
        self.shuffle = !self.shuffle;
        let Some(cursor) = self.cursor else {
            return;
        };
        let current = self.order[cursor];
        self.order = (0..self.source.len()).collect();
        if self.shuffle {
            self.shuffle_order(current);
        } else {
            self.cursor = Some(current);
        }
    }

    pub fn cycle_repeat(&mut self) {
        self.repeat = match self.repeat {
            Repeat::Off => Repeat::All,
            Repeat::All => Repeat::One,
            Repeat::One => Repeat::Off,
        };
    }

    /// Called when a track finishes on its own; honors repeat-one.
    pub fn track_ended(&mut self) -> Advance {
        if self.repeat == Repeat::One {
            return Advance::Play;
        }
        self.next()
    }

    pub fn next(&mut self) -> Advance {
        let Some(cursor) = self.cursor else {
            return Advance::Stop;
        };
        if cursor + 1 < self.order.len() {
            self.cursor = Some(cursor + 1);
            Advance::Play
        } else {
            self.cursor = Some(0);
            if self.repeat == Repeat::All { Advance::Play } else { Advance::Stop }
        }
    }

    /// Like the Music app: restarts the song if more than 3s in, otherwise goes back one.
    /// Returns true if the cursor moved.
    pub fn previous(&mut self, position_secs: f32) -> bool {
        match self.cursor {
            Some(c) if position_secs <= 3.0 && c > 0 => {
                self.cursor = Some(c - 1);
                true
            }
            _ => false,
        }
    }

    /// Fisher-Yates over everything except `first`, which is placed at the front.
    fn shuffle_order(&mut self, first: usize) {
        let mut rest: Vec<usize> = (0..self.source.len()).filter(|&i| i != first).collect();
        for i in (1..rest.len()).rev() {
            let j = (self.next_rand() % (i as u64 + 1)) as usize;
            rest.swap(i, j);
        }
        self.order = std::iter::once(first).chain(rest).collect();
        self.cursor = Some(0);
    }

    fn next_rand(&mut self) -> u64 {
        // xorshift64
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        self.rng
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn queue(n: usize, start: usize) -> Queue<usize> {
        let mut q = Queue::new();
        q.set((0..n).collect(), start, false);
        q
    }

    #[test]
    fn save_and_restore_keep_order_and_position() {
        let mut q = queue(5, 2);
        q.toggle_shuffle();
        q.cycle_repeat();
        let (current, up_next) = (q.current().copied(), q.up_next());
        let json = serde_json::to_string(&q.save()).unwrap();
        let mut restored = Queue::new();
        assert!(restored.restore(serde_json::from_str(&json).unwrap()));
        assert_eq!(restored.current().copied(), current);
        assert_eq!(restored.up_next(), up_next);
        assert_eq!((restored.shuffle, restored.repeat), (q.shuffle, q.repeat));
    }

    #[test]
    fn restore_refuses_a_broken_queue() {
        let mut q = queue(3, 1);
        for bad in [
            r#"{"source":[0,1,2],"order":[0,1],"cursor":0,"shuffle":false,"repeat":"Off"}"#,
            r#"{"source":[0,1,2],"order":[0,1,1],"cursor":0,"shuffle":false,"repeat":"Off"}"#,
            r#"{"source":[0,1,2],"order":[0,1,2],"cursor":3,"shuffle":false,"repeat":"Off"}"#,
            r#"{"source":[],"order":[],"cursor":null,"shuffle":false,"repeat":"Off"}"#,
        ] {
            assert!(!q.restore(serde_json::from_str(bad).unwrap()), "{bad}");
        }
        assert_eq!(q.current(), Some(&1));
    }

    #[test]
    fn plays_through_and_stops_at_end() {
        let mut q = queue(2, 0);
        assert_eq!(q.track_ended(), Advance::Play);
        assert_eq!(q.current(), Some(&1));
        assert_eq!(q.track_ended(), Advance::Stop);
        assert_eq!(q.current(), Some(&0));
    }

    #[test]
    fn repeat_all_wraps_and_repeat_one_stays() {
        let mut q = queue(2, 1);
        q.repeat = Repeat::All;
        assert_eq!(q.next(), Advance::Play);
        assert_eq!(q.current(), Some(&0));
        q.repeat = Repeat::One;
        assert_eq!(q.track_ended(), Advance::Play);
        assert_eq!(q.current(), Some(&0));
        // An explicit skip still moves on under repeat-one.
        assert_eq!(q.next(), Advance::Play);
        assert_eq!(q.current(), Some(&1));
    }

    #[test]
    fn shuffle_keeps_current_and_covers_all() {
        let mut q = queue(10, 4);
        q.toggle_shuffle();
        assert_eq!(q.current(), Some(&4));
        let mut seen = q.up_next();
        seen.push(4);
        seen.sort();
        assert_eq!(seen, (0..10).collect::<Vec<_>>());
        q.toggle_shuffle();
        assert_eq!(q.current(), Some(&4));
        assert_eq!(q.up_next().len(), 5);
    }

    #[test]
    fn previous_restarts_after_three_seconds() {
        let mut q = queue(3, 1);
        assert!(!q.previous(10.0));
        assert_eq!(q.current(), Some(&1));
        assert!(q.previous(1.0));
        assert_eq!(q.current(), Some(&0));
    }

    #[test]
    fn peek_next_respects_repeat() {
        let mut q = queue(2, 0);
        assert_eq!(q.peek_next(), Some(&1));
        q.next();
        assert_eq!(q.peek_next(), None);
        q.repeat = Repeat::All;
        assert_eq!(q.peek_next(), Some(&0));
        q.repeat = Repeat::One;
        assert_eq!(q.peek_next(), None);
    }

    #[test]
    fn play_next_inserts_after_current() {
        let mut q = queue(3, 0);
        q.play_next(9);
        assert_eq!(q.up_next(), vec![9, 1, 2]);
        let mut empty = Queue::new();
        empty.play_next(5);
        assert_eq!(empty.current(), Some(&5));
    }
}
