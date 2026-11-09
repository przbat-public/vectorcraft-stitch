//! One maths library on every system, for the test binary that declares this module (the
//! embroidery golden corpus, the image listing and the pinned exports): the C maths functions
//! Rust's `f64` and `f32` methods call (`sin`, `cos`, `pow`…), and those the compiler makes of them
//! (`sincos` on Linux, `__sincos_stret` and `__exp10` on macOS), are defined here as the `libm`
//! crate's, a Rust port of musl's, so that the binary computes the same bits on every system.
//!
//! Each system's own library rounds some results differently in the last bit (macOS's, glibc's and
//! FreeBSD's each its own way), and one bit can move a stitch: on 2026-11-02 the corpus's document
//! 30 exported differently on Linux and on FreeBSD than on macOS, through upstream's Rotate
//! (`kurbo::Affine::rotate`, whose `sin_cos` rounds its angle's sine down on FreeBSD and in musl
//! and up on macOS). The app itself still computes with the system's library: only the hashes the
//! corpus records become the same on every system.
//!
//! A function defined in the test binary is the one every call in that binary reaches, since the
//! linker takes an object's definition before a shared library's (Mach-O and ELF alike). The module
//! is declared on macOS, Linux and FreeBSD, where `this_binary_computes_with_the_libm_crate` checks
//! it; elsewhere (Windows, whose engine tests CI doesn't run) the system's library is used.
// Standing in for the C library's functions takes `no_mangle`, and `sincos` writes through the
// pointers it is given.
#![allow(unsafe_code)]

/// Defines each C maths function named as the `libm` crate's function of the same name.
macro_rules! from_libm {
    ($($name:ident($($arg:ident),+) -> $t:ty;)+) => {
        $(
            #[unsafe(no_mangle)]
            extern "C" fn $name($($arg: $t),+) -> $t {
                libm::$name($($arg),+)
            }
        )+
    };
}

from_libm! {
    sin(x) -> f64;
    cos(x) -> f64;
    tan(x) -> f64;
    asin(x) -> f64;
    acos(x) -> f64;
    atan(x) -> f64;
    atan2(y, x) -> f64;
    sinh(x) -> f64;
    cosh(x) -> f64;
    tanh(x) -> f64;
    exp(x) -> f64;
    exp2(x) -> f64;
    exp10(x) -> f64;
    expm1(x) -> f64;
    log(x) -> f64;
    log2(x) -> f64;
    log10(x) -> f64;
    log1p(x) -> f64;
    pow(x, y) -> f64;
    hypot(x, y) -> f64;
    cbrt(x) -> f64;
    sinf(x) -> f32;
    cosf(x) -> f32;
    tanf(x) -> f32;
    asinf(x) -> f32;
    acosf(x) -> f32;
    atanf(x) -> f32;
    atan2f(y, x) -> f32;
    sinhf(x) -> f32;
    coshf(x) -> f32;
    tanhf(x) -> f32;
    expf(x) -> f32;
    exp2f(x) -> f32;
    exp10f(x) -> f32;
    expm1f(x) -> f32;
    logf(x) -> f32;
    log2f(x) -> f32;
    log10f(x) -> f32;
    log1pf(x) -> f32;
    powf(x, y) -> f32;
    hypotf(x, y) -> f32;
    cbrtf(x) -> f32;
}

/// The sine and cosine of one angle, as macOS's `__sincos_stret` returns them (in two registers,
/// read by the caller the compiler made).
#[repr(C)]
#[allow(dead_code)]
struct SinCos {
    sin: f64,
    cos: f64,
}

/// The same in single precision, as `__sincosf_stret` returns them.
#[repr(C)]
#[allow(dead_code)]
struct SinCosF {
    sin: f32,
    cos: f32,
}

/// What the compiler makes of the sine and the cosine of one angle on macOS.
#[unsafe(no_mangle)]
extern "C" fn __sincos_stret(x: f64) -> SinCos {
    let (sin, cos) = libm::sincos(x);
    SinCos { sin, cos }
}

/// The same in single precision.
#[unsafe(no_mangle)]
extern "C" fn __sincosf_stret(x: f32) -> SinCosF {
    let (sin, cos) = libm::sincosf(x);
    SinCosF { sin, cos }
}

