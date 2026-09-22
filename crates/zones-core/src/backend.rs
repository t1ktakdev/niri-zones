use std::collections::HashMap;

use thiserror::Error;

use crate::{Rect, Size};

pub type WindowId = u64;

#[derive(Debug, Clone, PartialEq)]
pub struct WindowSnapshot {
    pub id: WindowId,
    pub geometry: Rect,
    pub is_floating: bool,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
#[error("{message}")]
pub struct BackendError {
    pub message: String,
}

impl BackendError {
    pub fn new(message: impl Into<String>) -> Self {
        Self { message: message.into() }
    }
}

pub trait WindowBackend {
    fn window(&mut self, id: WindowId) -> Result<WindowSnapshot, BackendError>;
    fn set_floating(&mut self, id: WindowId, floating: bool) -> Result<(), BackendError>;
    fn resize(&mut self, id: WindowId, width: f64, height: f64) -> Result<(), BackendError>;
    fn move_window(&mut self, id: WindowId, x: f64, y: f64) -> Result<(), BackendError>;
}

#[derive(Debug, Clone, PartialEq)]
pub enum BackendOp {
    Read(WindowId),
    SetFloating(WindowId, bool),
    Resize(WindowId, f64, f64),
    Move(WindowId, f64, f64),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MockFailure {
    Read,
    SetFloating,
    Resize,
    Move,
}

#[derive(Debug, Default)]
pub struct MockBackend {
    windows: HashMap<WindowId, WindowSnapshot>,
    pub operations: Vec<BackendOp>,
    pub fail_next: Option<MockFailure>,
    pub minimum_size: Option<Size>,
}

impl MockBackend {
    pub fn with_window(window: WindowSnapshot) -> Self {
        let mut backend = Self::default();
        backend.windows.insert(window.id, window);
        backend
    }

    pub fn snapshot(&self, id: WindowId) -> Option<&WindowSnapshot> {
        self.windows.get(&id)
    }

    fn maybe_fail(&mut self, failure: MockFailure) -> Result<(), BackendError> {
        if self.fail_next == Some(failure) {
            self.fail_next = None;
            return Err(BackendError::new(format!("mock {failure:?} failure")));
        }
        Ok(())
    }
}

impl WindowBackend for MockBackend {
    fn window(&mut self, id: WindowId) -> Result<WindowSnapshot, BackendError> {
        self.operations.push(BackendOp::Read(id));
        self.maybe_fail(MockFailure::Read)?;
        self.windows.get(&id).cloned().ok_or_else(|| BackendError::new(format!("window {id} does not exist")))
    }

    fn set_floating(&mut self, id: WindowId, floating: bool) -> Result<(), BackendError> {
        self.operations.push(BackendOp::SetFloating(id, floating));
        self.maybe_fail(MockFailure::SetFloating)?;
        self.windows.get_mut(&id).ok_or_else(|| BackendError::new(format!("window {id} does not exist")))?.is_floating = floating;
        Ok(())
    }

    fn resize(&mut self, id: WindowId, width: f64, height: f64) -> Result<(), BackendError> {
        self.operations.push(BackendOp::Resize(id, width, height));
        self.maybe_fail(MockFailure::Resize)?;
        let minimum = self.minimum_size.unwrap_or(Size { width: 0.0, height: 0.0 });
        let window = self.windows.get_mut(&id).ok_or_else(|| BackendError::new(format!("window {id} does not exist")))?;
        window.geometry.width = width.max(minimum.width);
        window.geometry.height = height.max(minimum.height);
        Ok(())
    }

    fn move_window(&mut self, id: WindowId, x: f64, y: f64) -> Result<(), BackendError> {
        self.operations.push(BackendOp::Move(id, x, y));
        self.maybe_fail(MockFailure::Move)?;
        let window = self.windows.get_mut(&id).ok_or_else(|| BackendError::new(format!("window {id} does not exist")))?;
        window.geometry.x = x;
        window.geometry.y = y;
        Ok(())
    }
}
