//! Thin translation layer between niri-zones domain values and Niri 26.04 IPC.
//!
//! This crate deliberately does not guess Niri's working-area rectangle. Normalized
//! zone coordinates are converted to Niri's percentage-based actions so the compositor
//! resolves them against its own working area.

use std::collections::HashMap;
use std::io;
use std::thread;
use std::time::{Duration, Instant};

use niri_ipc::socket::Socket;
use niri_ipc::{Action, Event, Output, PositionChange, Request, Response, SizeChange, Window};
use thiserror::Error;
use zones_core::{NormalizedRect, Rect, Size};

#[derive(Debug, Error)]
pub enum NiriError {
    #[error("could not communicate with Niri: {0}")]
    Io(#[from] io::Error),
    #[error("Niri rejected the request: {0}")]
    Rejected(String),
    #[error("unexpected Niri response: expected {expected}, received {actual}")]
    UnexpectedResponse { expected: &'static str, actual: &'static str },
    #[error("window {0} no longer exists")]
    WindowMissing(u64),
    #[error("window {0} is tiled; explicit tiled-to-floating permission is required")]
    RequiresFloating(u64),
    #[error("gap must be finite and non-negative")]
    InvalidGap,
    #[error("gap is too large for Niri's fixed-size adjustment")]
    GapOutOfRange,
    #[error("invalid normalized target: {0}")]
    InvalidTarget(String),
    #[error("window {0} disappeared or did not become floating after the apply sequence")]
    PostconditionFailed(u64),
    #[error("timed out waiting for window {0} to enter the floating layout")]
    FloatingTransitionTimeout(u64),
    #[error("timed out waiting for window {0} geometry to settle after IPC actions")]
    GeometrySettleTimeout(u64),
    #[error("timed out waiting for window {0} to return to the tiling layout")]
    TilingTransitionTimeout(u64),
    #[error("saved restore geometry is missing or cannot be represented by Niri")]
    InvalidRestoreGeometry,
}

pub struct NiriBackend {
    socket: Socket,
}

pub struct NiriEventStream {
    read_next: Box<dyn FnMut() -> io::Result<Event>>,
}

#[derive(Debug, Clone)]
pub struct ApplyReport {
    pub before: Window,
    pub after: Window,
    pub actions_sent: usize,
}

impl NiriBackend {
    pub fn connect() -> Result<Self, NiriError> {
        Ok(Self { socket: Socket::connect()? })
    }

    pub fn version(&mut self) -> Result<String, NiriError> {
        match self.request(Request::Version)? {
            Response::Version(version) => Ok(version),
            other => Err(unexpected("Version", &other)),
        }
    }

    pub fn outputs(&mut self) -> Result<HashMap<String, Output>, NiriError> {
        match self.request(Request::Outputs)? {
            Response::Outputs(outputs) => Ok(outputs),
            other => Err(unexpected("Outputs", &other)),
        }
    }

    pub fn windows(&mut self) -> Result<Vec<Window>, NiriError> {
        match self.request(Request::Windows)? {
            Response::Windows(windows) => Ok(windows),
            other => Err(unexpected("Windows", &other)),
        }
    }

    pub fn focused_window(&mut self) -> Result<Option<Window>, NiriError> {
        match self.request(Request::FocusedWindow)? {
            Response::FocusedWindow(window) => Ok(window),
            other => Err(unexpected("FocusedWindow", &other)),
        }
    }

    pub fn window(&mut self, id: u64) -> Result<Window, NiriError> {
        self.windows()?
            .into_iter()
            .find(|window| window.id == id)
            .ok_or(NiriError::WindowMissing(id))
    }

