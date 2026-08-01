//! MGRS grid references, and the projector the Autoware extension builds on them.
//!
//! An MGRS reference names a 100 km square and an offset inside it. Encoding is
//! mechanical. Decoding is not: the row letter repeats every 20 squares — every
//! 2000 km — so a reference on its own does not say which repetition is meant. The
//! latitude band letter resolves it, and getting that wrong shifts a whole map by
//! 2000 km, or by 100 km if the alphabet is off by one, **without anything
//! reporting an error**.
//!
//! GeographicLib resolves it with a closed-form row estimate plus a table of
//! exceptions. Rather than transcribe a table from memory, this decodes by search:
//! every candidate repetition is projected back and kept only if its latitude falls
//! in the band the letter names. That is self-checking — a wrong answer would have
//! to land in the right band — and `1250_aw_mgrs` compares it against the reference
//! across every band, both zone parities and all three column alphabets.
//!
//! Upstream: GeographicLib `MGRS`,
//! `autoware_lanelet2_extension/lib/mgrs_projector.cpp`

use std::sync::Mutex;

use crate::utmups;
use crate::{GpsPoint, Origin, ProjectionError, Projector};

/// One grid square, metres.
const TILE: f64 = 100_000.0;
/// The row alphabet repeats every twenty squares — 2000 km.
const ROW_PERIOD: usize = 20;
/// Even-numbered zones start their row alphabet five squares further along.
const EVEN_ROW_SHIFT: usize = 5;

/// Latitude band letters, from -80° upward in 8° steps. `I` and `O` are skipped.
const BANDS: &[u8] = b"CDEFGHJKLMNPQRSTUVWX";
/// Column letters, chosen by `(zone - 1) % 3`. `I` and `O` are skipped here too.
const COLUMNS: [&[u8]; 3] = [b"ABCDEFGH", b"JKLMNPQR", b"STUVWXYZ"];
/// Row letters.
const ROWS: &[u8] = b"ABCDEFGHJKLMNPQRSTUV";

/// The band letter a latitude falls in.
fn band_letter(lat: f64) -> u8 {
    let index = ((lat / 8.0).floor() as i32 + 10).clamp(0, 19) as usize;
    BANDS[index]
}

/// The latitude range a band letter covers, or `None` if it is not a band letter.
fn band_range(letter: u8) -> Option<(f64, f64)> {
    let index = BANDS.iter().position(|&b| b == letter)? as f64;
    let low = (index - 10.0) * 8.0;
    // The last band is stretched to 84° so that the grid reaches the end of UTM.
    let high = if index == 19.0 { 84.0 } else { low + 8.0 };
    Some((low, high))
}

/// Builds a grid reference from a position already in UTM.
///
/// `precision` counts digits per axis, 0..=5; 0 names the 100 km square alone.
pub fn forward(
    zone: i32,
    northp: bool,
    x: f64,
    y: f64,
    lat: f64,
    precision: usize,
) -> Result<String, ProjectionError> {
    let column = COLUMNS[((zone - 1) % 3) as usize];
    let column_index = (x / TILE).floor() as i64 - 1;
    if !(0..8).contains(&column_index) {
        return Err(ProjectionError::Forward(format!(
            "Easting {x} is outside the grid"
        )));
    }
    let row_index =
        ((y / TILE).floor() as i64).rem_euclid(ROW_PERIOD as i64) as usize;
    let row_index = (row_index + if zone % 2 == 0 { EVEN_ROW_SHIFT } else { 0 }) % ROW_PERIOD;

    // The zone is always two digits: the reference writes `01CER`, not `1CER`.
    let mut out = format!("{zone:02}{}", band_letter(lat) as char);
    out.push(column[column_index as usize] as char);
    out.push(ROWS[row_index] as char);
    if precision > 0 {
        let scale = 10f64.powi(precision as i32 - 5);
        let ex = ((x % TILE) * scale).floor() as i64;
        let ny = ((y % TILE) * scale).floor() as i64;
        out.push_str(&format!("{ex:0width$}{ny:0width$}", width = precision));
    }
    let _ = northp;
    Ok(out)
}

