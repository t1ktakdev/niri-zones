use std::collections::HashMap;

use crate::{Rect, WindowId, WindowSnapshot, ZoneId};

#[derive(Debug, Clone, PartialEq)]
pub struct SnapRecord {
    pub baseline: WindowSnapshot,
    pub current_zone: ZoneId,
    pub previous_zone: Option<ZoneId>,
    pub applied_geometry: Rect,
}

#[derive(Debug, Default)]
pub struct SnapStateStore {
    records: HashMap<WindowId, SnapRecord>,
}

impl SnapStateStore {
    pub fn get(&self, id: WindowId) -> Option<&SnapRecord> {
        self.records.get(&id)
    }

    pub fn record_success(&mut self, before: WindowSnapshot, zone: ZoneId, applied_geometry: Rect) {
        match self.records.get_mut(&before.id) {
            Some(record) => {
                if record.current_zone != zone {
                    record.previous_zone = Some(record.current_zone.clone());
                    record.current_zone = zone;
                }
                record.applied_geometry = applied_geometry;
            }
            None => {
                self.records.insert(
                    before.id,
                    SnapRecord {
                        baseline: before,
                        current_zone: zone,
                        previous_zone: None,
                        applied_geometry,
                    },
                );
            }
        }
    }

    pub fn remove(&mut self, id: WindowId) -> Option<SnapRecord> {
        self.records.remove(&id)
    }

    pub fn clear_if_manual_change(&mut self, id: WindowId, actual: Rect, tolerance: f64) -> bool {
        let changed = self
            .records
            .get(&id)
            .is_some_and(|record| !record.applied_geometry.approx_eq(actual, tolerance));
        if changed {
            self.records.remove(&id);
        }
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moving_between_zones_keeps_original_baseline() {
        let baseline = WindowSnapshot {
            id: 7,
            geometry: Rect::new(10.0, 20.0, 500.0, 400.0).unwrap(),
            is_floating: true,
        };
        let mut store = SnapStateStore::default();
        store.record_success(
            baseline.clone(),
            ZoneId::from("1"),
            Rect::new(0.0, 0.0, 800.0, 900.0).unwrap(),
        );
        store.record_success(
            WindowSnapshot {
                id: 7,
                geometry: Rect::new(0.0, 0.0, 800.0, 900.0).unwrap(),
                is_floating: true,
            },
            ZoneId::from("2"),
            Rect::new(800.0, 0.0, 800.0, 900.0).unwrap(),
        );
        let record = store.get(7).unwrap();
        assert_eq!(record.baseline, baseline);
        assert_eq!(record.previous_zone.as_ref().unwrap().0, "1");
    }
}
