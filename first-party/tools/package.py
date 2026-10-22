#!/usr/bin/env python3
"""Package native Windows builds as Stream Deck archives; hash the final archives."""
import hashlib
import shutil
import zipfile
from pathlib import Path

root = Path(__file__).resolve().parents[1]
dist = root / "dist"
dist.mkdir(exist_ok=True)
checksums = []
for name in ("clock", "counter", "sysmonitor", "nowplaying", "oledsaver", "googleassistant"):
    plugin = root / "plugins" / f"com.novydeck.{name}.sdPlugin"
    exe = plugin / "bin" / f"novydeck-{name}.exe"
    exe.parent.mkdir(exist_ok=True)
    shutil.copy2(root / "target/release" / exe.name, exe)
    archive = dist / f"com.novydeck.{name}.streamDeckPlugin"
    with zipfile.ZipFile(archive, "w", compression=zipfile.ZIP_DEFLATED) as z:
        for path in sorted(plugin.rglob("*")):
            if path.is_file():
                z.write(path, path.relative_to(root / "plugins").as_posix())
    checksums.append(f"{hashlib.sha256(archive.read_bytes()).hexdigest()}  {archive.name}")
(dist / "SHA256SUMS.txt").write_text("\n".join(checksums) + "\n", encoding="utf-8")
