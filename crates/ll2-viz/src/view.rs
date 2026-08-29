//! Where the map is looked at from.
//!
//! A [`Scene`](crate::Scene) is three-dimensional: every vertex keeps the elevation
//! its node carried, because a Lanelet2 map has one and an overpass that draws at
//! the same height as the road under it is a map that has lost something. A page and
//! a `<canvas>`, though, are flat — so somewhere between the two, three coordinates
//! have to become two. That is this module, and it is the only place it happens.
//!
//! The default, [`View::plan`], is the map view every 2D renderer already drew:
//! straight down, x east, y north, elevation discarded. [`View::oblique`] tilts the
//! camera off the vertical, and then elevation is what separates a bridge from the
//! road beneath it.
//!
//! The projection is *orthographic* — parallel lines stay parallel and a metre is a
//! metre wherever it is on the page. That is the right choice for a map rather than
//! for a game: a perspective view makes the far side of a junction smaller than the
//! near side, and a viewer measuring a road off the screen would be wrong. It also
//! keeps the projection affine, which is what lets a bounding box be projected by
//! its corners and a renderer keep its one screen transform.
//!
//! Two conventions worth stating, because every renderer downstream relies on them:
//!
//! * The projection leaves screen x alone — it is the map's own east-west axis,
//!   rotated by the yaw but never foreshortened — so a scale bar drawn along it is
//!   still telling the truth at any tilt. Screen y is the axis that is compressed.
//! * The result is in *metres*, in the same y-up frame a plan view produces. Nothing
//!   here knows about pixels, and a renderer still flips y for itself.

use ll2_core::geometry::bbox::{BoundingBox2d, BoundingBox3d};

use crate::polyline::Point3;

/// Metres of relief below which tilting the camera is not worth offering.
///
/// A Lanelet2 map is not required to carry `ele`, and plenty do not. Tilting one of
/// those gives a flat sheet at an angle, which reads as a broken renderer rather
/// than as a map with nothing to show — so the answer is stated once, here, and
/// every caller that offers a 3D view asks [`worth_tilting`] rather than inventing
/// a threshold of its own.
pub const MIN_USEFUL_RELIEF: f64 = 0.5;

/// Whether a map with this much relief, in metres, has anything to show in 3D.
pub fn worth_tilting(relief: f64) -> bool {
    relief.is_finite() && relief >= MIN_USEFUL_RELIEF
}

/// The direction the map is seen from.
///
/// Cheap to copy and to pass around: the trigonometry is done once, when the view is
/// built, rather than once per vertex of a city.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    yaw: f64,
    pitch: f64,
    exaggeration: f64,
    cos_yaw: f64,
    sin_yaw: f64,
    /// The two products every projected vertex needs, folded together here so the
    /// hot loop is a multiply-add rather than two multiplies and a lookup: how far
    /// up the page a metre of elevation moves a point, and how much nearer to the
    /// viewer it brings it.
    lift: f64,
    sink: f64,
    /// How much of the map's own northing survives onto the page, and into depth.
    sin_pitch: f64,
    cos_pitch: f64,
    plan: bool,
}

impl Default for View {
    fn default() -> Self {
        View::plan()
    }
}

impl View {
    /// Straight down: the map view, and what every renderer here drew before there
    /// was a choice. Elevation is discarded rather than merely flattened, so this is
    /// bit-for-bit the old output and not a tilt of nearly zero.
    pub fn plan() -> View {
        View {
            yaw: 0.0,
            pitch: 90.0,
            exaggeration: 1.0,
            cos_yaw: 1.0,
            sin_yaw: 0.0,
            // Never read — every consumer takes the `plan` branch first — but kept
            // consistent so the derived `PartialEq` agrees with `oblique(0, 90, 1)`.
            lift: 0.0,
            sink: 1.0,
            sin_pitch: 1.0,
            cos_pitch: 0.0,
            plan: true,
        }
    }

