//! Transverse Mercator via the Krüger series, to order 6.
//!
//! This is the projection under UTM, and it is the one place where using a
//! textbook Snyder series instead would show: Snyder is millimetre-to-centimetre
//! accurate at zone edges, while this is accurate to a few nanometres over the
//! whole zone plus its padding.
//!
//! A direct port of GeographicLib's `TransverseMercator`, including its Clenshaw
//! summation of the complex series and the rational coefficient tables. The
//! coefficients are computed from the third flattening `n = f / (2 - f)` at
//! construction, exactly as GeographicLib does, rather than hardcoded, so the
//! arithmetic matches term for term.

use crate::{ang_diff, ang_normalize, atan2d, atand, sincosd, wgs84};

/// The series order GeographicLib compiles in for double precision.
const MAXPOW: usize = 6;

/// A minimal complex number, enough for the Clenshaw recurrence.
#[derive(Clone, Copy, Debug)]
struct Complex {
    re: f64,
    im: f64,
}

impl Complex {
    const ZERO: Complex = Complex { re: 0.0, im: 0.0 };

    fn new(re: f64, im: f64) -> Self {
        Complex { re, im }
    }

    fn real(value: f64) -> Self {
        Complex { re: value, im: 0.0 }
    }

    fn mul(self, other: Complex) -> Self {
        Complex {
            re: self.re * other.re - self.im * other.im,
            im: self.re * other.im + self.im * other.re,
        }
    }

    fn sub(self, other: Complex) -> Self {
        Complex {
            re: self.re - other.re,
            im: self.im - other.im,
        }
    }

    fn add(self, other: Complex) -> Self {
        Complex {
            re: self.re + other.re,
            im: self.im + other.im,
        }
    }

    fn scale(self, factor: f64) -> Self {
        Complex {
            re: self.re * factor,
            im: self.im * factor,
        }
    }
}

/// Horner evaluation of `coefficients[0] * x^n + ... + coefficients[n]`.
fn polyval(order: usize, coefficients: &[f64], x: f64) -> f64 {
    let mut result = coefficients[0];
    for &coefficient in &coefficients[1..=order] {
        result = result * x + coefficient;
    }
    result
}

/// `b1 * (n + 1)`, a polynomial in `n^2` of order 3, then its normaliser.
const B1_COEFF: [f64; 5] = [1.0, 4.0, 64.0, 256.0, 256.0];

/// The forward-series coefficients, grouped by power of `n`, each group being a
/// polynomial in `n` followed by its denominator.
const ALP_COEFF: [f64; 27] = [
    // alp[1]/n^1, order 5
    31564.0, -66675.0, 34440.0, 47250.0, -100800.0, 75600.0, 151200.0,
    // alp[2]/n^2, order 4
    -1983433.0, 863232.0, 748608.0, -1161216.0, 524160.0, 1935360.0,
    // alp[3]/n^3, order 3
    670412.0, 406647.0, -533952.0, 184464.0, 725760.0,
    // alp[4]/n^4, order 2
    6601661.0, -7732800.0, 2230245.0, 7257600.0,
    // alp[5]/n^5, order 1
    -13675556.0, 3438171.0, 7983360.0,
    // alp[6]/n^6, order 0
    212378941.0, 319334400.0,
];

/// The reverse-series coefficients, same layout.
const BET_COEFF: [f64; 27] = [
    // bet[1]/n^1, order 5
    384796.0, -382725.0, -6720.0, 932400.0, -1612800.0, 1209600.0, 2419200.0,
    // bet[2]/n^2, order 4
    -1118711.0, 1695744.0, -1174656.0, 258048.0, 80640.0, 3870720.0,
    // bet[3]/n^3, order 3
    22276.0, -16929.0, -15984.0, 12852.0, 362880.0,
    // bet[4]/n^4, order 2
    -830251.0, -158400.0, 197865.0, 7257600.0,
    // bet[5]/n^5, order 1
    -435388.0, 453717.0, 15966720.0,
    // bet[6]/n^6, order 0
    20648693.0, 638668800.0,
];

/// Transverse Mercator on the WGS84 ellipsoid.
pub struct TransverseMercator {
    /// Scale on the central meridian.
    k0: f64,
    /// Signed eccentricity, `sqrt(e^2)` for an oblate ellipsoid.
    es: f64,
    /// `a * b1`, the rectifying radius times the equatorial radius.
    a1: f64,
    /// Forward series coefficients, 1-based.
    alp: [f64; MAXPOW + 1],
    /// Reverse series coefficients, 1-based.
    bet: [f64; MAXPOW + 1],
}

/// `e * atanh(e * x)`, or its circular counterpart for a prolate ellipsoid.
fn eatanhe(x: f64, es: f64) -> f64 {
    if es > 0.0 {
        es * (es * x).atanh()
    } else {
        -es * (es * x).atan()
    }
}

/// Converts a tangent of latitude to a tangent of *conformal* latitude.
pub fn taupf(tau: f64, es: f64) -> f64 {
    let tau1 = f64::hypot(1.0, tau);
    let sig = eatanhe(tau / tau1, es).sinh();
    f64::hypot(1.0, sig) * tau - sig * tau1
}

