"""Check Mac programs and libraries without a Mac (docs/macos-checklist.md):

    python3 tools/release/macho_check.py <file>...

Each file must be a universal Mach-O with an arm64 and an x86_64 part,
and each part must:

- be signed, every page of its code hashed as its signature says. Apple
  Silicon runs nothing unsigned; an ad-hoc signature, which proves no one
  made it, is enough to run;
- run on macOS 11 or later (the first with Apple Silicon);
- load only Apple's system frameworks and libraries, or a library beside
  it (`@loader_path/`, Steam's), so it runs on a Mac with nothing else
  installed;
- list each library once. macOS 27's loader stops a program at launch
  ("dyld: duplicate linked dylib") that names one twice, and Zig's linker
  wrote one entry per `-framework` it was given — Foundation four times —
  until macos-env.sh's shim stopped giving it the same one twice. Found by
  Todd's test of 0.0.5, the first Mac build: every program crashed.

Apple's own `codesign --verify` would do the first, but it runs only on
a Mac, and rcodesign's verify reports every ad-hoc signature as broken.
"""

import hashlib
import struct
import sys

CPU = {0x01000007: "x86_64", 0x0100000C: "arm64"}
LOADS = {0xC, 0x80000018, 0x8000001F, 0x20, 0x80000023}  # every kind of dylib load
LC_CODE_SIGNATURE = 0x1D
LC_BUILD_VERSION = 0x32
LC_VERSION_MIN_MACOSX = 0x24
HASHES = {1: hashlib.sha1, 2: hashlib.sha256, 3: lambda b: hashlib.sha256(b), 4: hashlib.sha384}
ALLOWED = ("/System/Library/", "/usr/lib/", "@loader_path/")


def version(v):
    return f"{v >> 16}.{(v >> 8) & 0xFF}.{v & 0xFF}"


def check_signature(part, dataoff, datasize):
    """The signature's problems (each code directory's page hashes against
    the part's bytes; none when it holds), and whether it is ad hoc."""
    sig = part[dataoff:dataoff + datasize]
    magic, _, count = struct.unpack_from(">III", sig, 0)
    if magic != 0xFADE0CC0:
        return ["the signature is not a signature blob"], None
    problems, cds, kind = [], 0, None
    for i in range(count):
        slot_type, off = struct.unpack_from(">II", sig, 12 + 8 * i)
        if slot_type != 0 and not 0x1000 <= slot_type <= 0x1004:
            continue
        cd = sig[off:]
        (cmagic, _, cver, flags, hash_off, _, _, n_code, limit, hsize, htype, _, page_log) = struct.unpack_from(
            ">IIIIIIIIIBBBB", cd, 0)
        if cmagic != 0xFADE0C02:
            continue
        cds += 1
        if cver >= 0x20300:
            limit64 = struct.unpack_from(">Q", cd, 56)[0]
            limit = limit64 or limit
        page = 1 << page_log if page_log else limit
        h = HASHES.get(htype)
        if h is None:
            problems.append(f"an unknown hash type {htype}")
            continue
        for slot in range(n_code):
            start = slot * page
            want = cd[hash_off + slot * hsize:hash_off + (slot + 1) * hsize]
            have = h(part[start:min(start + page, limit)]).digest()[:hsize]
            if want != have:
                problems.append(f"page {slot} of {n_code} does not match its hash")
                break
        kind = "ad hoc" if flags & 0x2 else "signed"
    if cds == 0:
        problems.append("no code directory in the signature")
    return problems, kind


def check_part(name, part):
    problems = []
    magic, cpu, _, _, ncmds, _, _, _ = struct.unpack_from("<IiiIIIII", part, 0)
    if magic != 0xFEEDFACF:
        return [f"{name}: not a 64-bit Mach-O"], None
    arch = CPU.get(cpu & 0xFFFFFFFF, hex(cpu))
    off, loads, minos, signed, kind = 32, [], None, False, None
    for _ in range(ncmds):
        cmd, size = struct.unpack_from("<II", part, off)
        if cmd in LOADS:
            n = struct.unpack_from("<I", part, off + 8)[0]
            loads.append(part[off + n:off + size].split(b"\0")[0].decode())
        elif cmd == LC_BUILD_VERSION:
            minos = struct.unpack_from("<I", part, off + 12)[0]
        elif cmd == LC_VERSION_MIN_MACOSX and minos is None:
            minos = struct.unpack_from("<I", part, off + 8)[0]
        elif cmd == LC_CODE_SIGNATURE:
            dataoff, datasize = struct.unpack_from("<II", part, off + 8)
            bad, kind = check_signature(part, dataoff, datasize)
            problems += [f"{arch}: {p}" for p in bad]
            signed = not bad
        off += size
    if not signed and not any(p.startswith(f"{arch}:") for p in problems):
        problems.append(f"{arch}: not signed")
    if minos is None:
        problems.append(f"{arch}: says no oldest macOS")
    elif minos > 0x0B0000:
        problems.append(f"{arch}: needs macOS {version(minos)}, later than 11")
    for lib in sorted(set(loads)):
        if not lib.startswith(ALLOWED):
            problems.append(f"{arch}: loads {lib}, which a Mac does not have")
    for lib in sorted({l for l in loads if loads.count(l) > 1}):
        problems.append(f"{arch}: loads {lib} {loads.count(lib)} times, which macOS 27 refuses to start")
    summary = f"{arch} {kind or 'unsigned'}, macOS {version(minos) if minos else '?'}+, {len(set(loads))} libraries"
    return problems, summary


def check(path):
    data = open(path, "rb").read()
    magic, n = struct.unpack_from(">II", data, 0)
    if magic != 0xCAFEBABE:
        return [f"not a universal Mach-O"], []
    problems, summaries, archs = [], [], []
    for i in range(n):
        cpu, _, off, size, _ = struct.unpack_from(">iiIII", data, 8 + 20 * i)
        archs.append(CPU.get(cpu & 0xFFFFFFFF, hex(cpu)))
        p, s = check_part(path, data[off:off + size])
        problems += p
        if s:
            summaries.append(s)
    for want in ("arm64", "x86_64"):
        if want not in archs:
            problems.append(f"no {want} part")
    return problems, summaries


def main():
    failed = False
    for path in sys.argv[1:]:
        problems, summaries = check(path)
        if problems:
            failed = True
            print(f"{path}: FAILED")
            for p in problems:
                print(f"  {p}")
        else:
            print(f"{path}: {'; '.join(summaries)}")
    sys.exit(1 if failed else 0)


if __name__ == "__main__":
    main()