    /// Apply a normalized zone to a Niri window.
    ///
    /// `gap` is in logical pixels. The base geometry uses Niri percentages; when a gap
    /// is requested, follow-up fixed adjustments inset the target by half the gap on
    /// each edge. This avoids inventing panel/strut dimensions that Niri IPC does not expose.
    ///
    /// This method checks existence/floating state again after applying. Exact geometry
    /// verification needs compositor-session testing because applications can impose size
    /// constraints and configure responses are asynchronous.
    pub fn apply_zone(
        &mut self,
        id: u64,
        target: NormalizedRect,
        gap: f64,
        allow_tiled_to_floating: bool,
    ) -> Result<ApplyReport, NiriError> {
        target.validate().map_err(|error| NiriError::InvalidTarget(error.to_string()))?;
        validate_gap(gap)?;

        let before = self.window(id)?;
        if !before.is_floating && !allow_tiled_to_floating {
            return Err(NiriError::RequiresFloating(id));
        }

        let mut actions_sent = 0;
        if !before.is_floating {
            self.action(Action::MoveWindowToFloating { id: Some(id) })?;
            actions_sent += 1;
            self.wait_until_floating(id, Duration::from_secs(1))?;
        }

        for action in zone_action_plan(id, target, gap)? {
            self.action(action)?;
            actions_sent += 1;
        }

        let after = self.wait_until_layout_stable(id, true, Duration::from_secs(1))?;
        if !after.is_floating {
            return Err(NiriError::PostconditionFailed(id));
        }

        Ok(ApplyReport { before, after, actions_sent })
    }

    pub fn restore_window(
        &mut self,
        id: u64,
        baseline_floating: bool,
        baseline_geometry: Option<Rect>,
        baseline_size: Option<Size>,
    ) -> Result<Window, NiriError> {
        let current = self.window(id)?;
        if baseline_floating {
            let geometry = baseline_geometry.ok_or(NiriError::InvalidRestoreGeometry)?;
            if !current.is_floating {
                self.action(Action::MoveWindowToFloating { id: Some(id) })?;
                self.wait_until_floating(id, Duration::from_secs(1))?;
            }
            self.action(Action::SetWindowWidth {
                id: Some(id),
                change: SizeChange::SetFixed(fixed_size(geometry.width)?),
            })?;
            self.action(Action::SetWindowHeight {
                id: Some(id),
                change: SizeChange::SetFixed(fixed_size(geometry.height)?),
            })?;
            let resized = self.wait_until_layout_stable(id, true, Duration::from_secs(1))?;
            let current_geometry =
                visual_geometry(&resized).ok_or(NiriError::InvalidRestoreGeometry)?;
            self.action(Action::MoveFloatingWindow {
                id: Some(id),
                x: PositionChange::AdjustFixed(geometry.x - current_geometry.x),
                y: PositionChange::AdjustFixed(geometry.y - current_geometry.y),
            })?;
            self.wait_until_layout_stable(id, true, Duration::from_secs(1))
        } else {
            if current.is_floating {
                self.action(Action::MoveWindowToTiling { id: Some(id) })?;
                self.wait_until_tiled(id, Duration::from_secs(1))?;
            }
            let size = baseline_size.ok_or(NiriError::InvalidRestoreGeometry)?;
            self.action(Action::SetWindowWidth {
                id: Some(id),
                change: SizeChange::SetFixed(fixed_size(size.width)?),
            })?;
            self.action(Action::SetWindowHeight {
                id: Some(id),
                change: SizeChange::SetFixed(fixed_size(size.height)?),
            })?;
            self.wait_until_layout_stable(id, false, Duration::from_secs(1))
        }
    }

    fn wait_until_floating(&mut self, id: u64, timeout: Duration) -> Result<Window, NiriError> {
        let deadline = Instant::now() + timeout;
        loop {
            let window = self.window(id)?;
            if window.is_floating {
                return Ok(window);
            }
            if Instant::now() >= deadline {
                return Err(NiriError::FloatingTransitionTimeout(id));
            }
            thread::sleep(Duration::from_millis(10));
        }
    }

