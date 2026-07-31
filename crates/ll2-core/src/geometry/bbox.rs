//! Axis-aligned bounding boxes.
//!
//! Upstream builds these by extending an empty Eigen `AlignedBox`, whose empty
//! state is `min = +inf`, `max = -inf`. That is worth reproducing: an empty box
//! must not behave like a box at the origin.
//!
//! Upstream: `lanelet2_core/include/lanelet2_core/geometry/BoundingBox.h`,
//! `lanelet2_core/include/lanelet2_core/geometry/impl/Lanelet.h:39-51`

use crate::area::Area;
use crate::compound::CompoundLineString;
use crate::lanelet::Lanelet;
use crate::linestring::LineString;
use crate::point::Point;

/// A 2D axis-aligned box, empty until extended.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BoundingBox2d {
    pub min: [f64; 2],
    pub max: [f64; 2],
}

/// A 3D axis-aligned box, empty until extended.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BoundingBox3d {
    pub min: [f64; 3],
    pub max: [f64; 3],
}

impl Default for BoundingBox2d {
    fn default() -> Self {
        BoundingBox2d::empty()
    }
}

impl Default for BoundingBox3d {
    fn default() -> Self {
        BoundingBox3d::empty()
    }
}

impl BoundingBox2d {
    pub fn empty() -> Self {
        BoundingBox2d {
            min: [f64::INFINITY; 2],
            max: [f64::NEG_INFINITY; 2],
        }
    }

    pub fn is_empty(&self) -> bool {
        (0..2).any(|axis| self.min[axis] > self.max[axis])
    }

    pub fn extend_point(&mut self, point: [f64; 2]) {
        for ((min, max), value) in self.min.iter_mut().zip(&mut self.max).zip(point) {
            *min = min.min(value);
            *max = max.max(value);
        }
    }

    pub fn extend(&mut self, other: &BoundingBox2d) {
        if other.is_empty() {
            return;
        }
        self.extend_point(other.min);
        self.extend_point(other.max);
    }

    /// Grows the box by `margin` on every side.
    pub fn inflated(&self, margin: f64) -> BoundingBox2d {
        if self.is_empty() {
            return *self;
        }
        BoundingBox2d {
            min: [self.min[0] - margin, self.min[1] - margin],
            max: [self.max[0] + margin, self.max[1] + margin],
        }
    }

    pub fn intersects(&self, other: &BoundingBox2d) -> bool {
        !self.is_empty()
            && !other.is_empty()
            && (0..2)
                .all(|axis| self.min[axis] <= other.max[axis] && other.min[axis] <= self.max[axis])
    }

    pub fn contains(&self, point: [f64; 2]) -> bool {
        !self.is_empty()
            && (0..2).all(|axis| point[axis] >= self.min[axis] && point[axis] <= self.max[axis])
    }

    /// Distance from a point to the box, zero when inside.
    pub fn distance_to(&self, point: [f64; 2]) -> f64 {
        if self.is_empty() {
            return f64::INFINITY;
        }
        let axis_gap = |axis: usize| {
            (self.min[axis] - point[axis])
                .max(point[axis] - self.max[axis])
                .max(0.0)
        };
        f64::hypot(axis_gap(0), axis_gap(1))
    }
}

impl BoundingBox3d {
    pub fn empty() -> Self {
        BoundingBox3d {
            min: [f64::INFINITY; 3],
            max: [f64::NEG_INFINITY; 3],
        }
    }

    pub fn is_empty(&self) -> bool {
        (0..3).any(|axis| self.min[axis] > self.max[axis])
    }

    pub fn extend_point(&mut self, point: [f64; 3]) {
        for ((min, max), value) in self.min.iter_mut().zip(&mut self.max).zip(point) {
            *min = min.min(value);
            *max = max.max(value);
        }
    }

    pub fn extend(&mut self, other: &BoundingBox3d) {
        if other.is_empty() {
            return;
        }
        self.extend_point(other.min);
        self.extend_point(other.max);
    }

    pub fn intersects(&self, other: &BoundingBox3d) -> bool {
        !self.is_empty()
            && !other.is_empty()
            && (0..3)
                .all(|axis| self.min[axis] <= other.max[axis] && other.min[axis] <= self.max[axis])
    }

    pub fn to_2d(&self) -> BoundingBox2d {
        if self.is_empty() {
            return BoundingBox2d::empty();
        }
        BoundingBox2d {
            min: [self.min[0], self.min[1]],
            max: [self.max[0], self.max[1]],
        }
    }
}