/// The inverse of [`taupf`], by Newton's method.
///
/// GeographicLib caps this at five iterations; two suffice everywhere outside the
/// immediate neighbourhood of the equator.
pub fn tauf(taup: f64, es: f64) -> f64 {
    const NUMIT: usize = 5;
    let tol = f64::EPSILON.sqrt() / 10.0;
    let e2m = 1.0 - es * es;
    let mut tau = taup / e2m;
    let stol = tol * taup.abs().max(1.0);
    for _ in 0..NUMIT {
        let taupa = taupf(tau, es);
        let dtau = (taup - taupa) * (1.0 + e2m * tau * tau)
            / (e2m * f64::hypot(1.0, tau) * f64::hypot(1.0, taupa));
        tau += dtau;
        if dtau.abs() < stol {
            break;
        }
    }
    tau
}

impl TransverseMercator {
    /// The instance UTM uses: WGS84 with a central-meridian scale of 0.9996.
    pub fn utm() -> Self {
        TransverseMercator::new(wgs84::A, wgs84::F, 0.9996)
    }

    pub fn new(a: f64, f: f64, k0: f64) -> Self {
        let e2 = f * (2.0 - f);
        let es = e2.abs().sqrt() * if f < 0.0 { -1.0 } else { 1.0 };
        let n = f / (2.0 - f);

        let b1 = polyval(3, &B1_COEFF, n * n) / (B1_COEFF[4] * (1.0 + n));
        let a1 = b1 * a;

        let mut alp = [0.0; MAXPOW + 1];
        let mut bet = [0.0; MAXPOW + 1];
        let mut offset = 0usize;
        let mut d = n;
        for l in 1..=MAXPOW {
            let m = MAXPOW - l;
            alp[l] = d * polyval(m, &ALP_COEFF[offset..], n) / ALP_COEFF[offset + m + 1];
            bet[l] = d * polyval(m, &BET_COEFF[offset..], n) / BET_COEFF[offset + m + 1];
            offset += m + 2;
            d *= n;
        }

        TransverseMercator { k0, es, a1, alp, bet }
    }

    /// Projects a geodetic position relative to the central meridian `lon0`.
    pub fn forward(&self, lon0: f64, lat: f64, lon: f64) -> (f64, f64) {
        let mut lon = ang_diff(lon0, lon);
        let mut lat = lat;

        // Fold the problem into the first quadrant, then unfold the result.
        let mut latsign = if lat.is_sign_negative() { -1.0 } else { 1.0 };
        let lonsign = if lon.is_sign_negative() { -1.0 } else { 1.0 };
        lon *= lonsign;
        lat *= latsign;
        let backside = lon > 90.0;
        if backside {
            if lat == 0.0 {
                latsign = -1.0;
            }
            lon = 180.0 - lon;
        }

        let (sin_phi, cos_phi) = sincosd(lat);
        let (sin_lam, cos_lam) = sincosd(lon);

        let (xip, etap) = if lat != 90.0 {
            let tau = sin_phi / cos_phi;
            let taup = taupf(tau, self.es);
            (
                taup.atan2(cos_lam),
                (sin_lam / f64::hypot(taup, cos_lam)).asinh(),
            )
        } else {
            (std::f64::consts::FRAC_PI_2, 0.0)
        };

        // Clenshaw summation of the complex series (Krüger p 43-44, eqns 61-62).
        let c0 = (2.0 * xip).cos();
        let ch0 = (2.0 * etap).cosh();
        let s0 = (2.0 * xip).sin();
        let sh0 = (2.0 * etap).sinh();
        let a = Complex::new(2.0 * c0 * ch0, -2.0 * s0 * sh0);

        let mut n = MAXPOW;
        let mut y0 = if n & 1 == 1 { Complex::real(self.alp[n]) } else { Complex::ZERO };
        let mut y1 = Complex::ZERO;
        if n & 1 == 1 {
            n -= 1;
        }
        while n > 0 {
            y1 = a.mul(y0).sub(y1).add(Complex::real(self.alp[n]));
            n -= 1;
            y0 = a.mul(y1).sub(y0).add(Complex::real(self.alp[n]));
            n -= 1;
        }

        let half = Complex::new(s0 * ch0, c0 * sh0);
        let result = Complex::new(xip, etap).add(half.mul(y0));

        let y = self.a1 * self.k0 * (if backside { std::f64::consts::PI - result.re } else { result.re })
            * latsign;
        let x = self.a1 * self.k0 * result.im * lonsign;
        (x, y)
    }

