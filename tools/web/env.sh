# Environment for building Argus for the web (wasm32-unknown-unknown).
#
# Luau is C++, and it raises its errors as C++ exceptions, so the wasm build
# needs a C++ compiler for wasm with a C++ runtime built for exceptions:
# wasi-sdk 33 or later (https://github.com/WebAssembly/wasi-sdk). Set
# WASI_SDK to where it is unpacked.
#
#   . tools/web/env.sh
W=${WASI_SDK:-C:/Users/Kevin/tools/wasi-sdk-34.0-x86_64-windows}
S=$W/share/wasi-sysroot
export CC_wasm32_unknown_unknown="$W/bin/clang.exe"
export CXX_wasm32_unknown_unknown="$W/bin/clang++.exe"
export AR_wasm32_unknown_unknown="$W/bin/ar.exe"
# The WASI target finds the sysroot's headers; -fwasm-exceptions is wasm's
# own exception handling, in its new form (exnref) like wasi-sdk's C++
# runtime, since a browser refuses a module that mixes the two; clock() is opt-in in wasi-libc (Luau's os.clock);
# Luau's parser uses strtod without including stdlib.h.
export CFLAGS_wasm32_unknown_unknown="--target=wasm32-wasip1 -fwasm-exceptions -mllvm -wasm-use-legacy-eh=false -D_WASI_EMULATED_PROCESS_CLOCKS"
export CXXFLAGS_wasm32_unknown_unknown="--target=wasm32-wasip1 -fwasm-exceptions -mllvm -wasm-use-legacy-eh=false -D_WASI_EMULATED_PROCESS_CLOCKS -include stdlib.h"
# luau0-src asks for libstdc++ unless told otherwise.
export CXXSTDLIB_wasm32_unknown_unknown="c++"
# The exception-capable C++ runtime, and the C library. The C library's
# system calls are answered inside the module (crates/argus-web/src/wasi.rs).
export CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUSTFLAGS="-C link-arg=-L$S/lib/wasm32-wasip1/eh -C link-arg=-L$S/lib/wasm32-wasip1 -C link-arg=-lc++abi -C link-arg=-lunwind -C link-arg=-lwasi-emulated-process-clocks -C link-arg=-lc"
