//! Times building the German-vehicle routing graph of a map.
//!
//! ```text
//! cargo run --release -p ll2-routing --example build_graph -- map.osm
//! ```

use std::sync::Arc;
use std::time::Instant;

use ll2_routing::{RoutingGraph, default_costs};
use ll2_traffic_rules::{TrafficRules, locations, participants};
use ll2_viz::{LoadOptions, load_osm_str};

fn main() -> Result<(), String> {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let [path] = arguments.as_slice() else {
        return Err("usage: build_graph <map.osm>".to_owned());
    };

    let started = Instant::now();
    let text = std::fs::read_to_string(path).map_err(|error| format!("{path}: {error}"))?;
    let map = load_osm_str(&text, &LoadOptions::default())?.map;
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
    Ok(())
}
