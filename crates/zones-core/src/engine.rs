use thiserror::Error;

use crate::{BackendError, Rect, SnapStateStore, WindowBackend, WindowId, WindowSnapshot, ZoneId};

#[derive(Debug, Clone, PartialEq)]
pub enum SnapOutcome {
    Applied { actual: Rect },
    /// The backend kept the requested position but adjusted the size. This commonly
    /// represents application/compositor size constraints rather than a failed move.
    AppliedAdjusted { requested: Rect, actual: Rect },
    Noop { actual: Rect },
    Restored { actual: WindowSnapshot },
}

#[derive(Debug, Error)]
pub enum EngineError {
    #[error(transparent)]
    Backend(#[from] BackendError),
    #[error("window {0} is tiled; pass an explicit allow-float policy before snapping it")]
    RequiresFloating(WindowId),
    #[error("backend applied geometry {actual:?}, expected {expected:?} within tolerance {tolerance}")]
    VerificationMismatch { expected: Rect, actual: Rect, tolerance: f64 },
    #[error("snap failed: {cause}; rollback also failed: {rollback}")]
    Degraded { cause: String, rollback: String },
    #[error("window {0} has no snap state to restore")]
    NothingToRestore(WindowId),
}

pub struct SnapEngine<B> {
    backend: B,
    state: SnapStateStore,
    tolerance: f64,
}

impl<B: WindowBackend> SnapEngine<B> {
    pub fn new(backend: B, tolerance: f64) -> Self {
        Self { backend, state: SnapStateStore::default(), tolerance }
    }

    pub fn backend(&self) -> &B {
        &self.backend
    }

    pub fn backend_mut(&mut self) -> &mut B {
        &mut self.backend
    }

    pub fn state(&self) -> &SnapStateStore {
        &self.state
    }

    pub fn snap(&mut self, id: WindowId, zone: ZoneId, target: Rect, allow_float: bool) -> Result<SnapOutcome, EngineError> {
        let before = self.backend.window(id)?;
        if before.geometry.approx_eq(target, self.tolerance) && before.is_floating {
            return Ok(SnapOutcome::Noop { actual: before.geometry });
        }
        if !before.is_floating && !allow_float {
            return Err(EngineError::RequiresFloating(id));
        }

        if !before.is_floating {
            self.backend.set_floating(id, true)?;
        }

        if let Err(error) = self.backend.resize(id, target.width, target.height) {
            return Err(self.rollback_error(&before, error));
        }
        if let Err(error) = self.backend.move_window(id, target.x, target.y) {
            return Err(self.rollback_error(&before, error));
        }

        let actual = match self.backend.window(id) {
            Ok(snapshot) => snapshot,
            Err(error) => return Err(self.rollback_error(&before, error)),
        };
        if !actual.geometry.position_approx_eq(target, self.tolerance) {
            let mismatch = EngineError::VerificationMismatch { expected: target, actual: actual.geometry, tolerance: self.tolerance };
            return match self.rollback(&before) {
                Ok(()) => Err(mismatch),
                Err(rollback) => Err(EngineError::Degraded { cause: mismatch.to_string(), rollback: rollback.to_string() }),
            };
        }

        let size_adjusted = !actual.geometry.size_approx_eq(target, self.tolerance);
        self.state.record_success(before, zone, actual.geometry);
        if size_adjusted {
            Ok(SnapOutcome::AppliedAdjusted { requested: target, actual: actual.geometry })
        } else {
            Ok(SnapOutcome::Applied { actual: actual.geometry })
        }
    }

    pub fn restore(&mut self, id: WindowId) -> Result<SnapOutcome, EngineError> {
        let record = self.state.get(id).cloned().ok_or(EngineError::NothingToRestore(id))?;
        let current = self.backend.window(id)?;

        if record.baseline.is_floating {
            self.backend.resize(id, record.baseline.geometry.width, record.baseline.geometry.height)?;
            self.backend.move_window(id, record.baseline.geometry.x, record.baseline.geometry.y)?;
            let actual = self.backend.window(id)?;
            if !actual.geometry.approx_eq(record.baseline.geometry, self.tolerance) {
                return Err(EngineError::VerificationMismatch {
                    expected: record.baseline.geometry,
                    actual: actual.geometry,
                    tolerance: self.tolerance,
                });
            }
        } else if current.is_floating {
            self.backend.set_floating(id, false)?;
        }

        let actual = self.backend.window(id)?;
        self.state.remove(id);
        Ok(SnapOutcome::Restored { actual })
    }

    fn rollback_error(&mut self, before: &WindowSnapshot, cause: BackendError) -> EngineError {
        match self.rollback(before) {
            Ok(()) => EngineError::Backend(cause),
            Err(rollback) => EngineError::Degraded { cause: cause.to_string(), rollback: rollback.to_string() },
        }
    }