/// What the compiler makes of `10^x` on macOS.
#[unsafe(no_mangle)]
extern "C" fn __exp10(x: f64) -> f64 {
    libm::exp10(x)
}

/// The same in single precision.
#[unsafe(no_mangle)]
extern "C" fn __exp10f(x: f32) -> f32 {
    libm::exp10f(x)
}

/// What the compiler makes of the sine and the cosine of one angle on Linux (glibc's `sincos`).
///
/// # Safety
///
/// `sin_out` and `cos_out` point at a place for one `f64` each, as the compiler passes them.
#[unsafe(no_mangle)]
unsafe extern "C" fn sincos(x: f64, sin_out: *mut f64, cos_out: *mut f64) {
    let (sin, cos) = libm::sincos(x);
    // SAFETY: the compiler calls `sincos` with a place for each result, as glibc's takes them.
    unsafe {
        sin_out.write(sin);
        cos_out.write(cos);
    }
}

/// The same in single precision.
///
/// # Safety
///
/// `sin_out` and `cos_out` point at a place for one `f32` each, as the compiler passes them.
#[unsafe(no_mangle)]
unsafe extern "C" fn sincosf(x: f32, sin_out: *mut f32, cos_out: *mut f32) {
    let (sin, cos) = libm::sincosf(x);
    // SAFETY: the compiler calls `sincosf` with a place for each result, as glibc's takes them.
    unsafe {
        sin_out.write(sin);
        cos_out.write(cos);
    }
}

// Arguments (their bits) on which macOS's maths library and the `libm` crate round differently, so
// that the check below fails on macOS if a call reaches the system's library: each found in the
// corpus's document 30 on 2026-11-02, by comparing the two results of every call it makes.

/// The angle of document 30's Rotate, in radians (0.6990328284000833), whose sine moved a stitch.
const ROTATE_ANGLE: u64 = 0x3fe6_5e7a_1819_e12e;
/// A base and an exponent of `pow` (0.2588190451025206 and 0.6).
const POW_ARGS: (u64, u64) = (0x3fd0_907d_c193_068d, 0x3fe3_3333_3333_3333);
/// An argument of `acos` (0.9925409194943832).
const ACOS_ARG: u64 = 0x3fef_c2e5_2ca5_71d3;
/// An argument of `tan` (0.005847225435664521).
const TAN_ARG: u64 = 0x3f77_f342_a04b_da06;
/// The `y` and `x` of an `atan2` (0.2672341385880834 and 10.005560123453574).
const ATAN2_ARGS: (u64, u64) = (0x3fd1_1a5d_3767_1340, 0x4024_02d8_c6c8_ca26);
/// The two sides of a `hypot` (0.0029150940811827975 and 0.08642622825727737).
const HYPOT_ARGS: (u64, u64) = (0x3f67_e165_37ca_4992, 0x3fb6_2007_7fe1_b2fd);

/// Fails unless `got` has the bits of `want`, the `libm` crate's result.
fn same(what: &str, got: f64, want: f64) {
    assert_eq!(got.to_bits(), want.to_bits(), "{what}: {got:?} where the libm crate gives {want:?}, so the system's maths library was called");
}

#[test]
fn this_binary_computes_with_the_libm_crate() {
    use std::hint::black_box;
    let bits = |b: u64| black_box(f64::from_bits(b));
    let angle = bits(ROTATE_ANGLE);
    same("sin", angle.sin(), libm::sin(angle));
    let angle = bits(ROTATE_ANGLE);
    let (sin, cos) = angle.sin_cos();
    same("sin_cos's sine", sin, libm::sin(angle));
    same("sin_cos's cosine", cos, libm::cos(angle));
    let (base, power) = (bits(POW_ARGS.0), bits(POW_ARGS.1));
    same("powf", base.powf(power), libm::pow(base, power));
    let x = bits(ACOS_ARG);
    same("acos", x.acos(), libm::acos(x));
    let x = bits(TAN_ARG);
    same("tan", x.tan(), libm::tan(x));
    let (y, x) = (bits(ATAN2_ARGS.0), bits(ATAN2_ARGS.1));
    same("atan2", y.atan2(x), libm::atan2(y, x));
    let (a, b) = (bits(HYPOT_ARGS.0), bits(HYPOT_ARGS.1));
    same("hypot", a.hypot(b), libm::hypot(a, b));
}