/// The south-west corner of the square a grid reference names, in UTM.
///
/// Returns `(zone, northp, x, y, precision)`.
pub fn reverse(code: &str) -> Result<(i32, bool, f64, f64, usize), ProjectionError> {
    let bytes = code.trim().to_ascii_uppercase().into_bytes();
    let digits = bytes.iter().take_while(|b| b.is_ascii_digit()).count();
    if digits == 0 || bytes.len() < digits + 3 {
        return Err(ProjectionError::Reverse(format!("Malformed MGRS code {code}")));
    }
    let zone: i32 = std::str::from_utf8(&bytes[..digits])
        .unwrap()
        .parse()
        .map_err(|_| ProjectionError::Reverse(format!("Bad zone in {code}")))?;
    if !(1..=60).contains(&zone) {
        return Err(ProjectionError::Reverse(format!("Zone {zone} is out of range")));
    }

    let band = bytes[digits];
    let (band_low, band_high) = band_range(band)
        .ok_or_else(|| ProjectionError::Reverse(format!("Bad band letter in {code}")))?;
    let northp = band_low >= 0.0;

    let column = COLUMNS[((zone - 1) % 3) as usize];
    let column_index = column
        .iter()
        .position(|&b| b == bytes[digits + 1])
        .ok_or_else(|| ProjectionError::Reverse(format!("Bad column letter in {code}")))?;
    let row_letter_index = ROWS
        .iter()
        .position(|&b| b == bytes[digits + 2])
        .ok_or_else(|| ProjectionError::Reverse(format!("Bad row letter in {code}")))?;
    let row_index = (row_letter_index + ROW_PERIOD
        - if zone % 2 == 0 { EVEN_ROW_SHIFT } else { 0 })
        % ROW_PERIOD;

    let tail = &bytes[digits + 3..];
    if tail.len() % 2 != 0 || tail.len() > 10 || !tail.iter().all(|b| b.is_ascii_digit()) {
        return Err(ProjectionError::Reverse(format!("Bad offset in {code}")));
    }
    let precision = tail.len() / 2;
    let (offset_x, offset_y) = if precision == 0 {
        (0.0, 0.0)
    } else {
        let half = std::str::from_utf8(tail).unwrap();
        let scale = 10f64.powi(5 - precision as i32);
        (
            half[..precision].parse::<f64>().unwrap() * scale,
            half[precision..].parse::<f64>().unwrap() * scale,
        )
    };

    let x = (column_index as f64 + 1.0) * TILE + offset_x;

    // The row letter repeats every 2000 km, so several northings spell the same
    // reference. Take the one whose latitude actually lands in the band the letter
    // names — the answer verifies itself rather than relying on a remembered table.
    // Southern references count down from the 10,000 km false northing, so the
    // search covers the whole UTM range either way.
    let mut best: Option<(f64, f64)> = None;
    for repetition in 0..=100 {
        let y = (row_index as f64 + (repetition * ROW_PERIOD) as f64) * TILE + offset_y;
        if y > 10_000_000.0 {
            break;
        }
        let Ok((lat, _)) = utmups::reverse(zone, northp, x, y) else {
            continue;
        };
        if lat < band_low || lat >= band_high {
            continue;
        }
        // Keep the first match: bands are 8° tall and the period is ~18°, so at most
        // one repetition can fall inside one.
        best = Some((y, lat));
        break;
    }
    let (y, _) = best.ok_or_else(|| {
        ProjectionError::Reverse(format!("No northing in band {} for {code}", band as char))
    })?;
    Ok((zone, northp, x, y, precision))
}

/// `MGRSProjector`, as the Autoware extension exposes it.
///
/// Two things about it are unusual and both are reproduced rather than tidied. The
/// origin is stored and then completely ignored — MGRS is origin-independent. And
/// nothing throws: a failure prints to stderr and yields a zero point, because that
/// is what the reference does and callers are written against it.
pub struct Mgrs {
    origin: Origin,
    /// The grid the last `forward` landed in, so that a `reverse` afterwards knows
    /// which square it is undoing. Interior mutability because `Projector` takes
    /// `&self`, and shared with every clone of the `Arc` the way upstream's
    /// `shared_ptr` is — a code latched during `io.load` is visible afterwards.
    projected_grid: Mutex<String>,
    /// A grid set explicitly through `setMGRSCode`, which wins over the latched one.
    code: Mutex<String>,
}

impl Mgrs {
    pub fn new(origin: Origin) -> Self {
        Mgrs {
            origin,
            projected_grid: Mutex::new(String::new()),
            code: Mutex::new(String::new()),
        }
    }

    pub fn set_code(&self, code: &str) {
        *self.code.lock().unwrap() = code.to_owned();
    }

    pub fn is_code_set(&self) -> bool {
        !self.code.lock().unwrap().is_empty()
    }

    pub fn projected_grid(&self) -> String {
        self.projected_grid.lock().unwrap().clone()
    }
}