    fn rollback(&mut self, before: &WindowSnapshot) -> Result<(), BackendError> {
        if before.is_floating {
            self.backend.resize(before.id, before.geometry.width, before.geometry.height)?;
            self.backend.move_window(before.id, before.geometry.x, before.geometry.y)?;
        } else {
            self.backend.set_floating(before.id, false)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::{BackendOp, MockBackend, MockFailure};

    use super::*;

    fn floating() -> WindowSnapshot {
        WindowSnapshot { id: 1, geometry: Rect::new(10.0, 20.0, 400.0, 300.0).unwrap(), is_floating: true }
    }

    #[test]
    fn repeated_snap_is_idempotent() {
        let target = Rect::new(0.0, 0.0, 800.0, 900.0).unwrap();
        let backend = MockBackend::with_window(WindowSnapshot { id: 1, geometry: target, is_floating: true });
        let mut engine = SnapEngine::new(backend, 1.0);
        let outcome = engine.snap(1, ZoneId::from("1"), target, false).unwrap();
        assert!(matches!(outcome, SnapOutcome::Noop { .. }));
        assert_eq!(engine.backend().operations, vec![BackendOp::Read(1)]);
    }

    #[test]
    fn move_failure_rolls_back_resize_and_does_not_commit_state() {
        let original = floating();
        let backend = MockBackend::with_window(original.clone());
        let mut engine = SnapEngine::new(backend, 1.0);
        engine.backend_mut().fail_next = Some(MockFailure::Move);
        let target = Rect::new(500.0, 0.0, 900.0, 700.0).unwrap();
        assert!(engine.snap(1, ZoneId::from("2"), target, false).is_err());
        assert_eq!(engine.backend().snapshot(1).unwrap().geometry, original.geometry);
        assert!(engine.state().get(1).is_none());
    }

    #[test]
    fn tiled_window_requires_explicit_conversion() {
        let backend = MockBackend::with_window(WindowSnapshot {
            id: 1,
            geometry: Rect::new(0.0, 0.0, 600.0, 800.0).unwrap(),
            is_floating: false,
        });
        let mut engine = SnapEngine::new(backend, 1.0);
        let error = engine.snap(1, ZoneId::from("1"), Rect::new(0.0, 0.0, 500.0, 500.0).unwrap(), false).unwrap_err();
        assert!(matches!(error, EngineError::RequiresFloating(1)));
    }


    #[test]
    fn size_constraint_is_recorded_as_adjusted_instead_of_rolled_back() {
        let backend = MockBackend::with_window(floating());
        let mut engine = SnapEngine::new(backend, 1.0);
        engine.backend_mut().minimum_size = Some(crate::Size { width: 700.0, height: 500.0 });
        let target = Rect::new(0.0, 0.0, 400.0, 300.0).unwrap();
        let outcome = engine.snap(1, ZoneId::from("small"), target, false).unwrap();
        assert!(matches!(outcome, SnapOutcome::AppliedAdjusted { .. }));
        let record = engine.state().get(1).unwrap();
        assert_eq!(record.applied_geometry.width, 700.0);
        assert_eq!(record.applied_geometry.height, 500.0);
    }

    #[test]
    fn resize_failure_never_commits_snap_state() {
        let original = floating();
        let backend = MockBackend::with_window(original.clone());
        let mut engine = SnapEngine::new(backend, 1.0);
        engine.backend_mut().fail_next = Some(MockFailure::Resize);
        let target = Rect::new(500.0, 0.0, 900.0, 700.0).unwrap();
        assert!(engine.snap(1, ZoneId::from("2"), target, false).is_err());
        assert_eq!(engine.backend().snapshot(1).unwrap(), &original);
        assert!(engine.state().get(1).is_none());
    }

    #[test]
    fn failed_snap_after_explicit_float_restores_tiled_state() {
        let original = WindowSnapshot {
            id: 1,
            geometry: Rect::new(0.0, 0.0, 600.0, 800.0).unwrap(),
            is_floating: false,
        };
        let backend = MockBackend::with_window(original.clone());
        let mut engine = SnapEngine::new(backend, 1.0);
        engine.backend_mut().fail_next = Some(MockFailure::Resize);
        let target = Rect::new(10.0, 10.0, 500.0, 500.0).unwrap();
        assert!(engine.snap(1, ZoneId::from("1"), target, true).is_err());
        assert_eq!(engine.backend().snapshot(1).unwrap(), &original);
        assert!(engine.state().get(1).is_none());
    }

    #[test]
    fn restore_returns_original_floating_geometry() {
        let original = floating();
        let backend = MockBackend::with_window(original.clone());
        let mut engine = SnapEngine::new(backend, 1.0);
        let target = Rect::new(500.0, 0.0, 900.0, 700.0).unwrap();
        engine.snap(1, ZoneId::from("2"), target, false).unwrap();
        engine.restore(1).unwrap();
        assert_eq!(engine.backend().snapshot(1).unwrap(), &original);
        assert!(engine.state().get(1).is_none());
    }
}
