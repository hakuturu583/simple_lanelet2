//! Which argument types each geometry function accepts.
//!
//! Upstream registers these through Boost.Python's overload chain, and the
//! registered sets are narrower than they look: `area` takes only a
//! `ConstHybridPolygon2d`, `boundingBox2d` has no overload for a `LineString3d`,
//! and `length` rejects the hybrids. Calling one with an unregistered type raises
//! `ArgumentError` rather than doing something reasonable.
//!
//! These tables were measured against the reference implementation by trying every
//! type combination, not read off the C++ — the registrations are spread across
//! macros and are easy to misread.
//!
//! Names are normalised by dropping a leading `Const`, because Boost implicitly
//! converts a mutable primitive to its const counterpart wherever one is expected.

/// The class name with any leading `Const` removed.
pub fn normalise(class_name: &str) -> &str {
    class_name.strip_prefix("Const").unwrap_or(class_name)
}

/// Whether a single-argument call is registered for this type.
pub fn accepts_unary(allowed: &[&str], class_name: &str) -> bool {
    allowed.contains(&normalise(class_name))
}

/// Whether a two-argument call is registered for this pair.
pub fn accepts_binary(allowed: &[(&str, &str)], first: &str, second: &str) -> bool {
    let (first, second) = (normalise(first), normalise(second));
    allowed.iter().any(|&(a, b)| a == first && b == second)
}

/// Types `boundingBox2d` is registered for.
pub const BOUNDING_BOX_2D: &[&str] = &[
    "Area",
    "CompoundLineString2d",
    "HybridLineString2d",
    "HybridPolygon2d",
    "Lanelet",
    "LineString2d",
    "Point2d",
    "Polygon2d",
];

/// Types `boundingBox3d` is registered for.
pub const BOUNDING_BOX_3D: &[&str] = &[
    "Area",
    "CompoundLineString3d",
    "HybridLineString3d",
    "HybridPolygon3d",
    "Lanelet",
    "LineString3d",
    "Point3d",
    "Polygon3d",
];

/// Types `length` is registered for.
pub const LENGTH: &[&str] = &[
    "CompoundLineString2d",
    "CompoundLineString3d",
    "LineString2d",
    "LineString3d",
];

/// Types `area` is registered for.
pub const AREA: &[&str] = &[
    "HybridPolygon2d",
];

/// Types `interpolatedPointAtDistance` is registered for.
pub const INTERPOLATED_POINT_AT_DISTANCE: &[&str] = &[
    "CompoundLineString2d",
    "CompoundLineString3d",
    "LineString2d",
    "LineString3d",
];

/// Types `nearestPointAtDistance` is registered for.
pub const NEAREST_POINT_AT_DISTANCE: &[&str] = &[
    "CompoundLineString2d",
    "CompoundLineString3d",
    "LineString2d",
    "LineString3d",
];

/// Type pairs `distance` is registered for.
pub const DISTANCE: &[(&str, &str)] = &[
    ("Area", "BasicPoint2d"),
    ("Area", "BasicPoint3d"),
    ("BasicPoint2d", "Area"),
    ("BasicPoint2d", "BasicPoint2d"),
    ("BasicPoint2d", "CompoundLineString2d"),
    ("BasicPoint2d", "Lanelet"),
    ("BasicPoint2d", "LineString2d"),
    ("BasicPoint2d", "Point2d"),
    ("BasicPoint3d", "Area"),
    ("BasicPoint3d", "BasicPoint3d"),
    ("BasicPoint3d", "Lanelet"),
    ("BasicPoint3d", "Point3d"),
    ("CompoundLineString2d", "BasicPoint2d"),
    ("CompoundLineString2d", "CompoundLineString2d"),
    ("CompoundLineString2d", "LineString2d"),
    ("CompoundLineString2d", "Point2d"),
    ("CompoundLineString2d", "Polygon2d"),
    ("CompoundLineString3d", "CompoundLineString3d"),
    ("CompoundLineString3d", "LineString3d"),
    ("CompoundLineString3d", "Point3d"),
    ("CompoundPolygon2d", "Polygon2d"),
    ("HybridLineString2d", "HybridLineString2d"),
    ("HybridLineString2d", "HybridPolygon2d"),
    ("HybridLineString2d", "Point2d"),
    ("HybridLineString3d", "HybridLineString3d"),
    ("HybridLineString3d", "Point3d"),
    ("HybridPolygon2d", "BasicPoint2d"),
    ("HybridPolygon2d", "HybridLineString2d"),
    ("HybridPolygon2d", "HybridPolygon2d"),
    ("HybridPolygon2d", "Polygon2d"),
    ("Lanelet", "BasicPoint2d"),
    ("Lanelet", "BasicPoint3d"),
    ("Lanelet", "Lanelet"),
    ("LineString2d", "BasicPoint2d"),
    ("LineString2d", "CompoundLineString2d"),
    ("LineString2d", "LineString2d"),
    ("LineString2d", "Point2d"),
    ("LineString2d", "Polygon2d"),
    ("LineString3d", "CompoundLineString3d"),
    ("LineString3d", "LineString3d"),
    ("LineString3d", "Point3d"),
    ("Point2d", "BasicPoint2d"),
    ("Point2d", "CompoundLineString2d"),
    ("Point2d", "HybridLineString2d"),
    ("Point2d", "LineString2d"),
    ("Point2d", "Point2d"),
    ("Point3d", "BasicPoint3d"),
    ("Point3d", "CompoundLineString3d"),
    ("Point3d", "HybridLineString3d"),
    ("Point3d", "LineString3d"),
    ("Point3d", "Point3d"),
    ("Polygon2d", "BasicPoint2d"),
    ("Polygon2d", "CompoundLineString2d"),
    ("Polygon2d", "HybridPolygon2d"),
    ("Polygon2d", "LineString2d"),
    ("Polygon2d", "Point2d"),
    ("Polygon2d", "Polygon2d"),
];

