//! Colour difference: an sRGB colour in CIE L*a*b*, and the CIEDE2000 difference between two
//! colours there, which matching a colour to a thread palette makes as small as it can
//! ([`crate::thread`]).
//!
//! RGB distance weighs the three channels alike, which the eye does not: two blues far apart in
//! RGB can look alike, and two yellows close in RGB can look unlike. CIE L*a*b* places colours by
//! how they look, and CIEDE2000 corrects much of what L*a*b* still gets wrong (greys and other
//! neutrals, saturated colours, and blues in part: a saturated blue can still match a purple, see
//! `docs/embroidery.md` › What is not stitched), so the thread nearest by CIEDE2000 is usually the
//! one that looks nearest. A difference of about 1 is the smallest seen side by side.
//!
//! Its scope: CIEDE2000 was fitted to small differences, and its standard and Sharma, Wu, Dalal
//! and Celik (below) advise keeping it to them: within about 5 CIELAB units its discontinuities
//! stay under 0.3, and past that they grow. Matching art to a palette of 61 or 78 threads works
//! well past that range (a median of 5 to 6 on the real designs, some blocks past 10), so the
//! ranking of threads far from a colour is less sure than that of near ones; the project owner
//! chose CIEDE2000 over OKLab all the same.
//!
//! References:
//! - sRGB: IEC 61966-2-1:1999, its transfer function and primaries; its white, CIE's D65.
//! - CIE L*a*b*: CIE 15:2004 *Colorimetry* (also ISO/CIE 11664-4).
//! - CIEDE2000: G. Sharma, W. Wu and E. N. Dalal, "The CIEDE2000 color-difference formula:
//!   implementation notes, supplementary test data, and mathematical observations", *Color
//!   Research and Application* 30(1), 21–30, 2005: equations (2) to (22), and the test data of its
//!   Table I (hajim.rochester.edu/ece/sites/gsharma/ciede2000/).
//! - Its range: G. Sharma, W. Wu, E. N. Dalal and M. U. Celik, "Mathematical discontinuities in
//!   CIEDE2000 color difference computations", *Color and Imaging Conference* 12, 2004
//!   (library.imaging.org/cic/articles/12/1/art00058).

/// A colour in CIE L*a*b*: lightness from 0 (black) to 100 (white), then green (negative) to red
/// (positive), then blue (negative) to yellow (positive).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Lab {
    pub l: f64,
    pub a: f64,
    pub b: f64,
}

// sRGB's transfer function (IEC 61966-2-1): an 8-bit channel over CHANNEL_MAX is linear below
// SRGB_LINEAR_LIMIT (divided by SRGB_LINEAR_SLOPE) and a power curve above it,
// ((v + SRGB_OFFSET) / (1 + SRGB_OFFSET)) ^ SRGB_GAMMA.

/// The largest value of an 8-bit channel.
const CHANNEL_MAX: f64 = 255.0;
/// Where sRGB's straight segment ends (encoded value).
const SRGB_LINEAR_LIMIT: f64 = 0.040_45;
/// The straight segment's slope.
const SRGB_LINEAR_SLOPE: f64 = 12.92;
/// The power curve's offset and exponent.
const SRGB_OFFSET: f64 = 0.055;
const SRGB_GAMMA: f64 = 2.4;

/// Linear sRGB to CIE XYZ, with white's Y = 1: the matrix of sRGB's primaries, red (0.64, 0.33),
/// green (0.30, 0.60) and blue (0.15, 0.06) (IEC 61966-2-1), scaled so that white comes out as
/// CIE's D65 white point for the 2° observer, X 0.95047, Y 1, Z 1.08883 (CIE 15:2004).
/// `the_srgb_matrix_comes_from_its_primaries` derives it again from those values.
const SRGB_TO_XYZ: [[f64; 3]; 3] =
    [[0.412_456_4, 0.357_576_1, 0.180_437_5], [0.212_672_9, 0.715_152_2, 0.072_175_0], [0.019_333_9, 0.119_192_0, 0.950_304_1]];

