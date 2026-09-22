use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{GeometryError, NormalizedRect, Rect};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ZoneId(pub String);

impl From<&str> for ZoneId {
    fn from(value: &str) -> Self {
        Self(value.to_owned())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ZoneSpec {
    pub id: ZoneId,
    pub name: String,
    pub rect: NormalizedRect,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedZone {
    pub id: ZoneId,
    pub name: String,
    pub normalized: NormalizedRect,
    pub rect: Rect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Axis {
    Horizontal,
    Vertical,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SplitNode {
    Leaf { id: ZoneId, name: String },
    Split { axis: Axis, ratio: f64, first: Box<SplitNode>, second: Box<SplitNode> },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LayoutKind {
    Rectangles { zones: Vec<ZoneSpec> },
    SplitTree { root: SplitNode },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LayoutDefinition {
    pub name: String,
    pub kind: LayoutKind,
}

#[derive(Debug, Error)]
pub enum LayoutError {
    #[error(transparent)]
    Geometry(#[from] GeometryError),
    #[error("layout must contain at least one zone")]
    Empty,
    #[error("split ratio must be strictly between 0 and 1")]
    InvalidSplitRatio,
    #[error("gap cannot be negative")]
    NegativeGap,
}

impl LayoutDefinition {
    pub fn resolve(&self, usable: Rect, gap: f64) -> Result<Vec<ResolvedZone>, LayoutError> {
        if gap < 0.0 || !gap.is_finite() {
            return Err(LayoutError::NegativeGap);
        }
        let inset = gap / 2.0;
        let zones = match &self.kind {
            LayoutKind::Rectangles { zones } => {
                if zones.is_empty() {
                    return Err(LayoutError::Empty);
                }
                zones
                    .iter()
                    .map(|zone| {
                        zone.rect.validate()?;
                        let rect = zone.rect.resolve(usable)?.inset(inset)?;
                        Ok(ResolvedZone {
                            id: zone.id.clone(),
                            name: zone.name.clone(),
                            normalized: zone.rect,
                            rect,
                        })
                    })
                    .collect::<Result<Vec<_>, LayoutError>>()?
            }
            LayoutKind::SplitTree { root } => {
                let mut out = Vec::new();
                resolve_split(root, usable, usable, inset, &mut out)?;
                out
            }
        };
        Ok(zones)
    }
}

fn resolve_split(
    node: &SplitNode,
    current: Rect,
    root: Rect,
    inset: f64,
    out: &mut Vec<ResolvedZone>,
) -> Result<(), LayoutError> {
    match node {
        SplitNode::Leaf { id, name } => {
            let rect = current.inset(inset)?;
            let normalized = NormalizedRect::new(
                (current.x - root.x) / root.width,
                (current.y - root.y) / root.height,
                current.width / root.width,
                current.height / root.height,
            )?;
            out.push(ResolvedZone { id: id.clone(), name: name.clone(), normalized, rect });
        }
        SplitNode::Split { axis, ratio, first, second } => {
            if !ratio.is_finite() || *ratio <= 0.0 || *ratio >= 1.0 {
                return Err(LayoutError::InvalidSplitRatio);
            }
            let (a, b) = match axis {
                Axis::Horizontal => (
                    Rect::new(current.x, current.y, current.width * ratio, current.height)?,
                    Rect::new(
                        current.x + current.width * ratio,
                        current.y,
                        current.width * (1.0 - ratio),
                        current.height,
                    )?,
                ),
                Axis::Vertical => (
                    Rect::new(current.x, current.y, current.width, current.height * ratio)?,
                    Rect::new(
                        current.x,
                        current.y + current.height * ratio,
                        current.width,
                        current.height * (1.0 - ratio),
                    )?,
                ),
            };
            resolve_split(first, a, root, inset, out)?;
            resolve_split(second, b, root, inset, out)?;
        }
    }
    Ok(())
}

fn leaf(id: &str, name: &str) -> SplitNode {
    SplitNode::Leaf { id: ZoneId::from(id), name: name.to_owned() }
}

fn split(axis: Axis, ratio: f64, first: SplitNode, second: SplitNode) -> SplitNode {
    SplitNode::Split { axis, ratio, first: Box::new(first), second: Box::new(second) }
}

pub fn builtin_layout(name: &str) -> Option<LayoutDefinition> {
    builtin_layouts().into_iter().find(|layout| layout.name == name)
}

pub fn builtin_layouts() -> Vec<LayoutDefinition> {
    vec![
        LayoutDefinition {
            name: "halves".into(),
            kind: LayoutKind::SplitTree {
                root: split(Axis::Horizontal, 0.5, leaf("1", "Left"), leaf("2", "Right")),
            },
        },
        LayoutDefinition {
            name: "thirds".into(),
            kind: LayoutKind::Rectangles {
                zones: vec![
                    zone("1", "Left", 0.0, 0.0, 1.0 / 3.0, 1.0),
                    zone("2", "Center", 1.0 / 3.0, 0.0, 1.0 / 3.0, 1.0),
                    zone("3", "Right", 2.0 / 3.0, 0.0, 1.0 / 3.0, 1.0),
                ],
            },
        },
        LayoutDefinition {
            name: "main-stack".into(),
            kind: LayoutKind::SplitTree {
                root: split(
                    Axis::Horizontal,
                    0.65,
                    leaf("1", "Main"),
                    split(Axis::Vertical, 0.5, leaf("2", "Top stack"), leaf("3", "Bottom stack")),
                ),
            },
        },
        LayoutDefinition {
            name: "quarters".into(),
            kind: LayoutKind::SplitTree {
                root: split(
                    Axis::Horizontal,
                    0.5,
                    split(Axis::Vertical, 0.5, leaf("1", "Top left"), leaf("3", "Bottom left")),
                    split(Axis::Vertical, 0.5, leaf("2", "Top right"), leaf("4", "Bottom right")),
                ),
            },
        },
    ]
}

fn zone(id: &str, name: &str, x: f64, y: f64, width: f64, height: f64) -> ZoneSpec {
    ZoneSpec {
        id: ZoneId::from(id),
        name: name.to_owned(),
        rect: NormalizedRect { x, y, width, height },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn main_stack_is_stable_on_ultrawide() {
        let usable = Rect::new(0.0, 0.0, 3440.0, 1440.0).unwrap();
        let zones = builtin_layout("main-stack").unwrap().resolve(usable, 12.0).unwrap();
        assert_eq!(zones.len(), 3);
        assert!(zones[0].rect.width > zones[1].rect.width);
        assert!(zones.iter().all(|z| z.rect.right() <= usable.right()));
    }

    proptest! {
        #[test]
        fn two_way_split_preserves_total_bounds(
            ratio in 0.01f64..0.99,
            x in -4000.0f64..4000.0,
            y in -2000.0f64..2000.0,
            width in 100.0f64..8000.0,
            height in 100.0f64..5000.0,
        ) {
            let layout = LayoutDefinition {
                name: "property".into(),
                kind: LayoutKind::SplitTree {
                    root: split(Axis::Horizontal, ratio, leaf("a", "A"), leaf("b", "B")),
                },
            };
            let usable = Rect::new(x, y, width, height).unwrap();
            let zones = layout.resolve(usable, 0.0).unwrap();
            prop_assert_eq!(zones.len(), 2);
            prop_assert!((zones[0].rect.x - usable.x).abs() <= f64::EPSILON);
            prop_assert!((zones[1].rect.right() - usable.right()).abs() <= width * 1e-12 + f64::EPSILON);
            prop_assert!((zones[0].rect.width + zones[1].rect.width - width).abs() <= width * 1e-12 + f64::EPSILON);
            prop_assert!((zones[0].rect.right() - zones[1].rect.x).abs() <= width * 1e-12 + f64::EPSILON);
        }
    }

    #[test]
    fn quarters_respect_negative_desktop_origin() {
        let usable = Rect::new(-1080.0, 0.0, 1080.0, 1920.0).unwrap();
        let zones = builtin_layout("quarters").unwrap().resolve(usable, 0.0).unwrap();
        assert_eq!(zones[0].rect.x, -1080.0);
        assert_eq!(zones[2].rect.right(), 0.0);
    }
}
