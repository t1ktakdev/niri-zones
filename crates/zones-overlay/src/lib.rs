//! Wayland-native zone chooser rendered with wlr-layer-shell.

use std::num::NonZeroU32;

use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    delegate_registry,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        keyboard::{KeyEvent, KeyboardHandler, Keysym, Modifiers, RawModifiers},
        pointer::{PointerEvent, PointerEventKind, PointerHandler},
        Capability, SeatHandler, SeatState,
    },
    shell::{
        wlr_layer::{
            Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
            LayerSurfaceConfigure,
        },
        WaylandSurface,
    },
    shm::{slot::SlotPool, Shm, ShmHandler},
};
use thiserror::Error;
use wayland_client::{
    globals::registry_queue_init,
    protocol::{wl_keyboard, wl_output, wl_pointer, wl_seat, wl_shm, wl_surface},
    Connection, QueueHandle,
};
use zones_core::{select_directional, Direction, LayoutDefinition, Rect, ResolvedZone};

#[derive(Debug, Error)]
pub enum OverlayError {
    #[error("Wayland overlay failed: {0}")]
    Wayland(String),
}

pub fn probe_wayland() -> Result<(), OverlayError> {
    Connection::connect_to_env().map_err(display_error)?;
    Ok(())
}

pub fn show_overlay(layout: LayoutDefinition, gap: f64) -> Result<Option<String>, OverlayError> {
    let conn = Connection::connect_to_env().map_err(display_error)?;
    let (globals, mut event_queue) = registry_queue_init(&conn).map_err(display_error)?;
    let qh = event_queue.handle();

    let compositor = CompositorState::bind(&globals, &qh).map_err(display_error)?;
    let layer_shell = LayerShell::bind(&globals, &qh).map_err(display_error)?;
    let shm = Shm::bind(&globals, &qh).map_err(display_error)?;
    let surface = compositor.create_surface(&qh);
    let layer =
        layer_shell.create_layer_surface(&qh, surface, Layer::Overlay, Some("niri-zones"), None);
    layer.set_anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT);
    layer.set_keyboard_interactivity(KeyboardInteractivity::Exclusive);
    layer.set_exclusive_zone(0);
    layer.set_size(0, 0);
    layer.commit();

    let pool = SlotPool::new(4, &shm).map_err(display_error)?;
    let mut overlay = Overlay {
        registry_state: RegistryState::new(&globals),
        seat_state: SeatState::new(&globals, &qh),
        output_state: OutputState::new(&globals, &qh),
        shm,
        pool,
        layer,
        layout,
        gap,
        zones: Vec::new(),
        selected: 0,
        width: 1,
        height: 1,
        first_configure: true,
        keyboard: None,
        pointer: None,
        exit: false,
        confirmed: None,
        failure: None,
    };
    while !overlay.exit {
        event_queue.blocking_dispatch(&mut overlay).map_err(display_error)?;
    }

    if let Some(error) = overlay.failure {
        return Err(OverlayError::Wayland(error));
    }
    Ok(overlay.confirmed)
}

fn display_error(error: impl std::fmt::Display) -> OverlayError {
    OverlayError::Wayland(error.to_string())
}

struct Overlay {
    registry_state: RegistryState,
    seat_state: SeatState,
    output_state: OutputState,
    shm: Shm,
    pool: SlotPool,
    layer: LayerSurface,
    layout: LayoutDefinition,
    gap: f64,
    zones: Vec<ResolvedZone>,
    selected: usize,
    width: u32,
    height: u32,
    first_configure: bool,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    pointer: Option<wl_pointer::WlPointer>,
    exit: bool,
    confirmed: Option<String>,
    failure: Option<String>,
}

impl Overlay {
    fn refresh_zones(&mut self) {
        let usable = match Rect::new(0.0, 0.0, self.width as f64, self.height as f64) {
            Ok(rect) => rect,
            Err(error) => {
                self.fail(error.to_string());
                return;
            }
        };
        match self.layout.resolve(usable, self.gap) {
            Ok(zones) => {
                self.zones = zones;
                if self.selected >= self.zones.len() {
                    self.selected = 0;
                }
            }
            Err(error) => self.fail(error.to_string()),
        }
    }