/// Type pairs `equals` is registered for.
pub const EQUALS: &[(&str, &str)] = &[
    ("BasicPoint2d", "BasicPoint2d"),
    ("BasicPoint2d", "Point2d"),
    ("BasicPoint3d", "BasicPoint3d"),
    ("BasicPoint3d", "Point3d"),
    ("Point2d", "BasicPoint2d"),
    ("Point2d", "Point2d"),
    ("Point3d", "BasicPoint3d"),
    ("Point3d", "Point3d"),
];

/// Type pairs `intersects2d` is registered for.
pub const INTERSECTS_2D: &[(&str, &str)] = &[
    ("Area", "Area"),
    ("BoundingBox3d", "BoundingBox3d"),
    ("CompoundLineString2d", "CompoundLineString2d"),
    ("HybridLineString2d", "HybridLineString2d"),
    ("HybridPolygon2d", "HybridPolygon2d"),
    ("Lanelet", "Lanelet"),
    ("LineString2d", "LineString2d"),
    ("Polygon2d", "Polygon2d"),
];

/// Type pairs `intersects3d` accepts when a height tolerance is given explicitly.
///
/// The linestring overloads leave the tolerance without a default, so they are
/// unreachable from a two-argument call.
pub const INTERSECTS_3D_WITH_TOLERANCE: &[(&str, &str)] = &[
    ("Area", "Area"),
    ("BoundingBox3d", "BoundingBox3d"),
    ("CompoundLineString3d", "CompoundLineString3d"),
    ("HybridLineString3d", "HybridLineString3d"),
    ("Lanelet", "Lanelet"),
    ("LineString3d", "LineString3d"),
];

/// Type pairs `intersects3d` is registered for.
pub const INTERSECTS_3D: &[(&str, &str)] = &[
    ("BoundingBox3d", "BoundingBox3d"),
    ("Lanelet", "Lanelet"),
];

/// Type pairs `inside` is registered for.
pub const INSIDE: &[(&str, &str)] = &[
    ("Lanelet", "BasicPoint2d"),
];

/// Type pairs `overlaps2d` is registered for.
pub const OVERLAPS_2D: &[(&str, &str)] = &[
    ("Lanelet", "Lanelet"),
];

/// Type pairs `overlaps3d` is registered for.
pub const OVERLAPS_3D: &[(&str, &str)] = &[
    ("Lanelet", "Lanelet"),
];

/// Type pairs `leftOf` is registered for.
pub const LEFT_OF: &[(&str, &str)] = &[
    ("Lanelet", "Lanelet"),
];

/// Type pairs `rightOf` is registered for.
pub const RIGHT_OF: &[(&str, &str)] = &[
    ("Lanelet", "Lanelet"),
];

/// Type pairs `follows` is registered for.
pub const FOLLOWS: &[(&str, &str)] = &[
    ("Lanelet", "Lanelet"),
];

/// Type pairs `intersectCenterlines2d` is registered for.
pub const INTERSECT_CENTERLINES_2D: &[(&str, &str)] = &[
    ("Lanelet", "Lanelet"),
];

/// Type pairs `intersection` is registered for.
pub const INTERSECTION: &[(&str, &str)] = &[
    ("CompoundLineString2d", "CompoundLineString2d"),
    ("CompoundLineString2d", "LineString2d"),
    ("LineString2d", "LineString2d"),
];

/// Type pairs `project` is registered for.
pub const PROJECT: &[(&str, &str)] = &[
    ("CompoundLineString2d", "BasicPoint2d"),
    ("CompoundLineString3d", "BasicPoint3d"),
    ("LineString2d", "BasicPoint2d"),
    ("LineString3d", "BasicPoint3d"),
];

/// Type pairs `toArcCoordinates` is registered for.
pub const TO_ARC_COORDINATES: &[(&str, &str)] = &[
    ("CompoundLineString2d", "BasicPoint2d"),
    ("LineString2d", "BasicPoint2d"),
];

/// Type pairs `distanceToCenterline2d` is registered for.
pub const DISTANCE_TO_CENTERLINE_2D: &[(&str, &str)] = &[
    ("Lanelet", "BasicPoint2d"),
];

/// Type pairs `distanceToCenterline3d` is registered for.
pub const DISTANCE_TO_CENTERLINE_3D: &[(&str, &str)] = &[
    ("Lanelet", "BasicPoint3d"),
];

/// Type pairs `projectedPoint3d` is registered for.
pub const PROJECTED_POINT_3D: &[(&str, &str)] = &[
    ("CompoundLineString3d", "CompoundLineString3d"),
    ("HybridLineString3d", "HybridLineString3d"),
    ("LineString3d", "LineString3d"),
];

