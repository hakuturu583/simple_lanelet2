//! C++ text formatting, reproduced exactly.
//!
//! Every `__repr__` and `__str__` in Lanelet2 renders its doubles through C++
//! `operator<<(std::ostream&, double)`, which is `printf("%g")` with the default
//! precision of 6 — so `0.0` prints as `0`, `1234567.0` as `1.23457e+06`, and
//! `1e-5` as `1e-05`. Rust's own `{}` and `{:e}` match none of that, and a single
//! wrong case would poison thousands of assertions across the whole library, so
//! this module is pinned by a 1700-vector table captured from the reference
//! implementation (`tests/ostream_double.rs`).

/// Default precision of a C++ output stream.
const DEFAULT_PRECISION: usize = 6;

/// Formats a double the way an unconfigured `std::ostream` does.
///
/// This is C's `%g` with precision 6: choose between fixed and scientific
/// notation by the decimal exponent, then strip trailing zeros from the
/// fractional part.
pub fn ostream_double(value: f64) -> String {
    format_g(value, DEFAULT_PRECISION)
}

/// `%g` with an explicit precision, per C99 7.19.6.1.
///
/// Let `p` be the precision (0 is treated as 1) and `x` the exponent of the value
/// rendered in scientific notation with `p - 1` fractional digits. If
/// `-4 <= x < p` the result is fixed-point with `p - 1 - x` fractional digits,
/// otherwise it is scientific with `p - 1` fractional digits. Either way trailing
/// zeros — and then a trailing decimal point — are removed.
fn format_g(value: f64, precision: usize) -> String {
    if value.is_nan() {
        // glibc distinguishes the sign of a NaN, and so does libstdc++.
        return if value.is_sign_negative() { "-nan".into() } else { "nan".into() };
    }
    if value.is_infinite() {
        return if value < 0.0 { "-inf".into() } else { "inf".into() };
    }

    let p = precision.max(1);
    let exponent = decimal_exponent(value, p);

    if exponent >= -4 && exponent < p as i32 {
        // Fixed notation. `p - 1 - exponent` is never negative here because
        // `exponent < p`, and Rust rounds `{:.*}` the same way glibc does.
        let digits = (p as i32 - 1 - exponent) as usize;
        strip_trailing_zeros(&format!("{value:.digits$}"))
    } else {
        let rendered = format!("{:.*e}", p - 1, value);
        let (mantissa, exp) = split_exponent(&rendered);
        format!("{}e{}", strip_trailing_zeros(mantissa), format_exponent(exp))
    }
}

/// The decimal exponent the value would have when rendered with `p` significant
/// digits — i.e. after rounding, so that 999999.5 at p=6 reports 6, not 5.
fn decimal_exponent(value: f64, p: usize) -> i32 {
    if value == 0.0 {
        return 0;
    }
    split_exponent(&format!("{:.*e}", p - 1, value)).1
}

/// Splits Rust's `{:e}` output into its mantissa and exponent parts.
///
/// Rust writes `1.23456e6` / `1.23456e-7`; C writes `1.23456e+06` / `1.23456e-07`.
fn split_exponent(rendered: &str) -> (&str, i32) {
    let at = rendered.rfind('e').expect("{:e} always emits an exponent");
    let exp = rendered[at + 1..].parse().expect("{:e} emits a valid exponent");
    (&rendered[..at], exp)
}

/// C renders exponents with a sign and at least two digits: `e+06`, `e-324`.
fn format_exponent(exp: i32) -> String {
    let sign = if exp < 0 { '-' } else { '+' };
    format!("{sign}{:02}", exp.unsigned_abs())
}

/// Removes trailing zeros from a fractional part, then a bare trailing point.
///
/// Leaves integers untouched, and leaves `-0.00000` as `-0` rather than `-`.
fn strip_trailing_zeros(text: &str) -> String {
    if !text.contains('.') {
        return text.to_owned();
    }
    let trimmed = text.trim_end_matches('0');
    trimmed.strip_suffix('.').unwrap_or(trimmed).to_owned()
}

/// Formats a double the way `std::to_string` does: `%f`, six decimal places.
///
/// Used by `ArcCoordinates.__repr__`, which is the one place upstream reaches for
/// `std::to_string` instead of a stream.
pub fn to_string_double(value: f64) -> String {
    if value.is_nan() {
        return if value.is_sign_negative() { "-nan".into() } else { "nan".into() };
    }
    if value.is_infinite() {
        return if value < 0.0 { "-inf".into() } else { "inf".into() };
    }
    format!("{value:.6}")
}

/// Builds a `Name(arg, arg, ...)` repr the way upstream's `makeRepr` does.
///
/// The quirk worth knowing: an argument that is an *empty* string is dropped
/// entirely — no comma, no empty slot. That is what turns a point with no
/// attributes into `Point3d(1000, 1, 2, 3)` rather than `Point3d(1000, 1, 2, 3, )`,
/// because the attribute-map argument renders as `""` when the map is empty.
/// The first argument is emitted verbatim even when empty.
///
/// Upstream: `lanelet2_python/include/lanelet2_python/internal/repr.h`
pub fn make_repr(name: &str, args: &[String]) -> String {
    let mut out = String::with_capacity(name.len() + 16);
    out.push_str(name);
    out.push('(');
    for (index, arg) in args.iter().enumerate() {
        if index == 0 {
            out.push_str(arg);
        } else if !arg.is_empty() {
            out.push_str(", ");
            out.push_str(arg);
        }
    }
    out.push(')');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spot_checks_from_the_reference_implementation() {
        let cases = [
            (0.0, "0"),
            (-0.0, "-0"),
            (1.0, "1"),
            (0.5, "0.5"),
            (1.0 / 3.0, "0.333333"),
            (2.0 / 3.0, "0.666667"),
            (1e-7, "1e-07"),
            (1e-5, "1e-05"),
            (1e-4, "0.0001"),
            (0.000123456789, "0.000123457"),
            (123456.0, "123456"),
            (123456.7, "123457"),
            (1234567.0, "1.23457e+06"),
            (999999.5, "1e+06"),
            (1234565.0, "1.23456e+06"),
            (1234575.0, "1.23458e+06"),
            (1e300, "1e+300"),
            (5e-324, "4.94066e-324"),
            (f64::INFINITY, "inf"),
            (f64::NEG_INFINITY, "-inf"),
            (f64::NAN, "nan"),
        ];
        for (value, expected) in cases {
            assert_eq!(ostream_double(value), expected, "formatting {value:?}");
        }
    }

    #[test]
    fn to_string_uses_six_decimal_places() {
        assert_eq!(to_string_double(1.0), "1.000000");
        assert_eq!(to_string_double(-0.5), "-0.500000");
        assert_eq!(to_string_double(0.0), "0.000000");
    }

    #[test]
    fn make_repr_drops_empty_arguments_after_the_first() {
        let args = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(make_repr("P", &args(&["1", "2"])), "P(1, 2)");
        assert_eq!(make_repr("P", &args(&["1", "", "2"])), "P(1, 2)");
        assert_eq!(make_repr("P", &args(&["1", "2", ""])), "P(1, 2)");
        assert_eq!(make_repr("P", &args(&["", "2"])), "P(, 2)");
        assert_eq!(make_repr("P", &[]), "P()");
    }
}