    fn fail(&mut self, message: String) {
        self.failure = Some(message);
        self.exit = true;
    }
    fn draw(&mut self) {
        if self.exit || self.zones.is_empty() {
            return;
        }

        let stride = self.width as i32 * 4;
        let (buffer, canvas) = match self.pool.create_buffer(
            self.width as i32,
            self.height as i32,
            stride,
            wl_shm::Format::Argb8888,
        ) {
            Ok(value) => value,
            Err(error) => {
                self.fail(error.to_string());
                return;
            }
        };

        canvas.fill(0);
        for (index, zone) in self.zones.iter().enumerate() {
            let selected = index == self.selected;
            draw_zone(canvas, self.width, self.height, zone.rect, selected, index + 1);
        }

        self.layer.wl_surface().damage_buffer(0, 0, self.width as i32, self.height as i32);
        if let Err(error) = buffer.attach_to(self.layer.wl_surface()) {
            self.fail(error.to_string());
            return;
        }
        self.layer.commit();
    }
    fn choose_index(&mut self, index: usize) {
        if index < self.zones.len() {
            self.selected = index;
            self.confirmed = Some(self.zones[index].id.0.clone());
            self.exit = true;
        }
    }

    fn select_direction(&mut self, direction: Direction) {
        let Some(source) = self.zones.get(self.selected).map(|zone| zone.rect) else {
            return;
        };
        let Some(next) = select_directional(source, &self.zones, direction) else {
            return;
        };
        if let Some(index) = self.zones.iter().position(|zone| zone.id == next.id) {
            self.selected = index;
        }
    }

    fn zone_at(&self, x: f64, y: f64) -> Option<usize> {
        self.zones.iter().position(|zone| {
            x >= zone.rect.x && x < zone.rect.right() && y >= zone.rect.y && y < zone.rect.bottom()
        })
    }
    fn handle_key(&mut self, event: KeyEvent) -> bool {
        if event.keysym == Keysym::Escape {
            self.exit = true;
            return false;
        }
        if event.keysym == Keysym::Return || event.keysym == Keysym::KP_Enter {
            self.choose_index(self.selected);
            return false;
        }

        let direction = if event.keysym == Keysym::Left {
            Some(Direction::Left)
        } else if event.keysym == Keysym::Right {
            Some(Direction::Right)
        } else if event.keysym == Keysym::Up {
            Some(Direction::Up)
        } else if event.keysym == Keysym::Down {
            Some(Direction::Down)
        } else {
            None
        };
        if let Some(direction) = direction {
            self.select_direction(direction);
            return true;
        }

        let Some(text) = event.utf8.as_deref() else {
            return false;
        };
        let Some(digit) = text.chars().next().and_then(|value| value.to_digit(10)) else {
            return false;
        };
        if digit > 0 {
            self.choose_index(digit as usize - 1);
        }
        false
    }
}

impl CompositorHandler for Overlay {
    fn scale_factor_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _new_factor: i32,
    ) {
    }

    fn transform_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _new_transform: wl_output::Transform,
    ) {
    }
    fn frame(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _time: u32,
    ) {
    }

    fn surface_enter(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _output: &wl_output::WlOutput,
    ) {
    }

    fn surface_leave(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _output: &wl_output::WlOutput,
    ) {
    }
}

impl LayerShellHandler for Overlay {
    fn closed(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _layer: &LayerSurface) {
        self.exit = true;
    }

    fn configure(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _layer: &LayerSurface,
        configure: LayerSurfaceConfigure,
        _serial: u32,
    ) {
        self.width = NonZeroU32::new(configure.new_size.0).map_or(1, NonZeroU32::get);
        self.height = NonZeroU32::new(configure.new_size.1).map_or(1, NonZeroU32::get);
        self.refresh_zones();
        if self.first_configure {
            self.first_configure = false;
        }
        self.draw();
    }
}