/// L*a*b*'s reference white, D65, as [`SRGB_TO_XYZ`] gives it: each of its rows summed (D65 to
/// the matrix's seven decimals), so that white is exactly a* = b* = 0.
const WHITE: [f64; 3] = [
    SRGB_TO_XYZ[0][0] + SRGB_TO_XYZ[0][1] + SRGB_TO_XYZ[0][2],
    SRGB_TO_XYZ[1][0] + SRGB_TO_XYZ[1][1] + SRGB_TO_XYZ[1][2],
    SRGB_TO_XYZ[2][0] + SRGB_TO_XYZ[2][1] + SRGB_TO_XYZ[2][2],
];

// CIE L*a*b* (CIE 15:2004): with f(t) the cube root of t above (6/29)³ and a straight line below
// it, L* = 116 f(Y/Yn) − 16, a* = 500 (f(X/Xn) − f(Y/Yn)) and b* = 200 (f(Y/Yn) − f(Z/Zn)).

/// f is a cube root above this value cubed (≈ 0.008856 of the white) and a straight line below,
/// t / (3 δ²) + 4/29, which meets the cube root there with the same slope.
const LAB_DELTA: f64 = 6.0 / 29.0;
/// L*'s scale and offset.
const LIGHTNESS_SCALE: f64 = 116.0;
const LIGHTNESS_OFFSET: f64 = 16.0;
/// a*'s and b*'s scales.
const A_SCALE: f64 = 500.0;
const B_SCALE: f64 = 200.0;

/// `rgb`, an 8-bit sRGB colour, in CIE L*a*b* (D65).
pub fn lab(rgb: [u8; 3]) -> Lab {
    let linear = rgb.map(|c| {
        let v = f64::from(c) / CHANNEL_MAX;
        if v <= SRGB_LINEAR_LIMIT { v / SRGB_LINEAR_SLOPE } else { ((v + SRGB_OFFSET) / (1.0 + SRGB_OFFSET)).powf(SRGB_GAMMA) }
    });
    let xyz = SRGB_TO_XYZ.map(|row| row[0] * linear[0] + row[1] * linear[1] + row[2] * linear[2]);
    let f = |t: f64| if t > LAB_DELTA.powi(3) { t.cbrt() } else { t / (3.0 * LAB_DELTA * LAB_DELTA) + LIGHTNESS_OFFSET / LIGHTNESS_SCALE };
    let [fx, fy, fz] = [0, 1, 2].map(|i| f(xyz[i] / WHITE[i]));
    Lab { l: LIGHTNESS_SCALE * fy - LIGHTNESS_OFFSET, a: A_SCALE * (fx - fy), b: B_SCALE * (fy - fz) }
}

// CIEDE2000 (Sharma, Wu and Dalal 2005; the equation numbers are theirs), with the parametric
// factors k_L = k_C = k_H = 1 of its reference conditions.

/// The chroma at which the stretch of a* for neutral colours (G, equation 4) and the rotation
/// term's weight (R_C, equation 17) are half way, taken to the [`CHROMA_POWER`]: 25.
const CHROMA_HALF: f64 = 25.0;
/// The power those two terms take of the chroma: 7.
const CHROMA_POWER: i32 = 7;
/// How much a* is stretched for a neutral colour, at most (G, equation 4).
const A_STRETCH: f64 = 0.5;
/// The hue weighting function T (equation 15) as its terms, each a coefficient, a multiple of the
/// mean hue and a phase in degrees: T = 1 − 0.17 cos(h̄′ − 30°) + 0.24 cos(2h̄′) + 0.32 cos(3h̄′ + 6°)
/// − 0.20 cos(4h̄′ − 63°).
const HUE_TERMS: [(f64, f64, f64); 4] = [(-0.17, 1.0, -30.0), (0.24, 2.0, 0.0), (0.32, 3.0, 6.0), (-0.20, 4.0, -63.0)];
/// The rotation of the blues (Δθ, equation 16): at most this many degrees, at this mean hue, over
/// this width of hue (degrees).
const ROTATION_PEAK: f64 = 30.0;
const ROTATION_HUE: f64 = 275.0;
const ROTATION_WIDTH: f64 = 25.0;
/// The rotation term's weight at high chroma (R_C, equation 17).
const ROTATION_WEIGHT: f64 = 2.0;
/// Lightness weighting (S_L, equation 18): 1 + 0.015 (L̄′ − 50)² / √(20 + (L̄′ − 50)²).
const LIGHTNESS_WEIGHT: f64 = 0.015;
const MID_LIGHTNESS: f64 = 50.0;
const LIGHTNESS_SOFTENING: f64 = 20.0;
/// Chroma weighting (S_C = 1 + 0.045 C̄′, equation 19) and hue weighting (S_H = 1 + 0.015 C̄′ T,
/// equation 20).
const CHROMA_WEIGHT: f64 = 0.045;
const HUE_WEIGHT: f64 = 0.015;
/// Half a turn and a whole one, degrees: hue differences and means wrap round (equations 10 and 14).
const HALF_TURN: f64 = 180.0;
const TURN: f64 = 360.0;

