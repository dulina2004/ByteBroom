pub(crate) const TILE_LIMIT: usize = 80;

#[derive(Clone, Copy, Debug)]
pub(crate) struct Tile {
    pub(crate) id: usize,
    pub(crate) x: f64,
    pub(crate) y: f64,
    pub(crate) width: f64,
    pub(crate) height: f64,
    pub(crate) label: bool,
    pub(crate) logical_bytes: u128,
}

#[derive(Clone, Copy)]
struct Rect {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

/// Partition rectangles by balanced byte totals along the longest viewport axis.
/// The caller supplies a reserved ID for aggregated tiles. Coordinates are normalized.
pub(crate) fn layout(
    weights: impl Iterator<Item = (usize, u64)>,
    other: usize,
    width: f64,
    height: f64,
) -> Vec<Tile> {
    if !(width.is_finite() && height.is_finite() && width > 0.0 && height > 0.0) {
        return Vec::new();
    }
    let mut weights = weights
        .filter(|&(_, size)| size != 0)
        .map(|(id, size)| (id, u128::from(size)))
        .collect::<Vec<_>>();
    weights.sort_unstable_by_key(|&(id, size)| (std::cmp::Reverse(size), id));
    if weights.len() > TILE_LIMIT {
        let omitted = weights.drain(TILE_LIMIT - 1..).map(|(_, size)| size).sum();
        weights.push((other, omitted));
        weights.sort_unstable_by_key(|&(id, size)| (std::cmp::Reverse(size), id));
    }
    let mut tiles = Vec::with_capacity(weights.len());
    if !weights.is_empty() {
        partition(
            &weights,
            weights.iter().map(|&(_, size)| size).sum(),
            Rect {
                x: 0.0,
                y: 0.0,
                width: 1.0,
                height: 1.0,
            },
            (width, height),
            &mut tiles,
        );
    }
    tiles
}

fn partition(
    weights: &[(usize, u128)],
    sum: u128,
    rect: Rect,
    viewport: (f64, f64),
    tiles: &mut Vec<Tile>,
) {
    let (width, height) = (rect.width * viewport.0, rect.height * viewport.1);
    if weights.len() == 1 {
        tiles.push(Tile {
            id: weights[0].0,
            x: rect.x,
            y: rect.y,
            width: rect.width,
            height: rect.height,
            label: width >= 96.0 && height >= 43.0,
            logical_bytes: weights[0].1,
        });
        return;
    }
    let mut left = weights[0].1;
    let mut split = 1;
    while split < weights.len() - 1 {
        let next = left + weights[split].1;
        if next.abs_diff(sum / 2) >= left.abs_diff(sum / 2) {
            break;
        }
        left = next;
        split += 1;
    }
    // Compute the smaller side directly so a very skewed split preserves its area.
    let split_axis = |length: f64| {
        if left <= sum - left {
            let first = length * (left as f64 / sum as f64);
            (first, length - first)
        } else {
            let second = length * ((sum - left) as f64 / sum as f64);
            (length - second, second)
        }
    };
    let (first, second) = if width >= height {
        let (border, remainder) = split_axis(rect.width);
        (
            Rect {
                width: border,
                ..rect
            },
            Rect {
                x: rect.x + border,
                width: remainder,
                ..rect
            },
        )
    } else {
        let (border, remainder) = split_axis(rect.height);
        (
            Rect {
                height: border,
                ..rect
            },
            Rect {
                y: rect.y + border,
                height: remainder,
                ..rect
            },
        )
    };
    partition(&weights[..split], left, first, viewport, tiles);
    partition(&weights[split..], sum - left, second, viewport, tiles);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handles_empty_zero_invalid_and_single_inputs() {
        assert!(layout(std::iter::empty(), 1, 100.0, 100.0).is_empty());
        assert!(layout([(0, 0)].into_iter(), 1, 100.0, 100.0).is_empty());
        for size in [0., -1., f64::INFINITY, f64::NAN] {
            assert!(layout([(0, 1)].into_iter(), 1, size, 50.0).is_empty());
        }
        let tiles = layout([(5, u64::MAX)].into_iter(), 0, 0.001, 0.001);
        assert_eq!((tiles[0].id, tiles[0].width, tiles[0].height), (5, 1., 1.));
        assert!(!tiles[0].label);
    }

    #[test]
    fn tiles_are_deterministic_disjoint_bounded_and_proportional() {
        let weights = (0..400).map(|index| {
            (
                index,
                if index % 3 == 0 {
                    u64::MAX
                } else {
                    1 + index as u64
                },
            )
        });
        let mut greater = weights.clone().collect::<Vec<_>>();
        greater.sort_unstable_by_key(|&(id, weight)| (std::cmp::Reverse(weight), id));
        let total: u128 = greater.iter().map(|&(_, size)| u128::from(size)).sum();
        let other: u128 = greater
            .iter()
            .skip(TILE_LIMIT - 1)
            .map(|&(_, size)| u128::from(size))
            .sum();
        for (w, h) in [(540., 250.), (250., 540.), (0.1, 0.01)] {
            let output = layout(weights.clone(), usize::MAX, w, h);
            let reversed = layout(weights.clone().rev(), usize::MAX, w, h);
            assert_eq!(output.len(), TILE_LIMIT);
            for (index, tile) in output.iter().enumerate() {
                assert_eq!(tile.id, reversed[index].id);
                assert_eq!(tile.width, reversed[index].width);
                assert!(tile.width >= 0. && tile.height >= 0. && tile.x >= 0. && tile.y >= 0.);
                assert!(tile.x + tile.width <= 1. + 1e-12 && tile.y + tile.height <= 1. + 1e-12);
                let size = if tile.id == usize::MAX {
                    other
                } else {
                    u128::from(weights.clone().nth(tile.id).unwrap().1)
                };
                assert!((tile.width * tile.height - size as f64 / total as f64).abs() < 1e-12);
                for rhs in &output[index + 1..] {
                    let x = (tile.x + tile.width).min(rhs.x + rhs.width) - tile.x.max(rhs.x);
                    let y = (tile.y + tile.height).min(rhs.y + rhs.height) - tile.y.max(rhs.y);
                    assert!(x <= 1e-12 || y <= 1e-12);
                }
            }
        }
    }
}
