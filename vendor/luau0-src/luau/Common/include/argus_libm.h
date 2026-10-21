// Argus: Luau's math from the libm crate, so a script's numbers are the
// same bits on every machine (the Argus repo's docs/multiplayer-checklist.md,
// item 0). The platforms' C libraries disagree in the last bit on a few
// inputs in a hundred; libm does not. The functions are Rust's, defined in
// vendor/mlua-sys/src/argus_libm.rs, which every Luau build links.
//
// A file that computes a transcendental includes this last, after every
// other header, so these macros rename only its own calls (and the
// luai_numpow it expands), never a declaration in <math.h>.
#pragma once

#ifdef __cplusplus
extern "C" {
#endif
double argus_libm_sin(double x);
double argus_libm_cos(double x);
double argus_libm_tan(double x);
double argus_libm_asin(double x);
double argus_libm_acos(double x);
double argus_libm_atan(double x);
double argus_libm_atan2(double y, double x);
double argus_libm_sinh(double x);
double argus_libm_cosh(double x);
double argus_libm_tanh(double x);
double argus_libm_exp(double x);
double argus_libm_log(double x);
double argus_libm_log2(double x);
double argus_libm_log10(double x);
double argus_libm_pow(double x, double y);
#ifdef __cplusplus
}
#endif

#define sin(x) argus_libm_sin(x)
#define cos(x) argus_libm_cos(x)
#define tan(x) argus_libm_tan(x)
#define asin(x) argus_libm_asin(x)
#define acos(x) argus_libm_acos(x)
#define atan(x) argus_libm_atan(x)
#define atan2(y, x) argus_libm_atan2(y, x)
#define sinh(x) argus_libm_sinh(x)
#define cosh(x) argus_libm_cosh(x)
#define tanh(x) argus_libm_tanh(x)
#define exp(x) argus_libm_exp(x)
#define log(x) argus_libm_log(x)
#define log2(x) argus_libm_log2(x)
#define log10(x) argus_libm_log10(x)
#define pow(x, y) argus_libm_pow(x, y)