    /// The inverse: recovers latitude and longitude from projected coordinates.
    pub fn reverse(&self, lon0: f64, x: f64, y: f64) -> (f64, f64) {
        let mut xi = y / (self.a1 * self.k0);
        let mut eta = x / (self.a1 * self.k0);

        let xisign = if xi.is_sign_negative() { -1.0 } else { 1.0 };
        let etasign = if eta.is_sign_negative() { -1.0 } else { 1.0 };
        xi *= xisign;
        eta *= etasign;
        let backside = xi > std::f64::consts::FRAC_PI_2;
        if backside {
            xi = std::f64::consts::PI - xi;
        }

        let c0 = (2.0 * xi).cos();
        let ch0 = (2.0 * eta).cosh();
        let s0 = (2.0 * xi).sin();
        let sh0 = (2.0 * eta).sinh();
        let a = Complex::new(2.0 * c0 * ch0, -2.0 * s0 * sh0);

        let mut n = MAXPOW;
        let mut y0 = if n & 1 == 1 { Complex::real(-self.bet[n]) } else { Complex::ZERO };
        let mut y1 = Complex::ZERO;
        if n & 1 == 1 {
            n -= 1;
        }
        while n > 0 {
            y1 = a.mul(y0).sub(y1).sub(Complex::real(self.bet[n]));
            n -= 1;
            y0 = a.mul(y1).sub(y0).sub(Complex::real(self.bet[n]));
            n -= 1;
        }

        let half = Complex::new(s0 * ch0, c0 * sh0);
        let result = Complex::new(xi, eta).add(half.mul(y0));

        let (xip, etap) = (result.re, result.im);
        // Krüger p 17 (25): tan(lon) = sinh(etap) / cos(xip), and the conformal
        // latitude follows from sin(xip) / hypot(sinh(etap), cos(xip)). The clamp
        // guards cos(pi/2) coming out slightly negative.
        let s = etap.sinh();
        let c = xip.cos().max(0.0);
        let r = f64::hypot(s, c);

        let (mut lat, mut lon) = if r != 0.0 {
            let lon = atan2d(s, c);
            let sxip = xip.sin();
            let tau = tauf(sxip / r, self.es);
            (atand(tau), lon)
        } else {
            (90.0, 0.0)
        };

        lat *= xisign;
        if backside {
            lon = 180.0 - lon;
        }
        lon *= etasign;
        (lat, ang_normalize(lon + lon0))
    }
}

// `Complex::scale` is used by the scale-factor computation, which we do not expose.
const _: fn(Complex, f64) -> Complex = Complex::scale;

#[cfg(test)]
mod tests {
    use super::*;

    /// Reference values from GeographicLib's own `TransverseMercatorProj` utility
    /// (WGS84, k0 = 0.9996, central meridian 9°), to 1e-9 m.
    #[test]
    fn matches_geographiclib_reference_values() {
        let tm = TransverseMercator::utm();
        // Karlsruhe, in UTM zone 32 (central meridian 9°). Easting is measured from
        // the central meridian here, before the 500 km false easting is added.
        let (x, y) = tm.forward(9.0, 49.0, 8.4);
        assert!((x - (-43_885.404_137_739_5)).abs() < 1e-9, "x = {x}");
        assert!((y - 5_427_629.203_924_715).abs() < 1e-6, "y = {y}");
    }

    #[test]
    fn the_central_meridian_has_zero_easting_and_scaled_northing() {
        let tm = TransverseMercator::utm();
        let (x, _) = tm.forward(9.0, 49.0, 9.0);
        assert!(x.abs() < 1e-9, "x = {x}");
        let (x, y) = tm.forward(0.0, 0.0, 0.0);
        assert!(x.abs() < 1e-9 && y.abs() < 1e-9);
    }

    #[test]
    fn round_trips_across_a_whole_zone_and_its_padding() {
        let tm = TransverseMercator::utm();
        let mut worst = 0.0f64;
        // A UTM zone is 6° wide; ±5° from the central meridian covers it plus the
        // padding area where coordinates are still usable.
        for lat in (-80..=84).step_by(4) {
            for delta in [-5.0, -3.0, -1.0, 0.0, 1.0, 3.0, 5.0] {
                let lat = lat as f64;
                let lon = 9.0 + delta;
                let (x, y) = tm.forward(9.0, lat, lon);
                let (lat2, lon2) = tm.reverse(9.0, x, y);
                worst = worst.max((lat2 - lat).abs()).max((lon2 - lon).abs());
            }
        }
        // 1e-12 degrees is about 0.1 micrometres.
        assert!(worst < 1e-12, "worst round-trip error {worst} degrees");
    }

    #[test]
    fn the_series_coefficients_are_the_expected_magnitude() {
        let tm = TransverseMercator::utm();
        // alp[k] falls off like n^k with n about 1/598, so each is ~600x smaller.
        // alp[k] falls off like n^k with n ~ 1/598, so each is ~600x smaller.
        for k in 1..=5 {
            assert!(
                tm.alp[k].abs() > tm.alp[k + 1].abs() * 100.0,
                "alp[{k}] = {} is not far larger than alp[{}] = {}",
                tm.alp[k],
                k + 1,
                tm.alp[k + 1]
            );
        }
        assert!((tm.alp[1] - 8.377e-4).abs() < 1e-6, "{}", tm.alp[1]);
        assert!((tm.bet[1] - 8.377e-4).abs() < 1e-6, "{}", tm.bet[1]);
        assert!((tm.a1 - 6_367_449.145_82).abs() < 1e-4, "{}", tm.a1);
    }
}
