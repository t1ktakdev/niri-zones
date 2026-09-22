use std::env;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use zones_core::{Rect, Size};

const STATE_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct StoredGeometry {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl StoredGeometry {
    pub fn from_rect(rect: Rect) -> Self {
        Self { x: rect.x, y: rect.y, width: rect.width, height: rect.height }
    }

    pub fn to_rect(self) -> Result<Rect, String> {
        Rect::new(self.x, self.y, self.width, self.height).map_err(|error| error.to_string())
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct StoredSize {
    pub width: f64,
    pub height: f64,
}

impl StoredSize {
    pub fn from_size(size: Size) -> Self {
        Self { width: size.width, height: size.height }
    }

    pub fn to_size(self) -> Size {
        Size { width: self.width, height: self.height }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowRecord {
    pub id: u64,
    pub app_id: Option<String>,
    pub pid: Option<i32>,
    pub baseline_floating: bool,
    pub baseline_geometry: Option<StoredGeometry>,
    #[serde(default)]
    pub baseline_size: Option<StoredSize>,
    pub current_layout: String,
    pub current_zone: String,
    pub applied_geometry: StoredGeometry,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeState {
    version: u32,
    session: String,
    pub windows: Vec<WindowRecord>,
}

impl RuntimeState {
    pub fn load_current() -> Result<Self, String> {
        let session = current_session()?;
        let path = state_path()?;
        if !path.exists() {
            return Ok(Self::empty(session));
        }
        let input = fs::read_to_string(&path)
            .map_err(|error| format!("could not read {}: {error}", path.display()))?;
        let state: Self = toml::from_str(&input)
            .map_err(|error| format!("could not parse {}: {error}", path.display()))?;
        if state.version != STATE_VERSION {
            return Err(format!(
                "unsupported runtime state version {} (expected {})",
                state.version, STATE_VERSION
            ));
        }
        if state.session != session {
            return Ok(Self::empty(session));
        }
        Ok(state)
    }

    pub fn get(&self, id: u64) -> Option<&WindowRecord> {
        self.windows.iter().find(|record| record.id == id)
    }

    pub fn remove(&mut self, id: u64) -> Option<WindowRecord> {
        let index = self.windows.iter().position(|record| record.id == id)?;
        Some(self.windows.remove(index))
    }

    pub fn upsert_snap(&mut self, update: WindowRecord) {
        if let Some(record) = self.windows.iter_mut().find(|record| record.id == update.id) {
            record.current_layout = update.current_layout;
            record.current_zone = update.current_zone;
            record.applied_geometry = update.applied_geometry;
            return;
        }
        self.windows.push(update);
    }

    pub fn save_atomic(&self) -> Result<(), String> {
        let path = state_path()?;
        let parent = path.parent().ok_or_else(|| "runtime state path has no parent".to_owned())?;
        fs::create_dir_all(parent)
            .map_err(|error| format!("could not create {}: {error}", parent.display()))?;
        fs::set_permissions(parent, fs::Permissions::from_mode(0o700))
            .map_err(|error| format!("could not secure {}: {error}", parent.display()))?;

        let temp = parent.join(format!(".state.toml.tmp-{}", std::process::id()));
        let encoded = toml::to_string_pretty(self)
            .map_err(|error| format!("could not serialize runtime state: {error}"))?;
        let mut file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .mode(0o600)
            .open(&temp)
            .map_err(|error| format!("could not create {}: {error}", temp.display()))?;
        file.write_all(encoded.as_bytes())
            .map_err(|error| format!("could not write {}: {error}", temp.display()))?;
        file.sync_all().map_err(|error| format!("could not sync {}: {error}", temp.display()))?;
        fs::rename(&temp, &path)
            .map_err(|error| format!("could not replace {}: {error}", path.display()))?;
        File::open(parent)
            .and_then(|directory| directory.sync_all())
            .map_err(|error| format!("could not sync {}: {error}", parent.display()))?;
        Ok(())
    }

    fn empty(session: String) -> Self {
        Self { version: STATE_VERSION, session, windows: Vec::new() }
    }
}

fn current_session() -> Result<String, String> {
    env::var("NIRI_SOCKET").map_err(|_| "NIRI_SOCKET is required for runtime state".to_owned())
}

fn state_path() -> Result<PathBuf, String> {
    let socket = PathBuf::from(current_session()?);
    let runtime =
        socket.parent().ok_or_else(|| "NIRI_SOCKET has no parent directory".to_owned())?;
    Ok(runtime.join("niri-zones/state.toml"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_round_trip_preserves_baseline_and_applied_geometry() {
        let mut state = RuntimeState::empty("/run/user/1000/niri.test.sock".into());
        let baseline = Rect::new(10.0, 20.0, 500.0, 400.0).unwrap();
        let applied = Rect::new(6.0, 56.0, 948.0, 1068.0).unwrap();
        state.upsert_snap(WindowRecord {
            id: 7,
            app_id: Some("kitty".into()),
            pid: Some(42),
            baseline_floating: true,
            baseline_geometry: Some(StoredGeometry::from_rect(baseline)),
            baseline_size: Some(StoredSize::from_size(Size { width: 500.0, height: 400.0 })),
            current_layout: "halves".into(),
            current_zone: "1".into(),
            applied_geometry: StoredGeometry::from_rect(applied),
        });

        let encoded = toml::to_string(&state).unwrap();
        let decoded: RuntimeState = toml::from_str(&encoded).unwrap();
        let record = decoded.get(7).unwrap();
        assert!(record.baseline_floating);
        assert_eq!(record.baseline_geometry.unwrap().to_rect().unwrap(), baseline);
        assert_eq!(record.baseline_size.unwrap().to_size(), Size { width: 500.0, height: 400.0 });
        assert_eq!(record.applied_geometry.to_rect().unwrap(), applied);
    }
}