impl SeatHandler for Overlay {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat_state
    }

    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
    fn new_capability(
        &mut self,
        _conn: &Connection,
        qh: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Keyboard && self.keyboard.is_none() {
            match self.seat_state.get_keyboard(qh, &seat, None) {
                Ok(keyboard) => self.keyboard = Some(keyboard),
                Err(error) => self.fail(error.to_string()),
            }
        }
        if capability == Capability::Pointer && self.pointer.is_none() {
            match self.seat_state.get_pointer(qh, &seat) {
                Ok(pointer) => self.pointer = Some(pointer),
                Err(error) => self.fail(error.to_string()),
            }
        }
    }

    fn remove_capability(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Keyboard {
            if let Some(keyboard) = self.keyboard.take() {
                keyboard.release();
            }
        }
        if capability == Capability::Pointer {
            if let Some(pointer) = self.pointer.take() {
                pointer.release();
            }
        }
    }

    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

impl KeyboardHandler for Overlay {
    fn enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: &wl_surface::WlSurface,
        _: u32,
        _: &[u32],
        _: &[Keysym],
    ) {
    }
    fn leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: &wl_surface::WlSurface,
        _: u32,
    ) {
    }

    fn press_key(
        &mut self,
        _: &Connection,
        _qh: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        event: KeyEvent,
    ) {
        if self.handle_key(event) {
            self.draw();
        }
    }

    fn repeat_key(
        &mut self,
        _: &Connection,
        _qh: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        event: KeyEvent,
    ) {
        if self.handle_key(event) {
            self.draw();
        }
    }

    fn release_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        _: KeyEvent,
    ) {
    }

    fn update_modifiers(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        _: Modifiers,
        _: RawModifiers,
        _: u32,
    ) {
    }
}

impl PointerHandler for Overlay {
    fn pointer_frame(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_pointer::WlPointer,
        events: &[PointerEvent],
    ) {
        let mut redraw = false;
        for event in events {
            if event.surface != *self.layer.wl_surface() {
                continue;
            }
            match event.kind {
                PointerEventKind::Enter { .. } | PointerEventKind::Motion { .. } => {
                    if let Some(index) = self.zone_at(event.position.0, event.position.1) {
                        if index != self.selected {
                            self.selected = index;
                            redraw = true;
                        }
                    }
                }
                PointerEventKind::Press { button: 0x110, .. } => {
                    if let Some(index) = self.zone_at(event.position.0, event.position.1) {
                        self.choose_index(index);
                    }
                }
                _ => {}
            }
        }
        if redraw {
            self.draw();
        }
    }
}

