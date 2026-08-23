//! The viewer layout: one Spatial3D view, looking at the map.
//!
//! Without a blueprint the viewer guesses, and its guess for a store holding eight
//! sibling entities is eight views. A blueprint is what makes opening an `.rrd`
//! written by this crate show a map rather than a contact sheet of one.

use rerun::blueprint::{Blueprint, Spatial3DView};

/// A blueprint with a single Spatial3D view over everything under `root`.
///
/// The view's origin is `root`, which is also where [`crate::MapLayers::log_to`]
/// puts the view coordinates — so the viewer knows z is up and frames the map with
/// north away from the camera rather than dropping it on its side.
///
/// Every layer stays a separate entity under that origin, which is what gives the
/// blueprint tree a checkbox per layer: boundaries, road surface, driving direction
/// and the rest can be switched off in the viewer exactly as they can be in the
/// options here.
pub fn spatial3d(root: &str) -> Blueprint {
    Blueprint::new(
        Spatial3DView::new("Lanelet2 map")
            .with_origin(root)
            .with_contents([format!("{root}/**")]),
    )
    // The layout is the point; the viewer must not add views of its own next to it.
    .with_auto_layout(false)
    .with_auto_views(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_blueprint_can_be_built_for_any_root() {
        // Nothing here reaches a viewer; building it is the check that the view
        // class and the origin are ones the SDK accepts.
        let _ = spatial3d("map");
        let _ = spatial3d("world/map");
    }
}
