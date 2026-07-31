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
    let (document, mut errors) = osm::parse(&text).map_err(IoError::Parse)?;
    let (map, load_errors) = load::to_map(&document, projector);
    errors.extend(load_errors);
    Ok((map, errors))
}

/// Loads a map, failing if anything went wrong.
pub fn load(path: &Path, projector: &dyn Projector) -> Result<std::sync::Arc<LaneletMap>, IoError> {
    let (map, errors) = load_robust(path, projector)?;
    if errors.is_empty() {
        return Ok(map);
    }
    Err(IoError::Parse(wrap_errors(
        "Errors ocurred while parsing Lanelet Map:",
        &errors,
    )))
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
    Ok(errors)
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
    Err(IoError::Write(wrap_errors(
        "Errors ocurred while writing Lanelet Map:",
        &errors,
    )))
}

/// Joins collected errors the way upstream's multi-error exceptions read.
fn wrap_errors(header: &str, errors: &[String]) -> String {
    let mut out = header.to_owned();
    for error in errors {
        out.push_str("\n\t- ");
        out.push_str(error);
    }
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
    fn errors_are_joined_with_the_upstream_header() {
        assert_eq!(
            wrap_errors("Header:", &["one".into(), "two".into()]),
            "Header:\n\t- one\n\t- two"
        );
    }
}
