# First-party plugins / Plugins maison

This workspace contains `novydeck-sdk`, `novydeck-clock`, `novydeck-counter`, `novydeck-sysmonitor`, `novydeck-nowplaying` and `novydeck-oledsaver`. The SDK speaks the local Stream Deck WebSocket plugin protocol; it renders 144×144 PNG keys with the bundled DejaVu Sans font (license in `novydeck-sdk/assets/DEJAVU-LICENSE.txt`). No Node runtime or CDN is required. Source and plugin bundles are MIT licensed; the font retains its own license.

On Windows: `cargo generate-lockfile`, `cargo build --release`, `cargo test`, `cargo run --release --bin render-previews`, `python tools/make_icons.py`, `python tools/package.py`. Python packaging requires Pillow for icon generation. The GitHub Actions workflow builds and tests on Windows, uploads two archives and `SHA256SUMS.txt`, and publishes them on `first-party-v*` tags. Do not copy the registry template into `registry.json` until release URLs and SHA-256 hashes are available and the registry schema accepts its metadata.

En français : le workspace comprend actuellement le SDK interne, l'horloge et le compteur. La CI Windows prépare les archives et les aperçus, sans installation de Node. Le modèle de registre contient des valeurs TODO à compléter après publication. Les actions nécessitent un hôte compatible SDK Stream Deck et Windows 10 ou plus récent.

Not yet implemented: sysmonitor (PDH/NVML, CPU temperature explicitly unavailable without a third-party driver), nowplaying (Windows GSMTC and unavailable players), and oledsaver (Win32 monitor selection/fullscreen). They are deliberately not advertised or packaged. First Windows compilation/host compatibility check is still required.
