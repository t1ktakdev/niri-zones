use crate::{Rect, ResolvedZone};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Left,
    Right,
    Up,
    Down,
}

#[derive(Debug, Clone, Copy)]
struct Score {
    has_overlap: bool,
    overlap_ratio: f64,
    primary_distance: f64,
    orthogonal_distance: f64,
    euclidean_distance: f64,
}

pub fn select_directional<'a>(source: Rect, zones: &'a [ResolvedZone], direction: Direction) -> Option<&'a ResolvedZone> {
    zones
        .iter()
        .filter_map(|zone| score(source, zone.rect, direction).map(|score| (zone, score)))
        .min_by(|(a_zone, a), (b_zone, b)| compare_scores(*a, *b).then_with(|| a_zone.id.0.cmp(&b_zone.id.0)))
        .map(|(zone, _)| zone)
}

fn compare_scores(a: Score, b: Score) -> std::cmp::Ordering {
    b.has_overlap
        .cmp(&a.has_overlap)
        .then_with(|| b.overlap_ratio.total_cmp(&a.overlap_ratio))
        .then_with(|| a.primary_distance.total_cmp(&b.primary_distance))
        .then_with(|| a.orthogonal_distance.total_cmp(&b.orthogonal_distance))
        .then_with(|| a.euclidean_distance.total_cmp(&b.euclidean_distance))
}

fn score(source: Rect, target: Rect, direction: Direction) -> Option<Score> {
    let s = source.center();
    let t = target.center();
    let dx = t.x - s.x;
    let dy = t.y - s.y;

    let in_half_plane = match direction {
        Direction::Left => dx < 0.0,
        Direction::Right => dx > 0.0,
        Direction::Up => dy < 0.0,
        Direction::Down => dy > 0.0,
    };
    if !in_half_plane {
        return None;
    }

    let (overlap, overlap_base, primary_distance, orthogonal_distance) = match direction {
        Direction::Left | Direction::Right => (
            source.vertical_overlap(target),
            source.height.min(target.height),
            dx.abs(),
            dy.abs(),
        ),
        Direction::Up | Direction::Down => (
            source.horizontal_overlap(target),
            source.width.min(target.width),
            dy.abs(),
            dx.abs(),
        ),
    };
    let overlap_ratio = if overlap_base > 0.0 { overlap / overlap_base } else { 0.0 };

    Some(Score {
        has_overlap: overlap > 0.0,
        overlap_ratio,
        primary_distance,
        orthogonal_distance,
        euclidean_distance: (dx * dx + dy * dy).sqrt(),
    })
}

#[cfg(test)]
mod tests {
    use crate::{NormalizedRect, ZoneId};

    use super::*;

    fn zone(id: &str, rect: Rect) -> ResolvedZone {
        ResolvedZone {
            id: ZoneId::from(id),
            name: id.into(),
            normalized: NormalizedRect::new(0.0, 0.0, 1.0, 1.0).unwrap(),
            rect,
        }
    }

    #[test]
    fn right_prefers_vertical_overlap_over_diagonal_candidate() {
        let source = Rect::new(0.0, 0.0, 100.0, 100.0).unwrap();
        let zones = vec![
            zone("diagonal", Rect::new(120.0, 180.0, 100.0, 100.0).unwrap()),
            zone("right", Rect::new(180.0, 20.0, 100.0, 100.0).unwrap()),
        ];
        assert_eq!(select_directional(source, &zones, Direction::Right).unwrap().id.0, "right");
    }

    #[test]
    fn direction_is_based_on_geometry_not_id() {
        let source = Rect::new(100.0, 100.0, 100.0, 100.0).unwrap();
        let zones = vec![
            zone("99", Rect::new(0.0, 100.0, 80.0, 100.0).unwrap()),
            zone("1", Rect::new(240.0, 100.0, 80.0, 100.0).unwrap()),
        ];
        assert_eq!(select_directional(source, &zones, Direction::Left).unwrap().id.0, "99");
    }
}