impl OutputHandler for Overlay {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }

    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}

    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}

    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl ShmHandler for Overlay {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

#[derive(Clone, Copy)]
struct PixelRect {
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
}

impl PixelRect {
    fn from_rect(rect: Rect, width: u32, height: u32) -> Option<Self> {
        let bounds = Self {
            x0: rect.x.floor().max(0.0) as i32,
            y0: rect.y.floor().max(0.0) as i32,
            x1: rect.right().ceil().min(width as f64) as i32,
            y1: rect.bottom().ceil().min(height as f64) as i32,
        };
        (bounds.x1 > bounds.x0 && bounds.y1 > bounds.y0).then_some(bounds)
    }

    fn center(self) -> (i32, i32) {
        ((self.x0 + self.x1) / 2, (self.y0 + self.y1) / 2)
    }
}

fn draw_zone(canvas: &mut [u8], width: u32, height: u32, rect: Rect, selected: bool, label: usize) {
    let Some(bounds) = PixelRect::from_rect(rect, width, height) else {
        return;
    };
    let size = (width, height);
    let fill = if selected { premul(104, 52, 132, 246) } else { premul(58, 55, 94, 160) };
    let border = if selected { premul(230, 120, 190, 255) } else { premul(145, 150, 170, 210) };
    fill_rect(canvas, size, bounds, fill);
    stroke_rect(canvas, size, bounds, 3, border);

    if label <= 9 {
        let glyph_scale = ((bounds.x1 - bounds.x0).min(bounds.y1 - bounds.y0) / 18).clamp(6, 18);
        draw_digit(
            canvas,
            size,
            label as u8,
            bounds.center(),
            glyph_scale,
            premul(245, 255, 255, 255),
        );
    }
}

fn fill_rect(canvas: &mut [u8], size: (u32, u32), bounds: PixelRect, color: [u8; 4]) {
    let (width, height) = size;
    let x0 = bounds.x0.clamp(0, width as i32);
    let x1 = bounds.x1.clamp(0, width as i32);
    let y0 = bounds.y0.clamp(0, height as i32);
    let y1 = bounds.y1.clamp(0, height as i32);
    for y in y0..y1 {
        let row = y as usize * width as usize * 4;
        for x in x0..x1 {
            let offset = row + x as usize * 4;
            canvas[offset..offset + 4].copy_from_slice(&color);
        }
    }
}

fn stroke_rect(
    canvas: &mut [u8],
    size: (u32, u32),
    bounds: PixelRect,
    thickness: i32,
    color: [u8; 4],
) {
    let PixelRect { x0, y0, x1, y1 } = bounds;
    fill_rect(canvas, size, PixelRect { x0, y0, x1, y1: y0 + thickness }, color);
    fill_rect(canvas, size, PixelRect { x0, y0: y1 - thickness, x1, y1 }, color);
    fill_rect(canvas, size, PixelRect { x0, y0, x1: x0 + thickness, y1 }, color);
    fill_rect(canvas, size, PixelRect { x0: x1 - thickness, y0, x1, y1 }, color);
}
fn draw_digit(
    canvas: &mut [u8],
    size: (u32, u32),
    digit: u8,
    center: (i32, i32),
    scale: i32,
    color: [u8; 4],
) {
    let (width, height) = size;
    let (center_x, center_y) = center;
    const GLYPHS: [[u8; 7]; 10] = [
        [0b11111, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b11111],
        [0b00100, 0b01100, 0b10100, 0b00100, 0b00100, 0b00100, 0b11111],
        [0b11110, 0b00001, 0b00001, 0b11110, 0b10000, 0b10000, 0b11111],
        [0b11110, 0b00001, 0b00001, 0b01110, 0b00001, 0b00001, 0b11110],
        [0b10010, 0b10010, 0b10010, 0b11111, 0b00010, 0b00010, 0b00010],
        [0b11111, 0b10000, 0b10000, 0b11110, 0b00001, 0b00001, 0b11110],
        [0b01111, 0b10000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110],
        [0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000],
        [0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110],
        [0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00001, 0b11110],
    ];
    let glyph = GLYPHS[digit as usize];
    let glyph_width = 5 * scale;
    let glyph_height = 7 * scale;
    let start_x = center_x - glyph_width / 2;
    let start_y = center_y - glyph_height / 2;

    for (row, bits) in glyph.into_iter().enumerate() {
        for column in 0..5 {
            if bits & (1 << (4 - column)) != 0 {
                let x = start_x + column * scale;
                let y = start_y + row as i32 * scale;
                fill_rect(
                    canvas,
                    (width, height),
                    PixelRect { x0: x, y0: y, x1: x + scale, y1: y + scale },
                    color,
                );
            }
        }
    }
}

fn premul(alpha: u8, red: u8, green: u8, blue: u8) -> [u8; 4] {
    let a = alpha as u32;
    let r = red as u32 * a / 255;
    let g = green as u32 * a / 255;
    let b = blue as u32 * a / 255;
    let packed = (a << 24) | (r << 16) | (g << 8) | b;
    packed.to_ne_bytes()
}
delegate_registry!(Overlay);

impl ProvidesRegistryState for Overlay {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }

    registry_handlers![OutputState, SeatState];
}

smithay_client_toolkit::delegate_dispatch2!(Overlay);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn premultiplied_argb_has_expected_alpha() {
        let bytes = premul(128, 255, 0, 0);
        let packed = u32::from_ne_bytes(bytes);
        assert_eq!(packed >> 24, 128);
        assert_eq!((packed >> 16) & 0xff, 128);
    }

    #[test]
    fn zone_preview_uses_core_geometry() {
        let layout = zones_core::builtin_layout("halves").unwrap();
        let usable = Rect::new(0.0, 0.0, 1920.0, 1080.0).unwrap();
        let zones = layout.resolve(usable, 12.0).unwrap();
        assert_eq!(zones[0].rect, Rect::new(6.0, 6.0, 948.0, 1068.0).unwrap());
    }
}
