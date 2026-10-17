//! Times building the German-vehicle routing graph of a map.
//!
//! ```text
//! cargo run --release -p ll2-routing --example build_graph -- map.osm LAT LON
//! ```

use std::path::Path;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Instant;

use ll2_projection::{GpsPoint, Origin, Utm};
use ll2_routing::{RoutingGraph, default_costs};
use ll2_traffic_rules::{TrafficRules, locations, participants};

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let [path, lat, lon] = arguments.as_slice() else {
        eprintln!("usage: build_graph <map.osm> <lat> <lon>");
        return ExitCode::FAILURE;
    };
    let (Ok(lat), Ok(lon)) = (lat.parse(), lon.parse()) else {
        eprintln!("the origin must be two numbers: {lat} {lon}");
        return ExitCode::FAILURE;
    };

    let projector = match Utm::new(Origin::new(GpsPoint::new(lat, lon, 0.0)), true, false) {
        Ok(projector) => projector,
        Err(error) => {
            eprintln!("no projector for that origin: {error}");
            return ExitCode::FAILURE;
        }
    };
    let started = Instant::now();
    let map = match ll2_io::load(Path::new(path), &projector) {
        Ok(map) => map,
        Err(error) => {
            eprintln!("could not load {path}: {error}");
            return ExitCode::FAILURE;
        }
    };
    println!(
        "load: {:.2?}, {} lanelets",
        started.elapsed(),
        map.lanelets.len()
    );

    let rules = Arc::new(
        TrafficRules::create(locations::GERMANY, participants::VEHICLE)
            .expect("German vehicle rules exist"),
    );
    let costs = default_costs();
    for _ in 0..3 {
        let started = Instant::now();
        let graph = RoutingGraph::build(&map, rules.clone(), &costs);
        println!(
            "build: {:.2?}, {} edges",
            started.elapsed(),
            graph.edges().len()
        );
    }
    ExitCode::SUCCESS
}