impl Projector for Mgrs {
    fn forward(&self, point: GpsPoint) -> Result<[f64; 3], ProjectionError> {
        let projected = utmups::forward(point.lat, point.lon)
            .and_then(|(zone, northp, x, y)| {
                forward(zone, northp, x, y, point.lat, 0).map(|code| (code, x, y))
            });
        match projected {
            Err(error) => {
                eprintln!("Failed to project to MGRS: {error}");
                Ok([0.0, 0.0, point.ele])
            }
            Ok((code, x, y)) => {
                let mut grid = self.projected_grid.lock().unwrap();
                if !grid.is_empty() && *grid != code {
                    eprintln!("Projected MGRS Grid changed from {grid} to {code}");
                }
                *grid = code;
                Ok([x % TILE, y % TILE, point.ele])
            }
        }
    }

    fn reverse(&self, point: [f64; 3]) -> Result<GpsPoint, ProjectionError> {
        let code = {
            let explicit = self.code.lock().unwrap().clone();
            if explicit.is_empty() {
                self.projected_grid.lock().unwrap().clone()
            } else {
                explicit
            }
        };
        if code.is_empty() {
            eprintln!("MGRS code is not set");
            // Note the elevation is *not* carried through on this path, unlike the
            // one below. That asymmetry is upstream's.
            return Ok(GpsPoint::new(0.0, 0.0, 0.0));
        }
        match reverse(&code) {
            Err(error) => {
                eprintln!("Failed to reverse MGRS: {error}");
                Ok(GpsPoint::new(0.0, 0.0, 0.0))
            }
            Ok((zone, northp, x, y, precision)) => {
                let scale = TILE / 10f64.powi(precision as i32);
                let _ = scale;
                match utmups::reverse(zone, northp, x + point[0] % TILE, y + point[1] % TILE) {
                    Err(error) => {
                        eprintln!("Failed to reverse MGRS: {error}");
                        Ok(GpsPoint::new(0.0, 0.0, 0.0))
                    }
                    Ok((lat, lon)) => Ok(GpsPoint::new(lat, lon, point[2])),
                }
            }
        }
    }

    fn origin(&self) -> Origin {
        self.origin
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn band_letters_skip_i_and_o() {
        assert!(!BANDS.contains(&b'I') && !BANDS.contains(&b'O'));
        assert!(!ROWS.contains(&b'I') && !ROWS.contains(&b'O'));
        for set in COLUMNS {
            assert!(!set.contains(&b'I') && !set.contains(&b'O'));
        }
        assert_eq!(band_letter(-80.0), b'C');
        assert_eq!(band_letter(0.0), b'N');
        assert_eq!(band_letter(35.6895), b'S');
        assert_eq!(band_letter(83.0), b'X');
    }

    #[test]
    fn a_reference_round_trips_through_its_own_square() {
        for (lat, lon) in [
            (35.6895, 139.6917),
            (49.0, 8.4),
            (-33.0, 151.0),
            (0.0, 0.0),
            (78.0, 15.0),
            (-79.0, -60.0),
        ] {
            let (zone, northp, x, y) = utmups::forward(lat, lon).unwrap();
            let code = forward(zone, northp, x, y, lat, 0).unwrap();
            let (rzone, rnorthp, rx, ry, _) = reverse(&code).unwrap();
            assert_eq!((rzone, rnorthp), (zone, northp), "{code}");
            // The reference names the square's corner, so the original position is
            // within one tile of it.
            assert!((x - rx).abs() < TILE, "{code}: easting {x} vs {rx}");
            assert!((y - ry).abs() < TILE, "{code}: northing {y} vs {ry}");
        }
    }

    #[test]
    fn precision_digits_narrow_the_square() {
        let (zone, northp, x, y) = utmups::forward(35.6895, 139.6917).unwrap();
        for precision in 1..=5 {
            let code = forward(zone, northp, x, y, 35.6895, precision).unwrap();
            let (_, _, rx, ry, rprec) = reverse(&code).unwrap();
            assert_eq!(rprec, precision, "{code}");
            let cell = TILE / 10f64.powi(precision as i32);
            assert!((x - rx).abs() < cell, "{code}: easting off by {}", x - rx);
            assert!((y - ry).abs() < cell, "{code}: northing off by {}", y - ry);
        }
    }
}

/// Prints one grid code per line for a sweep of positions, so the same sweep can be
/// run against the reference and diffed. Used by `tools/mgrs_sweep.rs`-style checks.
pub fn sweep_codes() -> Vec<(f64, f64, String)> {
    let mut out = Vec::new();
    for band in 0..20 {
        let lat = (band as f64 - 10.0) * 8.0 + 4.0;
        if !(-80.0..84.0).contains(&lat) {
            continue;
        }
        for zone in 1..=60 {
            let lon = utmups::central_meridian(zone) + 1.5;
            if let Ok((z, northp, x, y)) = utmups::forward(lat, lon)
                && let Ok(code) = forward(z, northp, x, y, lat, 0)
            {
                out.push((lat, lon, code));
            }
        }
    }
    out
}
