#!/usr/bin/env python3

import lzma
import os
from pathlib import Path


def main() -> None:
    work_root = Path(os.environ["LIBLZMA_DEPENDENT_TEST_ROOT"])
    work = work_root / "python312"
    work.mkdir(parents=True, exist_ok=True)

    payload = (b"python lzma smoke\n" * 64) + bytes(range(64))
    compressed = lzma.compress(payload, format=lzma.FORMAT_XZ)
    assert lzma.decompress(compressed) == payload

    path = work / "payload.xz"
    with lzma.open(path, "wb", preset=6) as handle:
        handle.write(payload)

    with lzma.open(path, "rb") as handle:
        restored = handle.read()

    assert restored == payload
    print("python lzma ok")


if __name__ == "__main__":
    main()
