//! Argus: the functions Luau's C++ calls for its math (`argus_libm.h` in
//! vendor/luau0-src), the libm crate's, which give the same bits on every
//! machine where each platform's C library rounds its own way. They live
//! here, in the crate whose library holds Luau's objects, so every program
//! that links Luau links them too.

macro_rules! one {
    ($($name:ident => $f:path),* $(,)?) => {
        $(
            #[no_mangle]
            pub extern "C" fn $name(x: f64) -> f64 {
                $f(x)
            }
        )*
    };
}

one! {
    argus_libm_sin => libm::sin,
    argus_libm_cos => libm::cos,
    argus_libm_tan => libm::tan,
    argus_libm_asin => libm::asin,
    argus_libm_acos => libm::acos,
    argus_libm_atan => libm::atan,
    argus_libm_sinh => libm::sinh,
    argus_libm_cosh => libm::cosh,
    argus_libm_tanh => libm::tanh,
    argus_libm_exp => libm::exp,
    argus_libm_log => libm::log,
    argus_libm_log2 => libm::log2,
    argus_libm_log10 => libm::log10,
}

#[no_mangle]
pub extern "C" fn argus_libm_atan2(y: f64, x: f64) -> f64 {
    libm::atan2(y, x)
}

#[no_mangle]
pub extern "C" fn argus_libm_pow(x: f64, y: f64) -> f64 {
    libm::pow(x, y)
}