/// The CIEDE2000 colour difference ΔE₀₀ between `x` and `y`: 0 for the same colour, about 1 for
/// the smallest difference seen side by side. The same either way round. Not a number when either
/// colour has a coordinate that isn't a finite number (never one made by [`lab`]), so that it is
/// never taken for no difference.
pub fn delta_e(x: Lab, y: Lab) -> f64 {
    // How far a chroma `c` has come towards CHROMA_HALF and past it, 0 to 1 (equations 4 and 17).
    let saturation = |c: f64| {
        let p = c.powi(CHROMA_POWER);
        (p / (p + CHROMA_HALF.powi(CHROMA_POWER))).sqrt()
    };
    // (2)–(4): a* is stretched for colours of low chroma.
    let mean_chroma = (x.a.hypot(x.b) + y.a.hypot(y.b)) / 2.0;
    let g = A_STRETCH * (1.0 - saturation(mean_chroma));
    // (5)–(7): chroma and hue in degrees from 0 to 360 on the stretched axes; no hue (0) without
    // chroma.
    let polar = |c: Lab| {
        let a = (1.0 + g) * c.a;
        let hue = if a == 0.0 && c.b == 0.0 { 0.0 } else { c.b.atan2(a).to_degrees().rem_euclid(TURN) };
        (a.hypot(c.b), hue)
    };
    let ((c1, h1), (c2, h2)) = (polar(x), polar(y));
    let chromas = c1 * c2;
    // (8)–(11): the differences, signed; the hue's the shorter way round.
    let dl = y.l - x.l;
    let dc = c2 - c1;
    let dh = if chromas == 0.0 {
        0.0
    } else {
        match h2 - h1 {
            d if d > HALF_TURN => d - TURN,
            d if d < -HALF_TURN => d + TURN,
            d => d,
        }
    };
    let dh = 2.0 * chromas.sqrt() * (dh / 2.0).to_radians().sin();
    // (12)–(14): the means; the hue's the shorter way round. Where two hues lie half a turn apart,
    // the shorter way is either, so the mean hue, and ΔE₀₀ with it, jumps there: a discontinuity of
    // the formula itself, which Sharma, Wu and Dalal point out.
    let l = (x.l + y.l) / 2.0;
    let c = (c1 + c2) / 2.0;
    let h = if chromas == 0.0 {
        h1 + h2
    } else if (h1 - h2).abs() <= HALF_TURN {
        (h1 + h2) / 2.0
    } else if h1 + h2 < TURN {
        (h1 + h2 + TURN) / 2.0
    } else {
        (h1 + h2 - TURN) / 2.0
    };
    // (15)–(21): the weights, and the rotation that couples chroma and hue in the blues.
    let t = 1.0 + HUE_TERMS.iter().map(|&(k, n, phase)| k * (n * h + phase).to_radians().cos()).sum::<f64>();
    let rotation = ROTATION_PEAK * (-((h - ROTATION_HUE) / ROTATION_WIDTH).powi(2)).exp();
    let rc = ROTATION_WEIGHT * saturation(c);
    let lm = (l - MID_LIGHTNESS).powi(2);
    let sl = 1.0 + LIGHTNESS_WEIGHT * lm / (LIGHTNESS_SOFTENING + lm).sqrt();
    let sc = 1.0 + CHROMA_WEIGHT * c;
    let sh = 1.0 + HUE_WEIGHT * c * t;
    let rt = -(2.0 * rotation).to_radians().sin() * rc;
    // (22). The sum is never below 0 but by rounding, which is taken as 0; a sum that is not a
    // number stays one (`max(0.0)` would make it 0, no difference at all).
    let (dl, dc, dh) = (dl / sl, dc / sc, dh / sh);
    let squared = dl * dl + dc * dc + dh * dh + rt * dc * dh;
    if squared < 0.0 { 0.0 } else { squared.sqrt() }
}

