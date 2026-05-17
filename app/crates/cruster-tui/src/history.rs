//! Navigation history with recency × frequency ranking.

use std::collections::HashMap;

use cruster_core::ResourceKey;

#[derive(Debug, Clone)]
pub struct VisitedItem {
    pub view_id: String,
    pub key: Option<ResourceKey>,
    pub last_visited_step: u64,
    pub visit_count: u64,
}

#[derive(Default)]
pub struct History {
    items: HashMap<String, VisitedItem>,
    step: u64,
    cap: usize,
}

impl History {
    pub fn with_capacity(cap: usize) -> Self {
        Self {
            cap,
            ..Self::default()
        }
    }

    pub fn record(&mut self, view_id: &str, key: Option<ResourceKey>) {
        self.step += 1;
        let id = match &key {
            Some(k) => format!("{view_id}::{k}"),
            None => view_id.to_string(),
        };
        let entry = self.items.entry(id).or_insert_with(|| VisitedItem {
            view_id: view_id.into(),
            key: key.clone(),
            last_visited_step: 0,
            visit_count: 0,
        });
        entry.last_visited_step = self.step;
        entry.visit_count += 1;
        self.trim();
    }

    fn trim(&mut self) {
        if self.items.len() <= self.cap.max(1) {
            return;
        }
        let mut entries: Vec<(String, u64)> = self
            .items
            .iter()
            .map(|(k, v)| (k.clone(), v.last_visited_step))
            .collect();
        entries.sort_by_key(|a| a.1);
        let drop_n = self.items.len() - self.cap.max(1);
        for (k, _) in entries.into_iter().take(drop_n) {
            self.items.remove(&k);
        }
    }

    pub fn ranked(&self) -> Vec<&VisitedItem> {
        let mut v: Vec<&VisitedItem> = self.items.values().collect();
        v.sort_by(|a, b| {
            let sa = a.visit_count.saturating_mul(100) + a.last_visited_step;
            let sb = b.visit_count.saturating_mul(100) + b.last_visited_step;
            sb.cmp(&sa)
        });
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_increments_step_and_count() {
        let mut h = History::with_capacity(10);
        h.record("pods", None);
        h.record("pods", None);
        assert_eq!(h.items.len(), 1);
        assert_eq!(h.items.values().next().unwrap().visit_count, 2);
    }

    #[test]
    fn ranked_orders_by_frequency_then_recency() {
        let mut h = History::with_capacity(10);
        h.record("pods", None);
        h.record("services", None);
        h.record("services", None);
        let r = h.ranked();
        assert_eq!(r[0].view_id, "services");
        assert_eq!(r[1].view_id, "pods");
    }

    #[test]
    fn trim_drops_oldest_when_over_capacity() {
        let mut h = History::with_capacity(2);
        h.record("a", None);
        h.record("b", None);
        h.record("c", None);
        assert_eq!(h.items.len(), 2);
        assert!(h.items.values().all(|v| v.view_id != "a"));
    }
}
