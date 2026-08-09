//! Reading and writing Lanelet2 maps.
//!
//! Only the OSM XML format. Upstream also supports a `.bin` format, but that is a
//! Boost.Serialization archive whose layout depends on the compiler and Boost
//! version — there is nothing stable to target, so asking for it is an error rather
//! than a silently different format.
//!
//! Errors are collected rather than thrown as they occur: a file with one broken
//! way should still yield the rest of the map, which is what `loadRobust` is for.
//!
//! Upstream: `lanelet2_io/src/{Io,OsmHandlerLoad,OsmHandlerWrite}.cpp`

pub mod load;
pub mod osm;
pub mod save;

use std::path::Path;

use ll2_core::map::LaneletMap;
use ll2_projection::Projector;

pub use osm::WriteParams;

/// Everything that can go wrong at the file level.
#[derive(Debug)]
pub enum IoError {
    FileNotFound(String),
    UnsupportedExtension(String),
    Parse(String),
    Write(String),
}

impl IoError {
    pub fn message(&self) -> String {
        match self {
            IoError::FileNotFound(path) => format!("Could not open file: {path}"),
            IoError::UnsupportedExtension(extension) => {
                format!("Unsupported file extension: {extension}")
            }
            IoError::Parse(message) | IoError::Write(message) => message.clone(),
        }
    }
}

impl std::fmt::Display for IoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message())
    }
}

fn check_extension(path: &Path) -> Result<(), IoError> {
    match path.extension().and_then(|e| e.to_str()) {
        Some("osm") => Ok(()),
        Some("bin") => Err(IoError::UnsupportedExtension(
            "Binary maps use a Boost.Serialization archive, whose layout is not \
             portable between builds; only .osm is supported"
                .into(),
        )),
        Some(other) => Err(IoError::UnsupportedExtension(format!(".{other}"))),
        None => Err(IoError::UnsupportedExtension("(none)".into())),
    }
}

/// Loads a map, collecting any problems rather than failing on the first.
pub fn load_robust(
    path: &Path,
    projector: &dyn Projector,
) -> Result<(std::sync::Arc<LaneletMap>, Vec<String>), IoError> {
    check_extension(path)?;
    if !path.exists() {
        return Err(IoError::FileNotFound(path.display().to_string()));
    }
    // A georeferenced file read through a default-built origin would silently give
    // meaningless coordinates, so upstream refuses rather than guessing.
    if projector.origin().is_default {
        return Err(IoError::Parse(
            "You must pass an origin when loading a map with georeferenced (lat/lon) data!".into(),
        ));
    }
    let text = std::fs::read_to_string(path)
        .map_err(|e| IoError::FileNotFound(format!("{}: {e}", path.display())))?;
    load_str_robust(&text, projector)
}

/// Loads a map from OSM XML already in memory, collecting any problems rather than
/// failing on the first.
///
/// The same thing [`load_robust`] does once it has the bytes, minus the filesystem —
/// which is what a caller with the document in hand needs, and the only form
/// available at all on a target with no filesystem to speak of, such as `wasm32`.
///
/// The origin check that [`load_robust`] makes is deliberately *not* repeated here:
/// a caller holding the text can inspect it and pick an origin from the file itself,
/// which is exactly what the visualiser does.
pub fn load_str_robust(
    text: &str,
    projector: &dyn Projector,
) -> Result<(std::sync::Arc<LaneletMap>, Vec<String>), IoError> {
    let (document, mut errors) = osm::parse(text).map_err(IoError::Parse)?;
    let (map, load_errors) = load::to_map(&document, projector);
    errors.extend(load_errors);
    Ok((map, wrap_errors(PARSE_ERROR_HEADER, errors)))
}

/// Loads a map from OSM XML already in memory, failing if anything went wrong.
pub fn load_str(
    text: &str,
    projector: &dyn Projector,
) -> Result<std::sync::Arc<LaneletMap>, IoError> {
    let (map, errors) = load_str_robust(text, projector)?;
    if errors.is_empty() {
        return Ok(map);
    }
    Err(IoError::Parse(join_errors(&errors)))
}

