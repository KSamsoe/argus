# The environment for cross-building Argus for macOS on Linux (klab), for
# stage.sh --target macos. Source it: . tools/release/macos-env.sh
#
# Set up once, in ~/tools, nothing needing root (docs/macos-checklist.md):
#
#   rustup target add aarch64-apple-darwin x86_64-apple-darwin
#   Zig, from ziglang.org, unpacked in ~/tools ($ARGUS_ZIG to use another)
#   cargo install --locked cargo-zigbuild apple-codesign --bin rcodesign
#   MacOSX15.5.sdk, unpacked in ~/tools/sdk ($SDKROOT to use another)
#   libclang (for coreaudio-sys's bindings; $LIBCLANG_PATH to use another)
#
# Apple's SDK license allows the SDK only on Apple hardware; cross-building
# here was Kevin's choice, made knowing that.

ARGUS_ZIG=${ARGUS_ZIG:-$(ls -d "$HOME"/tools/zig-x86_64-linux-* 2>/dev/null | sort -V | tail -1)}
export SDKROOT=${SDKROOT:-$HOME/tools/sdk/MacOSX15.5.sdk}
# The oldest macOS the programs run on: the first with Apple Silicon.
export MACOSX_DEPLOYMENT_TARGET=${MACOSX_DEPLOYMENT_TARGET:-11.0}

# cargo-zigbuild hands Zig `-target aarch64-macos-none`, with no macOS in
# it, and drops any other -target, so Zig takes the SDK's (15) and every
# program would refuse to start on an older Mac. Zig takes the version
# only in the target (`aarch64-macos.11.0-none`; -mmacosx-version-min is
# ignored), so cargo-zigbuild finds this `zig` first, which puts it there.
#
# It also gives Zig each `-framework` once. rustc passes one for every
# crate that links a framework (Foundation three times for argus), and
# Zig's linker, unlike Apple's, writes a load command for each: 0.0.5's
# programs named Foundation up to five times, and macOS 27's loader
# stops a program that names a library twice ("dyld: duplicate linked
# dylib"), so every one of them crashed at launch on Todd's Mac.
# macho_check.py now fails a program that does.
ARGUS_ZIG_SHIM=$HOME/tools/zig-macos-shim
mkdir -p "$ARGUS_ZIG_SHIM"
cat > "$ARGUS_ZIG_SHIM/zig" <<SHIM
#!/bin/sh
n=\$#
seen=" "
framework=0
for a in "\$@"; do
  if [ "\$framework" = 1 ]; then
    framework=0
    case "\$seen" in *" \$a "*) continue ;; esac
    seen="\$seen\$a "
    set -- "\$@" -framework "\$a"
    continue
  fi
  case "\$a" in
    -framework) framework=1; continue ;;
    *-macos-none) a="\${a%-macos-none}-macos.$MACOSX_DEPLOYMENT_TARGET-none" ;;
  esac
  set -- "\$@" "\$a"
done
shift "\$n"
exec "$ARGUS_ZIG/zig" "\$@"
SHIM
chmod +x "$ARGUS_ZIG_SHIM/zig"
export PATH="$HOME/.cargo/bin:$ARGUS_ZIG_SHIM:$ARGUS_ZIG:$PATH"
export LIBCLANG_PATH=${LIBCLANG_PATH:-$(ls -d /usr/lib/llvm-*/lib 2>/dev/null | sort -V | tail -1)}
export COREAUDIO_SDK_PATH=$SDKROOT
export BINDGEN_EXTRA_CLANG_ARGS_aarch64_apple_darwin="--target=arm64-apple-macos$MACOSX_DEPLOYMENT_TARGET -isysroot $SDKROOT"
export BINDGEN_EXTRA_CLANG_ARGS_x86_64_apple_darwin="--target=x86_64-apple-macos$MACOSX_DEPLOYMENT_TARGET -isysroot $SDKROOT"

for need in zig cargo-zigbuild rcodesign; do
  command -v "$need" > /dev/null || echo "macos-env.sh: no $need on the PATH (see the set-up above)" >&2
done
[ -d "$SDKROOT" ] || echo "macos-env.sh: no macOS SDK at $SDKROOT" >&2
