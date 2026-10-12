"""Join one program's builds for each Mac architecture into one universal
program, as Apple's lipo does, without a Mac (docs/macos-checklist.md):

    python3 tools/release/lipo.py <out> <thin Mach-O>...

stage.sh builds each architecture on its own and joins them here.
cargo-zigbuild's own universal2 target once made an editor whose arm64
part was 384 KB long, where the arm64 build was 9.5 MB (an incremental
build; a clean one was whole); rcodesign, signing it, kept only the
x86_64 part, and macho_check.py caught that.

The universal file is a header listing each part (its CPU, where it
starts, its length) and the parts, each starting on a 16 KiB boundary
(arm64's page size; x86_64 needs only 4 KiB).
"""

import os
import struct
import sys

ALIGN = 14


def main():
    out, parts = sys.argv[1], sys.argv[2:]
    if len(parts) < 2:
        raise SystemExit("lipo.py: <out> <thin Mach-O> <thin Mach-O>...")
    blobs = []
    for p in parts:
        data = open(p, "rb").read()
        magic, cpu, sub = struct.unpack_from("<Iii", data, 0)
        if magic != 0xFEEDFACF:
            raise SystemExit(f"lipo.py: {p} is not a thin 64-bit Mach-O")
        if any(c == cpu for c, _, _ in blobs):
            raise SystemExit(f"lipo.py: two parts for one CPU ({p})")
        # The header's capability bits stay out of the fat header, as lipo has it.
        blobs.append((cpu, sub & 0x00FFFFFF, data))
    blobs.sort(key=lambda b: b[0] & 0xFFFFFFFF)
    header = struct.pack(">II", 0xCAFEBABE, len(blobs))
    offset = 8 + 20 * len(blobs)
    layout = []
    for cpu, sub, data in blobs:
        offset = (offset + (1 << ALIGN) - 1) & ~((1 << ALIGN) - 1)
        layout.append((cpu, sub, offset, data))
        header += struct.pack(">iiIII", cpu, sub, offset, len(data), ALIGN)
        offset += len(data)
    tmp = out + ".tmp"
    with open(tmp, "wb") as f:
        f.write(header)
        for _, _, at, data in layout:
            f.write(b"\0" * (at - f.tell()))
            f.write(data)
    os.chmod(tmp, 0o755)
    os.replace(tmp, out)


if __name__ == "__main__":
    main()