/// Loads a map, failing if anything went wrong.
pub fn load(path: &Path, projector: &dyn Projector) -> Result<std::sync::Arc<LaneletMap>, IoError> {
    let (map, errors) = load_robust(path, projector)?;
    if errors.is_empty() {
        return Ok(map);
    }
    Err(IoError::Parse(join_errors(&errors)))
}

/// Writes a map, collecting any problems rather than failing on the first.
pub fn write_robust(
    path: &Path,
    map: &LaneletMap,
    projector: &dyn Projector,
    params: WriteParams,
) -> Result<Vec<String>, IoError> {
    check_extension(path)?;
    let (document, errors) = save::from_map(map, projector);
    std::fs::write(path, document.to_xml(params))
        .map_err(|e| IoError::Write(format!("{}: {e}", path.display())))?;
    Ok(wrap_errors(
        "Errors ocurred while writing Lanelet Map:",
        errors,
    ))
}

/// Writes a map, failing if anything went wrong.
pub fn write(
    path: &Path,
    map: &LaneletMap,
    projector: &dyn Projector,
    params: WriteParams,
) -> Result<(), IoError> {
    let errors = write_robust(path, map, projector, params)?;
    if errors.is_empty() {
        return Ok(());
    }
    Err(IoError::Write(join_errors(&errors)))
}

/// The header `loadRobust` puts on a non-empty problem list. The misspelling is
/// upstream's, and is part of the output a caller compares against.
pub const PARSE_ERROR_HEADER: &str = "Errors ocurred while parsing Lanelet Map:";

/// Prefixes a header and bullets each entry, in place, leaving an empty list alone.
///
/// `loadRobust` hands this list back verbatim, and the exception `load` raises is
/// exactly these lines joined — measured from the reference, which reports
/// `['Errors ocurred while parsing Lanelet Map:', '\t- Error parsing primitive 20: …']`
/// (the misspelling is upstream's).
pub fn wrap_errors(header: &str, errors: Vec<String>) -> Vec<String> {
    if errors.is_empty() {
        return errors;
    }
    let mut out = Vec::with_capacity(errors.len() + 1);
    out.push(header.to_owned());
    out.extend(errors.into_iter().map(|error| format!("\t- {error}")));
    out
}

/// The message form of an already-wrapped error list. Note the trailing newline.
fn join_errors(errors: &[String]) -> String {
    let mut out = errors.join("\n");
    out.push('\n');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_osm_is_accepted() {
        assert!(check_extension(Path::new("map.osm")).is_ok());
        let error = check_extension(Path::new("map.bin")).unwrap_err();
        assert!(error.message().contains("Boost.Serialization"));
        assert!(check_extension(Path::new("map.txt")).is_err());
        assert!(check_extension(Path::new("map")).is_err());
    }

    #[test]
    fn errors_are_bulleted_under_the_upstream_header() {
        let wrapped = wrap_errors("Header:", vec!["one".into(), "two".into()]);
        assert_eq!(wrapped, ["Header:", "\t- one", "\t- two"]);
        // The exception form is the same lines joined, with a trailing newline —
        // measured from the reference, not inferred.
        assert_eq!(join_errors(&wrapped), "Header:\n\t- one\n\t- two\n");
    }

    #[test]
    fn an_empty_error_list_gains_no_header() {
        assert!(wrap_errors("Header:", Vec::new()).is_empty());
    }

    /// The in-memory entry point has to reach the same map as the file one, since
    /// the only difference between them is where the bytes came from.
    #[test]
    fn a_map_loads_from_a_string_without_touching_the_filesystem() {
        let text = std::fs::read_to_string("../../tests/data/mapping_example.osm").unwrap();
        let origin = ll2_projection::Origin::new(ll2_projection::GpsPoint::new(49.0, 8.4, 0.0));
        let projector = ll2_projection::Utm::new(origin, true, false).unwrap();

        let from_string = load_str(&text, &projector).unwrap();
        let from_file = load(
            Path::new("../../tests/data/mapping_example.osm"),
            &projector,
        )
        .unwrap();

        assert!(!from_string.lanelets.is_empty());
        assert_eq!(from_string.lanelets.len(), from_file.lanelets.len());
        assert_eq!(from_string.points.len(), from_file.points.len());
    }
}
