//! Writes a Lanelet2 `.osm` map as a plain OpenStreetMap road network.
//!
//! ```text
//! cargo run -p ll2-osm-roads --example ll2osm -- lanelet2_map.osm roads.osm [--oneway-ref=REF]
//! ```

use std::process::ExitCode;

use ll2_io::osm::{WriteParams, parse};
use ll2_osm_roads::{Options, to_road_osm};

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let positional: Vec<&String> = arguments.iter().filter(|a| !a.starts_with("--")).collect();
    let [input, output] = positional.as_slice() else {
        eprintln!("usage: ll2osm <lanelet2_map.osm> <roads.osm> [--oneway-ref=REF]");
        return ExitCode::FAILURE;
    };
    let options = Options {
        oneway_ref: arguments
            .iter()
            .find_map(|a| a.strip_prefix("--oneway-ref="))
            .map(str::to_string),
    };
    let text = match std::fs::read_to_string(input) {
        Ok(text) => text,
        Err(error) => {
            eprintln!("{input}: {error}");
            return ExitCode::FAILURE;
        }
    };
    let (document, errors) = match parse(&text) {
        Ok(parsed) => parsed,
        Err(error) => {
            eprintln!("{input}: {error}");
            return ExitCode::FAILURE;
        }
    };
    for error in &errors {
        eprintln!("warning: {error}");
    }
    let (roads, report) = match to_road_osm(&document, &options) {
        Ok(converted) => converted,
        Err(error) => {
            eprintln!("{input}: {error}");
            return ExitCode::FAILURE;
        }
    };
    if let Err(error) = std::fs::write(output, roads.to_xml(WriteParams::default())) {
        eprintln!("{output}: {error}");
        return ExitCode::FAILURE;
    }
    eprintln!("{report:?}");
    ExitCode::SUCCESS
}
