use serde::{Deserialize, Serialize};
use thiserror::Error;

const NORMALIZED_EPSILON: f64 = 1e-12;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Size {
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Error, Clone, PartialEq)]
pub enum GeometryError {
    #[error("geometry contains a non-finite value")]
    NonFinite,
    #[error("width and height must be greater than zero")]
    NonPositiveSize,
    #[error("normalized coordinates must stay inside 0.0..=1.0")]
    OutsideNormalizedBounds,
}

impl Rect {
    pub fn new(x: f64, y: f64, width: f64, height: f64) -> Result<Self, GeometryError> {
        let rect = Self { x, y, width, height };
        rect.validate()?;
        Ok(rect)
    }

    pub fn validate(&self) -> Result<(), GeometryError> {
        if ![self.x, self.y, self.width, self.height].into_iter().all(f64::is_finite) {
            return Err(GeometryError::NonFinite);
        }
        if self.width <= 0.0 || self.height <= 0.0 {
            return Err(GeometryError::NonPositiveSize);
        }
        Ok(())
    }

    pub fn right(self) -> f64 {
        self.x + self.width
    }

    pub fn bottom(self) -> f64 {
        self.y + self.height
    }

    pub fn center(self) -> Point {
        Point { x: self.x + self.width / 2.0, y: self.y + self.height / 2.0 }
    }

    pub fn inset(self, amount: f64) -> Result<Self, GeometryError> {
        Self::new(
            self.x + amount,
            self.y + amount,
            self.width - amount * 2.0,
            self.height - amount * 2.0,
        )
    }

    pub fn approx_eq(self, other: Self, tolerance: f64) -> bool {
        self.position_approx_eq(other, tolerance) && self.size_approx_eq(other, tolerance)
    }

    pub fn position_approx_eq(self, other: Self, tolerance: f64) -> bool {
        (self.x - other.x).abs() <= tolerance && (self.y - other.y).abs() <= tolerance
    }

    pub fn size_approx_eq(self, other: Self, tolerance: f64) -> bool {
        (self.width - other.width).abs() <= tolerance
            && (self.height - other.height).abs() <= tolerance
    }

    pub fn horizontal_overlap(self, other: Self) -> f64 {
        (self.right().min(other.right()) - self.x.max(other.x)).max(0.0)
    }

    pub fn vertical_overlap(self, other: Self) -> f64 {
        (self.bottom().min(other.bottom()) - self.y.max(other.y)).max(0.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct NormalizedRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl NormalizedRect {
    pub fn new(x: f64, y: f64, width: f64, height: f64) -> Result<Self, GeometryError> {
        let rect = Self { x, y, width, height };
        rect.validate()?;
        Ok(rect)
    }

    pub fn validate(&self) -> Result<(), GeometryError> {
        if ![self.x, self.y, self.width, self.height].into_iter().all(f64::is_finite) {
            return Err(GeometryError::NonFinite);
        }
        if self.width <= 0.0 || self.height <= 0.0 {
            return Err(GeometryError::NonPositiveSize);
        }
        if self.x < 0.0
            || self.y < 0.0
            || self.right() > 1.0 + NORMALIZED_EPSILON
            || self.bottom() > 1.0 + NORMALIZED_EPSILON
        {
            return Err(GeometryError::OutsideNormalizedBounds);
        }
        Ok(())
    }

    pub fn right(self) -> f64 {
        self.x + self.width
    }

    pub fn bottom(self) -> f64 {
        self.y + self.height
    }

    pub fn resolve(self, usable: Rect) -> Result<Rect, GeometryError> {
        self.validate()?;
        usable.validate()?;
        Rect::new(
            usable.x + usable.width * self.x,
            usable.y + usable.height * self.y,
            usable.width * self.width,
            usable.height * self.height,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn normalized_rect_resolves_inside_usable_area() {
        let usable = Rect::new(100.0, 50.0, 1000.0, 800.0).unwrap();
        let rect = NormalizedRect::new(0.25, 0.5, 0.5, 0.25).unwrap().resolve(usable).unwrap();
        assert_eq!(rect, Rect::new(350.0, 450.0, 500.0, 200.0).unwrap());
    }

    proptest! {
        #[test]
        fn valid_normalized_rect_never_escapes_bounds(
            x in 0.0f64..0.95,
            y in 0.0f64..0.95,
            w in 0.01f64..1.0,
            h in 0.01f64..1.0,
        ) {
            let w = w.min(1.0 - x);
            let h = h.min(1.0 - y);
            prop_assume!(w > 0.0 && h > 0.0);
            let normalized = NormalizedRect::new(x, y, w, h).unwrap();
            let usable = Rect::new(-1920.0, 0.0, 2560.0, 1440.0).unwrap();
            let resolved = normalized.resolve(usable).unwrap();
            let x_tolerance = usable.width.abs() * 1e-12 + f64::EPSILON;
            let y_tolerance = usable.height.abs() * 1e-12 + f64::EPSILON;
            prop_assert!(resolved.x >= usable.x - x_tolerance);
            prop_assert!(resolved.y >= usable.y - y_tolerance);
            prop_assert!(resolved.right() <= usable.right() + x_tolerance);
            prop_assert!(resolved.bottom() <= usable.bottom() + y_tolerance);
            prop_assert!(resolved.width > 0.0 && resolved.height > 0.0);
        }
    }
}