pub fn of_point_2d(point: &Point) -> BoundingBox2d {
    let [x, y, _] = point.xyz();
    BoundingBox2d {
        min: [x, y],
        max: [x, y],
    }
}

pub fn of_point_3d(point: &Point) -> BoundingBox3d {
    let xyz = point.xyz();
    BoundingBox3d { min: xyz, max: xyz }
}

pub fn of_points_2d(points: &[Point]) -> BoundingBox2d {
    let mut box2d = BoundingBox2d::empty();
    for point in points {
        let [x, y, _] = point.xyz();
        box2d.extend_point([x, y]);
    }
    box2d
}

pub fn of_points_3d(points: &[Point]) -> BoundingBox3d {
    let mut box3d = BoundingBox3d::empty();
    for point in points {
        box3d.extend_point(point.xyz());
    }
    box3d
}

pub fn of_linestring_2d(line: &LineString) -> BoundingBox2d {
    of_points_2d(&line.points())
}

pub fn of_linestring_3d(line: &LineString) -> BoundingBox3d {
    of_points_3d(&line.points())
}

pub fn of_compound_2d(compound: &CompoundLineString) -> BoundingBox2d {
    of_points_2d(&compound.points())
}

pub fn of_compound_3d(compound: &CompoundLineString) -> BoundingBox3d {
    of_points_3d(&compound.points())
}

/// A lanelet's box spans its two bounds. A custom centerline is deliberately not
/// included, matching upstream.
pub fn of_lanelet_2d(lanelet: &Lanelet) -> BoundingBox2d {
    let mut box2d = of_linestring_2d(&lanelet.left_bound());
    box2d.extend(&of_linestring_2d(&lanelet.right_bound()));
    box2d
}

pub fn of_lanelet_3d(lanelet: &Lanelet) -> BoundingBox3d {
    let mut box3d = of_linestring_3d(&lanelet.left_bound());
    box3d.extend(&of_linestring_3d(&lanelet.right_bound()));
    box3d
}

/// An area's box spans its outer ring and every hole.
pub fn of_area_2d(area: &Area) -> BoundingBox2d {
    let mut box2d = of_compound_2d(&area.outer_bound_polygon());
    for hole in area.inner_bound_polygons() {
        box2d.extend(&of_compound_2d(&hole));
    }
    box2d
}

pub fn of_area_3d(area: &Area) -> BoundingBox3d {
    let mut box3d = of_compound_3d(&area.outer_bound_polygon());
    for hole in area.inner_bound_polygons() {
        box3d.extend(&of_compound_3d(&hole));
    }
    box3d
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::attribute::AttributeMap;

    fn point(x: f64, y: f64) -> Point {
        Point::new(1, x, y, 0.0, AttributeMap::new())
    }

    #[test]
    fn an_empty_box_is_inverted_not_zero_sized() {
        let box2d = BoundingBox2d::empty();
        assert!(box2d.is_empty());
        assert!(!box2d.contains([0.0, 0.0]));
        assert!(!box2d.intersects(&BoundingBox2d::empty()));
        assert_eq!(of_points_2d(&[]), BoundingBox2d::empty());
    }

    #[test]
    fn extending_grows_to_cover_every_point() {
        let box2d = of_points_2d(&[point(1.0, 2.0), point(-3.0, 5.0), point(0.0, 0.0)]);
        assert_eq!(box2d.min, [-3.0, 0.0]);
        assert_eq!(box2d.max, [1.0, 5.0]);
        assert!(box2d.contains([0.0, 1.0]));
        assert!(box2d.contains([-3.0, 0.0]), "the boundary is inside");
        assert!(!box2d.contains([2.0, 1.0]));
    }

    #[test]
    fn distance_is_zero_inside_and_euclidean_outside() {
        let box2d = BoundingBox2d {
            min: [0.0, 0.0],
            max: [2.0, 2.0],
        };
        assert_eq!(box2d.distance_to([1.0, 1.0]), 0.0);
        assert_eq!(box2d.distance_to([3.0, 1.0]), 1.0);
        assert_eq!(box2d.distance_to([5.0, 6.0]), 5.0);
    }

    #[test]
    fn inflating_grows_every_side_but_leaves_an_empty_box_empty() {
        let box2d = BoundingBox2d {
            min: [0.0, 0.0],
            max: [2.0, 2.0],
        }
        .inflated(1.0);
        assert_eq!(box2d.min, [-1.0, -1.0]);
        assert_eq!(box2d.max, [3.0, 3.0]);
        assert!(BoundingBox2d::empty().inflated(1.0).is_empty());
    }
}