    /// A camera at a compass bearing and an angle above the horizon.
    ///
    /// * `yaw` turns the map about the vertical, in degrees. Zero puts north up.
    /// * `pitch` is degrees above the horizon: 90 looks straight down and is
    ///   [`View::plan`]; 0 is edge-on, where a flat map collapses to a line and only
    ///   its elevation profile is left. It is clamped to `[1, 90]`, because a
    ///   negative pitch is the same view from underneath and reads as a bug.
    /// * `exaggeration` multiplies every elevation. Roads climb tens of metres over
    ///   cities kilometres wide, so honest relief is often invisible; this is the
    ///   knob that makes it legible, and `1.0` is the truth. Zero flattens the map
    ///   onto a tilted plane, which is the way to ask for the tilt without trusting
    ///   the file's `ele` tags.
    pub fn oblique(yaw: f64, pitch: f64, exaggeration: f64) -> View {
        let yaw = if yaw.is_finite() {
            yaw.rem_euclid(360.0)
        } else {
            0.0
        };
        let pitch = if pitch.is_finite() {
            pitch.clamp(1.0, 90.0)
        } else {
            90.0
        };
        let exaggeration = if exaggeration.is_finite() {
            exaggeration.clamp(0.0, 100.0)
        } else {
            1.0
        };
        // A pitch of exactly 90 with no yaw *is* the plan view, and saying so keeps
        // `cos(FRAC_PI_2)`'s 6e-17 out of coordinates that would otherwise be exact.
        if pitch >= 90.0 && yaw == 0.0 {
            return View::plan();
        }
        let (sin_yaw, cos_yaw) = yaw.to_radians().sin_cos();
        let (sin_pitch, cos_pitch) = pitch.to_radians().sin_cos();
        View {
            yaw,
            pitch,
            exaggeration,
            cos_yaw,
            sin_yaw,
            lift: exaggeration * cos_pitch,
            sink: exaggeration * sin_pitch,
            sin_pitch,
            cos_pitch,
            plan: false,
        }
    }

    /// A conventional three-quarter view: turned 30° and tilted to 55° above the
    /// horizon, which shows a junction's shape and its relief at the same time.
    /// A sensible thing for a "3D" button to mean.
    pub fn three_quarter() -> View {
        View::oblique(30.0, 55.0, 1.0)
    }

    pub fn yaw(&self) -> f64 {
        self.yaw
    }

    pub fn pitch(&self) -> f64 {
        self.pitch
    }

    pub fn exaggeration(&self) -> f64 {
        self.exaggeration
    }

    /// Whether this is the straight-down view, in which elevation changes nothing.
    ///
    /// Renderers check it to skip the work a plan view does not need: the projection
    /// per vertex, and the depth sort of shapes that cannot occlude each other.
    pub fn is_plan(&self) -> bool {
        self.plan
    }

    /// Map coordinates to drawing coordinates, in metres, y still up.
    pub fn project(&self, point: Point3) -> [f64; 2] {
        if self.plan {
            return [point[0], point[1]];
        }
        let east = point[0] * self.cos_yaw + point[1] * self.sin_yaw;
        let north = point[1] * self.cos_yaw - point[0] * self.sin_yaw;
        [east, north * self.sin_pitch + point[2] * self.lift]
    }

    /// How far a point is from the viewer, in metres along the line of sight.
    ///
    /// Larger is further away, so a painter's-algorithm renderer draws in decreasing
    /// order of it. Only differences matter: the camera has no position, being
    /// orthographic, so this is measured from an arbitrary plane through the origin.
    pub fn depth(&self, point: Point3) -> f64 {
        if self.plan {
            return -point[2];
        }
        let north = point[1] * self.cos_yaw - point[0] * self.sin_yaw;
        north * self.cos_pitch - point[2] * self.sink
    }

