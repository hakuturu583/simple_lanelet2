//! The UTM projector Lanelet2 maps normally use.
//!
//! The zone and hemisphere are fixed by the origin, not chosen per point: a map
//! that straddles a zone boundary stays in one coordinate frame. By default the
//! origin's own easting and northing are subtracted, so local coordinates are small
//! numbers near the map rather than six- and seven-digit UTM values.
//!
//! Upstream: `lanelet2_projection/src/UTM.cpp`

use crate::{GpsPoint, Origin, ProjectionError, Projector, utmups};

pub struct Utm {
    origin: Origin,
    zone: i32,
    northern: bool,
    use_offset: bool,
    throw_in_padding_area: bool,
    x_offset: f64,
    y_offset: f64,
}

impl Utm {
    pub fn new(
        origin: Origin,
        use_offset: bool,
        throw_in_padding_area: bool,
    ) -> Result<Self, ProjectionError> {
        let (zone, northern, x, y) = utmups::forward(origin.position.lat, origin.position.lon)?;
        Ok(Utm {
            origin,
            zone,
            northern,
            use_offset,
            throw_in_padding_area,
            x_offset: if use_offset { x } else { 0.0 },
            y_offset: if use_offset { y } else { 0.0 },
        })
    }

    pub fn zone(&self) -> i32 {
        self.zone
    }

    pub fn is_northern(&self) -> bool {
        self.northern
    }
}

impl Projector for Utm {
    fn forward(&self, point: GpsPoint) -> Result<[f64; 3], ProjectionError> {
        let (zone, northern, mut x, mut y) = utmups::forward(point.lat, point.lon)?;

        if zone != self.zone || northern != self.northern {
            if self.throw_in_padding_area {
                return Err(ProjectionError::Forward(
                    "You have left the UTM zone or changed the hemisphere!".into(),
                ));
            }
            let (moved_x, moved_y, zone_after) =
                utmups::transfer(zone, northern, x, y, self.zone, self.northern)?;
            if zone_after != self.zone {
                return Err(ProjectionError::Forward(
                    "You have left the padding area of the UTM zone!".into(),
                ));
            }
            x = moved_x;
            y = moved_y;
        }

        Ok([x - self.x_offset, y - self.y_offset, point.ele])
    }

    fn reverse(&self, point: [f64; 3]) -> Result<GpsPoint, ProjectionError> {
        let (lat, lon) = utmups::reverse(
            self.zone,
            self.northern,
            point[0] + self.x_offset,
            point[1] + self.y_offset,
        )?;
        let result = GpsPoint::new(lat, lon, point[2]);

        // Upstream re-runs the forward projection purely as a zone-compliance check.
        if self.throw_in_padding_area {
            self.forward(result)
                .map_err(|error| ProjectionError::Reverse(error.message().to_owned()))?;
        }
        Ok(result)
    }

    fn origin(&self) -> Origin {
        self.origin
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn karlsruhe() -> Origin {
        Origin::new(GpsPoint::new(49.0, 8.4, 0.0))
    }

    #[test]
    fn the_origin_maps_to_zero_when_offsetting() {
        let projector = Utm::new(karlsruhe(), true, false).unwrap();
        let projected = projector.forward(GpsPoint::new(49.0, 8.4, 0.0)).unwrap();
        assert!(
            projected[0].abs() < 1e-6 && projected[1].abs() < 1e-6,
            "{projected:?}"
        );
    }

    #[test]
    fn without_the_offset_the_result_is_a_real_utm_coordinate() {
        let projector = Utm::new(karlsruhe(), false, false).unwrap();
        let projected = projector.forward(GpsPoint::new(49.0, 8.4, 0.0)).unwrap();
        assert_eq!(projector.zone(), 32);
        // These are the reference implementation's own values for Karlsruhe.
        assert!(
            (projected[0] - 456_114.595_862_260_5).abs() < 1e-6,
            "{projected:?}"
        );
        assert!(
            (projected[1] - 5_427_629.203_924_715).abs() < 1e-6,
            "{projected:?}"
        );
    }

    #[test]
    fn elevation_passes_through_untouched() {
        let projector = Utm::new(karlsruhe(), true, false).unwrap();
        let projected = projector.forward(GpsPoint::new(49.0, 8.4, 123.5)).unwrap();
        assert_eq!(projected[2], 123.5);
    }

    #[test]
    fn a_point_in_the_next_zone_is_transferred_rather_than_rejected() {
        let projector = Utm::new(karlsruhe(), true, false).unwrap();
        // 13 degrees east is zone 33, but the projector stays in zone 32.
        let projected = projector.forward(GpsPoint::new(49.0, 13.0, 0.0)).unwrap();
        let back = projector.reverse(projected).unwrap();
        assert!((back.lat - 49.0).abs() < 1e-9, "{back:?}");
        assert!((back.lon - 13.0).abs() < 1e-9, "{back:?}");
    }

    #[test]
    fn leaving_the_zone_is_an_error_when_asked_for() {
        let projector = Utm::new(karlsruhe(), true, true).unwrap();
        let error = projector
            .forward(GpsPoint::new(49.0, 13.0, 0.0))
            .unwrap_err();
        assert!(error.message().contains("left the UTM zone"), "{error}");
    }

    #[test]
    fn round_trips_across_the_whole_zone() {
        let projector = Utm::new(karlsruhe(), true, false).unwrap();
        let mut worst = 0.0f64;
        for lat in [46.0, 48.0, 49.0, 50.0, 54.0] {
            for lon in [6.5, 8.0, 8.4, 9.0, 11.5] {
                let point = GpsPoint::new(lat, lon, 42.0);
                let back = projector
                    .reverse(projector.forward(point).unwrap())
                    .unwrap();
                worst = worst
                    .max((back.lat - point.lat).abs())
                    .max((back.lon - point.lon).abs());
            }
        }
        assert!(worst < 1e-11, "worst round-trip error {worst} degrees");
    }
}