/// The CIEDE2000 difference between two 8-bit sRGB colours ([`delta_e`] of their [`lab`]).
pub fn delta_e_rgb(x: [u8; 3], y: [u8; 3]) -> f64 {
    delta_e(lab(x), lab(y))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The CIEDE2000 test data of Sharma, Wu and Dalal (2005), Table I, as their
    /// `ciede2000testdata.txt` lists it: L*, a*, b* of the first colour, of the second, and ΔE₀₀
    /// to four decimals. Pairs 1–6 test signed differences, 7–16 the hue's arctangent and mean,
    /// 17–34 colours across the space.
    const SHARMA: [[f64; 7]; 34] = [
        [50.0000, 2.6772, -79.7751, 50.0000, 0.0000, -82.7485, 2.0425],
        [50.0000, 3.1571, -77.2803, 50.0000, 0.0000, -82.7485, 2.8615],
        [50.0000, 2.8361, -74.0200, 50.0000, 0.0000, -82.7485, 3.4412],
        [50.0000, -1.3802, -84.2814, 50.0000, 0.0000, -82.7485, 1.0000],
        [50.0000, -1.1848, -84.8006, 50.0000, 0.0000, -82.7485, 1.0000],
        [50.0000, -0.9009, -85.5211, 50.0000, 0.0000, -82.7485, 1.0000],
        [50.0000, 0.0000, 0.0000, 50.0000, -1.0000, 2.0000, 2.3669],
        [50.0000, -1.0000, 2.0000, 50.0000, 0.0000, 0.0000, 2.3669],
        [50.0000, 2.4900, -0.0010, 50.0000, -2.4900, 0.0009, 7.1792],
        [50.0000, 2.4900, -0.0010, 50.0000, -2.4900, 0.0010, 7.1792],
        [50.0000, 2.4900, -0.0010, 50.0000, -2.4900, 0.0011, 7.2195],
        [50.0000, 2.4900, -0.0010, 50.0000, -2.4900, 0.0012, 7.2195],
        [50.0000, -0.0010, 2.4900, 50.0000, 0.0009, -2.4900, 4.8045],
        [50.0000, -0.0010, 2.4900, 50.0000, 0.0010, -2.4900, 4.8045],
        [50.0000, -0.0010, 2.4900, 50.0000, 0.0011, -2.4900, 4.7461],
        [50.0000, 2.5000, 0.0000, 50.0000, 0.0000, -2.5000, 4.3065],
        [50.0000, 2.5000, 0.0000, 73.0000, 25.0000, -18.0000, 27.1492],
        [50.0000, 2.5000, 0.0000, 61.0000, -5.0000, 29.0000, 22.8977],
        [50.0000, 2.5000, 0.0000, 56.0000, -27.0000, -3.0000, 31.9030],
        [50.0000, 2.5000, 0.0000, 58.0000, 24.0000, 15.0000, 19.4535],
        [50.0000, 2.5000, 0.0000, 50.0000, 3.1736, 0.5854, 1.0000],
        [50.0000, 2.5000, 0.0000, 50.0000, 3.2972, 0.0000, 1.0000],
        [50.0000, 2.5000, 0.0000, 50.0000, 1.8634, 0.5757, 1.0000],
        [50.0000, 2.5000, 0.0000, 50.0000, 3.2592, 0.3350, 1.0000],
        [60.2574, -34.0099, 36.2677, 60.4626, -34.1751, 39.4387, 1.2644],
        [63.0109, -31.0961, -5.8663, 62.8187, -29.7946, -4.0864, 1.2630],
        [61.2901, 3.7196, -5.3901, 61.4292, 2.2480, -4.9620, 1.8731],
        [35.0831, -44.1164, 3.7933, 35.0232, -40.0716, 1.5901, 1.8645],
        [22.7233, 20.0904, -46.6940, 23.0331, 14.9730, -42.5619, 2.0373],
        [36.4612, 47.8580, 18.3852, 36.2715, 50.5065, 21.2231, 1.4146],
        [90.8027, -2.0831, 1.4410, 91.1528, -1.6435, 0.0447, 1.4441],
        [90.9257, -0.5406, -0.9208, 88.6381, -0.8985, -0.7239, 1.5381],
        [6.7747, -0.2908, -2.4247, 5.8714, -0.0985, -2.2286, 0.6377],
        [2.0776, 0.0795, -1.1350, 0.9033, -0.0636, -0.5514, 0.9082],
    ];

    /// The table's ΔE₀₀ are rounded to four decimals. The farthest pair, 23, is 4.9499e-5 from its
    /// value: within the rounding by a hair, so the bound allows nothing more than floating point
    /// (the 1e-9 the test adds).
    const TABLE_ROUNDING: f64 = 0.000_05;

    #[test]
    fn delta_e_matches_the_published_test_pairs_either_way_round() {
        for (i, [l1, a1, b1, l2, a2, b2, want]) in SHARMA.iter().copied().enumerate() {
            let (x, y) = (Lab { l: l1, a: a1, b: b1 }, Lab { l: l2, a: a2, b: b2 });
            let got = delta_e(x, y);
            assert!((got - want).abs() <= TABLE_ROUNDING + 1e-9, "pair {}: {got} (Table I: {want})", i + 1);
            assert!((delta_e(y, x) - got).abs() < 1e-12, "pair {} is symmetric", i + 1);
            assert_eq!(delta_e(x, x), 0.0, "pair {}: a colour against itself", i + 1);
        }
    }

    /// A colour that isn't one (a coordinate not a finite number) is never "no difference" from
    /// another: its difference is not a number.
    #[test]
    fn a_colour_that_is_not_one_has_no_difference_of_zero() {
        let grey = lab([128, 128, 128]);
        for odd in [Lab { l: f64::NAN, ..grey }, Lab { a: f64::INFINITY, ..grey }, Lab { b: f64::NEG_INFINITY, ..grey }] {
            assert!(delta_e(odd, grey).is_nan() && delta_e(grey, odd).is_nan(), "{odd:?}");
        }
        assert_eq!(delta_e(grey, grey), 0.0);
    }

    /// Solving for the matrix whose columns are the primaries' XYZ, scaled so that they add up to
    /// CIE's D65 white, gives [`SRGB_TO_XYZ`] again; its rows add up to that white.
    #[test]
    fn the_srgb_matrix_comes_from_its_primaries() {
        /// CIE's D65 white point for the 2° observer, X and Z with Y = 1 (CIE 15:2004).
        const D65: [f64; 3] = [0.950_47, 1.0, 1.088_83];
        let xyz = |x: f64, y: f64| [x / y, 1.0, (1.0 - x - y) / y];
        let (r, g, b, w) = (xyz(0.64, 0.33), xyz(0.30, 0.60), xyz(0.15, 0.06), D65);
        for i in 0..3 {
            assert!((WHITE[i] - D65[i]).abs() < 1e-6, "{WHITE:?}");
        }
        // The scales s with s_r r + s_g g + s_b b = w, by Cramer's rule.
        let det = |m: [[f64; 3]; 3]| {
            m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1]) - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
                + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
        };
        let columns = |c: [[f64; 3]; 3]| [0, 1, 2].map(|i| [c[0][i], c[1][i], c[2][i]]);
        let m = columns([r, g, b]);
        let d = det(m);
        let s = [0, 1, 2].map(|k| {
            let mut mk = m;
            for i in 0..3 {
                mk[i][k] = w[i];
            }
            det(mk) / d
        });
        for i in 0..3 {
            for (k, primary) in [r, g, b].iter().enumerate() {
                let want = primary[i] * s[k];
                assert!((SRGB_TO_XYZ[i][k] - want).abs() < 1e-6, "[{i}][{k}]: {} against {want}", SRGB_TO_XYZ[i][k]);
            }
        }
    }

    /// White is L* 100 with no a* or b*, black 0, and a grey has no a* or b*. sRGB's primaries
    /// come out where published conversions put them (D65, to two decimals).
    #[test]
    fn srgb_colours_in_lab() {
        let near = |c: Lab, want: [f64; 3], tolerance: f64| {
            (c.l - want[0]).abs() < tolerance && (c.a - want[1]).abs() < tolerance && (c.b - want[2]).abs() < tolerance
        };
        assert!(near(lab([255, 255, 255]), [100.0, 0.0, 0.0], 1e-9), "{:?}", lab([255, 255, 255]));
        assert!(near(lab([0, 0, 0]), [0.0, 0.0, 0.0], 1e-9), "{:?}", lab([0, 0, 0]));
        let grey = lab([119, 119, 119]);
        assert!(grey.a.abs() < 1e-6 && grey.b.abs() < 1e-6 && (grey.l - 50.0).abs() < 0.1, "{grey:?}");
        for (rgb, want) in [([255, 0, 0], [53.24, 80.09, 67.20]), ([0, 255, 0], [87.73, -86.18, 83.18]), ([0, 0, 255], [32.30, 79.19, -107.86])] {
            assert!(near(lab(rgb), want, 0.01), "{rgb:?}: {:?}", lab(rgb));
        }
        // Lightness rises with every grey step.
        let greys: Vec<f64> = (0..=255u8).map(|v| lab([v, v, v]).l).collect();
        assert!(greys.windows(2).all(|w| w[1] > w[0]));
    }

    /// The darkest 8-bit values, 1 to 10 of 255, lie on sRGB's straight segment (IEC 61966-2-1:
    /// encoded values up to 0.04045 are the light divided by 12.92), and the light they give lies
    /// on L*a*b*'s straight segment too (below (6/29)³ of the white, where CIE 15:2004 has
    /// f(t) = κ t / 116 + 16/116 with κ = 24389/27). So a dark grey's L* is κ v / (255 × 12.92),
    /// and a dark blue's a* and b* follow from the blue primary's column of the matrix alone.
    #[test]
    fn dark_colours_follow_both_straight_segments() {
        /// CIE's κ, the slope of L* over the light below (6/29)³ of the white.
        const KAPPA: f64 = 24389.0 / 27.0;
        /// sRGB's straight segment: the darkest encoded value past it, and its slope.
        const LAST_ON_SEGMENT: u8 = 10;
        const SLOPE: f64 = 12.92;
        for v in 1..=LAST_ON_SEGMENT {
            let light = f64::from(v) / 255.0 / SLOPE;
            let grey = lab([v, v, v]);
            assert!((grey.l - KAPPA * light).abs() < 1e-9 && grey.a.abs() < 1e-9 && grey.b.abs() < 1e-9, "grey {v}: {grey:?}");
            let blue = lab([0, 0, v]);
            let [x, y, z] = [0, 1, 2].map(|i| SRGB_TO_XYZ[i][2] * light / WHITE[i]);
            let f = |t: f64| KAPPA * t / LIGHTNESS_SCALE;
            let want = [KAPPA * y, A_SCALE * (f(x) - f(y)), B_SCALE * (f(y) - f(z))];
            assert!(
                (blue.l - want[0]).abs() < 1e-9 && (blue.a - want[1]).abs() < 1e-9 && (blue.b - want[2]).abs() < 1e-9,
                "blue {v}: {blue:?} against {want:?}"
            );
        }
        // The next value is on the power curve, which gives a little more light than the straight
        // segment's line would there (its L* is still on L*'s straight segment).
        let past = f64::from(LAST_ON_SEGMENT + 1) / 255.0;
        let curve = ((past + SRGB_OFFSET) / (1.0 + SRGB_OFFSET)).powf(SRGB_GAMMA);
        let grey = lab([LAST_ON_SEGMENT + 1; 3]);
        assert!(past > SRGB_LINEAR_LIMIT && (grey.l - KAPPA * curve).abs() < 1e-9 && grey.l - KAPPA * past / SLOPE > 1e-3, "{grey:?}");
    }
}