    /// The box a projected box occupies on the page.
    ///
    /// Exact, and eight projections rather than one per vertex: the projection is
    /// affine, so the image of a box is the hull of the images of its corners, and
    /// the bounding box of that hull is the bounding box of those eight points.
    pub fn project_bounds(&self, bounds: &BoundingBox3d) -> BoundingBox2d {
        let mut projected = BoundingBox2d::empty();
        if bounds.is_empty() {
            return projected;
        }
        if self.plan {
            projected.extend_point([bounds.min[0], bounds.min[1]]);
            projected.extend_point([bounds.max[0], bounds.max[1]]);
            return projected;
        }
        for corner in 0..8u8 {
            let pick = |axis: usize| {
                if corner >> axis & 1 == 0 {
                    bounds.min[axis]
                } else {
                    bounds.max[axis]
                }
            };
            projected.extend_point(self.project([pick(0), pick(1), pick(2)]));
        }
        projected
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_plan_view_is_exactly_the_old_flattening() {
        let view = View::plan();
        assert!(view.is_plan());
        assert_eq!(view.project([3.0, 4.0, 100.0]), [3.0, 4.0]);
        // Asked for as an oblique view, it is still recognised as the plan view —
        // otherwise "3D at 90 degrees" would perturb every coordinate by 1e-17.
        assert!(View::oblique(0.0, 90.0, 3.0).is_plan());
        assert_eq!(View::oblique(0.0, 90.0, 1.0), view);
    }

    #[test]
    fn tilting_lifts_elevation_up_the_page_and_leaves_east_alone() {
        let view = View::oblique(0.0, 60.0, 1.0);
        let ground = view.project([10.0, 20.0, 0.0]);
        let raised = view.project([10.0, 20.0, 5.0]);
        assert_eq!(ground[0], 10.0, "east is never foreshortened");
        assert_eq!(raised[0], 10.0);
        assert!(raised[1] > ground[1], "a raised point draws higher up");
        // sin 60 for the ground plane, cos 60 for the lift.
        assert!((ground[1] - 20.0 * 0.8660254).abs() < 1e-6, "{ground:?}");
        assert!((raised[1] - ground[1] - 5.0 * 0.5).abs() < 1e-6);
    }

    #[test]
    fn edge_on_leaves_only_the_elevation_profile() {
        let view = View::oblique(0.0, 1.0, 1.0);
        let flat = view.project([0.0, 500.0, 0.0])[1];
        let hill = view.project([0.0, 500.0, 40.0])[1];
        assert!(hill - flat > 39.0, "{flat} to {hill}");
    }

    #[test]
    fn yaw_turns_the_map_about_the_vertical() {
        let view = View::oblique(90.0, 90.0, 1.0);
        // Turned a quarter turn, north points along screen x.
        let projected = view.project([0.0, 10.0, 0.0]);
        assert!((projected[0] - 10.0).abs() < 1e-9, "{projected:?}");
        assert!(projected[1].abs() < 1e-9);
    }

    #[test]
    fn exaggeration_scales_relief_and_zero_flattens_it() {
        let honest = View::oblique(0.0, 45.0, 1.0);
        let stretched = View::oblique(0.0, 45.0, 4.0);
        let flat = View::oblique(0.0, 45.0, 0.0);
        let lift = |view: View| view.project([0.0, 0.0, 10.0])[1] - view.project([0.0; 3])[1];
        assert!((lift(stretched) - 4.0 * lift(honest)).abs() < 1e-9);
        assert_eq!(lift(flat), 0.0);
    }

    #[test]
    fn nearer_things_have_a_smaller_depth() {
        let view = View::oblique(0.0, 40.0, 1.0);
        // The camera is south of the map and above it, so a point further north or
        // lower down is further away.
        assert!(view.depth([0.0, 100.0, 0.0]) > view.depth([0.0, 0.0, 0.0]));
        assert!(view.depth([0.0, 0.0, 0.0]) > view.depth([0.0, 0.0, 5.0]));
        // Straight down, only elevation can separate two shapes.
        let plan = View::plan();
        assert!(plan.depth([0.0, 100.0, 0.0]) == plan.depth([0.0, 0.0, 0.0]));
        assert!(plan.depth([0.0, 0.0, 0.0]) > plan.depth([0.0, 0.0, 5.0]));
    }

    #[test]
    fn nonsense_parameters_do_not_produce_a_nonsense_camera() {
        let view = View::oblique(f64::NAN, -30.0, f64::INFINITY);
        assert_eq!(view.yaw(), 0.0);
        assert_eq!(view.pitch(), 1.0);
        assert_eq!(view.exaggeration(), 1.0);
        assert!(view.project([1.0, 2.0, 3.0]).iter().all(|v| v.is_finite()));
        // A yaw is an angle, so a full turn is no turn at all.
        assert_eq!(
            View::oblique(390.0, 45.0, 1.0),
            View::oblique(30.0, 45.0, 1.0)
        );
    }

    #[test]
    fn a_projected_box_covers_every_projected_corner() {
        let bounds = BoundingBox3d {
            min: [-10.0, -20.0, 0.0],
            max: [30.0, 40.0, 12.0],
        };
        let view = View::oblique(25.0, 50.0, 2.0);
        let projected = view.project_bounds(&bounds);
        // The eight corners written out, rather than re-deriving them the way
        // `project_bounds` does — a test that restates the code proves nothing.
        for corner in [
            [-10.0, -20.0, 0.0],
            [-10.0, -20.0, 12.0],
            [-10.0, 40.0, 0.0],
            [-10.0, 40.0, 12.0],
            [30.0, -20.0, 0.0],
            [30.0, -20.0, 12.0],
            [30.0, 40.0, 0.0],
            [30.0, 40.0, 12.0],
        ] {
            let point = view.project(corner);
            assert!(projected.contains(point), "{point:?} outside {projected:?}");
        }
        // Tight, not merely covering: an interior point cannot reach a boundary the
        // corners did not already touch.
        assert!(projected.contains(view.project([5.0, 0.0, 6.0])));
        // And the plan view of the box is the box itself.
        assert_eq!(
            View::plan().project_bounds(&bounds),
            BoundingBox2d {
                min: [-10.0, -20.0],
                max: [30.0, 40.0],
            }
        );
        assert!(
            View::plan()
                .project_bounds(&BoundingBox3d::empty())
                .is_empty()
        );
    }
}
