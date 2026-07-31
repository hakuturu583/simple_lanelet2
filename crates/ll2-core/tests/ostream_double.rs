//! Pins `fmt::ostream_double` against the reference implementation.
//!
//! `tests/data/ostream_double.tsv` holds `<f64 bits in hex>\t<expected text>` pairs
//! captured by rendering `Point3d(0, v, 0, 0)` with the real `lanelet2==1.2.3` and
//! extracting the formatted double. Bit patterns rather than decimal literals, so
//! nothing is lost to a round-trip.
//!
//! Regenerate with the snippet in `docs/PORTING_NOTES.md` if the reference version
//! ever changes.

use ll2_core::fmt::ostream_double;

#[test]
fn matches_reference_output_for_every_captured_vector() {
    let table = include_str!("data/ostream_double.tsv");

    let mut checked = 0usize;
    let mut failures = Vec::new();

    for (lineno, line) in table.lines().enumerate() {
        let line = line.trim_end_matches('\r');
        if line.is_empty() {
            continue;
        }
        let (bits, expected) = line
            .split_once('\t')
            .unwrap_or_else(|| panic!("line {}: expected <hex>\\t<expected>", lineno + 1));
        let value = f64::from_bits(
            u64::from_str_radix(bits, 16)
                .unwrap_or_else(|e| panic!("line {}: bad hex {bits:?}: {e}", lineno + 1)),
        );

        let actual = ostream_double(value);
        if actual != expected {
            failures.push(format!(
                "  {bits} ({value:e}): expected {expected:?}, got {actual:?}"
            ));
        }
        checked += 1;
    }

    assert!(
        checked > 1000,
        "fixture looks truncated: only {checked} vectors"
    );
    assert!(
        failures.is_empty(),
        "{} of {checked} vectors mismatched:\n{}",
        failures.len(),
        failures
            .iter()
            .take(30)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
}