    fn wait_until_tiled(&mut self, id: u64, timeout: Duration) -> Result<Window, NiriError> {
        let deadline = Instant::now() + timeout;
        loop {
            let window = self.window(id)?;
            if !window.is_floating {
                return Ok(window);
            }
            if Instant::now() >= deadline {
                return Err(NiriError::TilingTransitionTimeout(id));
            }
            thread::sleep(Duration::from_millis(10));
        }
    }

    fn wait_until_layout_stable(
        &mut self,
        id: u64,
        expected_floating: bool,
        timeout: Duration,
    ) -> Result<Window, NiriError> {
        const POLL_INTERVAL: Duration = Duration::from_millis(10);
        const MIN_OBSERVE: Duration = Duration::from_millis(250);
        const STABLE_FOR: Duration = Duration::from_millis(120);

        let started = Instant::now();
        let deadline = started + timeout;
        let mut previous: Option<Window> = None;
        let mut stable_since: Option<Instant> = None;

        loop {
            let now = Instant::now();
            let current = self.window(id)?;
            if current.is_floating == expected_floating {
                match &previous {
                    Some(previous)
                        if previous.is_floating == expected_floating
                            && previous.layout == current.layout =>
                    {
                        stable_since.get_or_insert(now);
                    }
                    _ => stable_since = Some(now),
                }

                if now.duration_since(started) >= MIN_OBSERVE
                    && stable_since.is_some_and(|since| now.duration_since(since) >= STABLE_FOR)
                {
                    return Ok(current);
                }
            } else {
                stable_since = None;
            }
            previous = Some(current);

            if now >= deadline {
                return Err(NiriError::GeometrySettleTimeout(id));
            }
            thread::sleep(POLL_INTERVAL);
        }
    }

    pub fn action(&mut self, action: Action) -> Result<(), NiriError> {
        match self.request(Request::Action(action))? {
            Response::Handled => Ok(()),
            other => Err(unexpected("Handled", &other)),
        }
    }

    fn request(&mut self, request: Request) -> Result<Response, NiriError> {
        self.socket.send(request)?.map_err(NiriError::Rejected)
    }
}

impl NiriEventStream {
    /// Open a dedicated IPC socket for events. Commands must use a separate `NiriBackend`
    /// socket because Niri stops reading requests after `Request::EventStream`.
    pub fn connect() -> Result<Self, NiriError> {
        let mut socket = Socket::connect()?;
        match socket.send(Request::EventStream)?.map_err(NiriError::Rejected)? {
            Response::Handled => Ok(Self { read_next: Box::new(socket.read_events()) }),
            other => Err(unexpected("Handled", &other)),
        }
    }

    pub fn next_event(&mut self) -> Result<Event, NiriError> {
        (self.read_next)().map_err(NiriError::Io)
    }
}

/// Produce only Niri IPC actions, with no I/O. Keeping this translation pure makes
/// it testable without a running compositor.
pub fn zone_action_plan(
    id: u64,
    target: NormalizedRect,
    gap: f64,
) -> Result<Vec<Action>, NiriError> {
    target.validate().map_err(|error| NiriError::InvalidTarget(error.to_string()))?;
    validate_gap(gap)?;

    let mut actions = vec![
        Action::SetWindowWidth {
            id: Some(id),
            change: SizeChange::SetProportion(to_percent(target.width)),
        },
        Action::SetWindowHeight {
            id: Some(id),
            change: SizeChange::SetProportion(to_percent(target.height)),
        },
        Action::MoveFloatingWindow {
            id: Some(id),
            x: PositionChange::SetProportion(to_percent(target.x)),
            y: PositionChange::SetProportion(to_percent(target.y)),
        },
    ];

    if gap > 0.0 {
        let shrink = gap.round();
        if shrink > i32::MAX as f64 {
            return Err(NiriError::GapOutOfRange);
        }
        let shrink = shrink as i32;
        actions.extend([
            Action::SetWindowWidth { id: Some(id), change: SizeChange::AdjustFixed(-shrink) },
            Action::SetWindowHeight { id: Some(id), change: SizeChange::AdjustFixed(-shrink) },
            Action::MoveFloatingWindow {
                id: Some(id),
                x: PositionChange::AdjustFixed(gap / 2.0),
                y: PositionChange::AdjustFixed(gap / 2.0),
            },
        ]);
    }

    Ok(actions)
}

pub fn visual_geometry(window: &Window) -> Option<Rect> {
    let (x, y) = window.layout.tile_pos_in_workspace_view?;
    let (width, height) = window.layout.tile_size;
    Rect::new(x, y, width, height).ok()
}

fn fixed_size(value: f64) -> Result<i32, NiriError> {
    if !value.is_finite() {
        return Err(NiriError::InvalidRestoreGeometry);
    }
    let rounded = value.round();
    if rounded < 1.0 || rounded > i32::MAX as f64 {
        return Err(NiriError::InvalidRestoreGeometry);
    }
    Ok(rounded as i32)
}

fn validate_gap(gap: f64) -> Result<(), NiriError> {
    if !gap.is_finite() || gap < 0.0 {
        return Err(NiriError::InvalidGap);
    }
    Ok(())
}

fn to_percent(normalized: f64) -> f64 {
    normalized * 100.0
}

fn response_name(response: &Response) -> &'static str {
    match response {
        Response::Handled => "Handled",
        Response::Version(_) => "Version",
        Response::Outputs(_) => "Outputs",
        Response::Workspaces(_) => "Workspaces",
        Response::Windows(_) => "Windows",
        Response::Layers(_) => "Layers",
        Response::KeyboardLayouts(_) => "KeyboardLayouts",
        Response::FocusedOutput(_) => "FocusedOutput",
        Response::FocusedWindow(_) => "FocusedWindow",
        Response::PickedWindow(_) => "PickedWindow",
        Response::PickedColor(_) => "PickedColor",
        Response::OutputConfigChanged(_) => "OutputConfigChanged",
        Response::OverviewState(_) => "OverviewState",
        Response::Casts(_) => "Casts",
    }
}

fn unexpected(expected: &'static str, actual: &Response) -> NiriError {
    NiriError::UnexpectedResponse { expected, actual: response_name(actual) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalized_half_screen_becomes_niri_percentages() {
        let target = NormalizedRect::new(0.5, 0.0, 0.5, 1.0).unwrap();
        let actions = zone_action_plan(42, target, 0.0).unwrap();
        assert_eq!(actions.len(), 3);

        match &actions[0] {
            Action::SetWindowWidth { id, change: SizeChange::SetProportion(value) } => {
                assert_eq!(*id, Some(42));
                assert_eq!(*value, 50.0);
            }
            other => panic!("unexpected action: {other:?}"),
        }
        match &actions[2] {
            Action::MoveFloatingWindow {
                id,
                x: PositionChange::SetProportion(x),
                y: PositionChange::SetProportion(y),
            } => {
                assert_eq!(*id, Some(42));
                assert_eq!((*x, *y), (50.0, 0.0));
            }
            other => panic!("unexpected action: {other:?}"),
        }
    }

    #[test]
    fn logical_gap_is_applied_without_guessing_working_area_size() {
        let target = NormalizedRect::new(0.0, 0.0, 0.5, 1.0).unwrap();
        let actions = zone_action_plan(9, target, 12.0).unwrap();
        assert_eq!(actions.len(), 6);

        assert!(matches!(
            actions[3],
            Action::SetWindowWidth { id: Some(9), change: SizeChange::AdjustFixed(-12) }
        ));
        assert!(matches!(
            actions[5],
            Action::MoveFloatingWindow {
                id: Some(9),
                x: PositionChange::AdjustFixed(6.0),
                y: PositionChange::AdjustFixed(6.0)
            }
        ));
    }

    #[test]
    fn negative_gap_is_rejected_before_io() {
        let target = NormalizedRect::new(0.0, 0.0, 1.0, 1.0).unwrap();
        assert!(matches!(zone_action_plan(1, target, -1.0), Err(NiriError::InvalidGap)));
    }
}
