# Contributing plugins

The community registry is maintained by HatiFlash for NovyDeck. Contributions are welcome, but every submission is reviewed and may be declined or removed by the maintainer.

## Submit a plugin

1. Fork this repository.
2. Publish your free/open-source plugin under a verifiable free license and provide an official release archive (`.streamDeckPlugin`, or a `.zip` containing an `.sdPlugin` directory with a readable `manifest.json`) at a stable HTTPS URL. Test it before submitting.
3. Calculate the archive SHA-256: on Windows, run `certutil -hashfile path-to-file SHA256`; on Linux/macOS, run `sha256sum path-to-file`.
4. Add an entry to `registry.json`, following [the schema](schema/registry.schema.json) and [the example](scripts/example-entry.json). Include the license, tested decks (do not claim untested versions), accurate version, HTTPS homepage/download URLs, lowercase 64-character SHA-256, and required `min_novydeck`. Optional: `description_fr`, `uuid` (lowercase letters, digits, dots, hyphens), `category` (`system`, `audio`, `streaming`, `productivity`, `smart-home`, `communication`, `gaming`, `utilities`), `summary`/`summary_fr` (140 characters), square PNG `icon` (256 KiB), up to six PNG/JPEG `screenshots` (2 MiB, 4096 px), up to 100 `actions` (name, optional name_fr, 120 UTF-8 bytes), eight `tags` (40 characters) and `requires` (one string, 200 characters). All image URLs must be HTTPS.
5. Open a pull request and complete its checklist. CI checks the registry, downloads each archive, verifies its checksum, and inspects its ZIP contents.

Do not submit plugins you do not have the right to redistribute. Disclose relevant behavior and dependencies. No malicious behavior, hidden data collection, or misleading metadata is allowed. The maintainer moderates submissions and is not required to accept or retain any entry.

Badges: “Official” applies to `com.novydeck.*` plugins; “Scanned” is awarded only when the current archive SHA-256 matches, Microsoft Defender reports clean, and the automated static scan has no severe findings (`security/scan-results.json`, matching the plugin version/checksum). “Tested by NovyDeck” is a separate manual claim, recorded by the maintainers as `tested_by_novydeck: {"version": "1.0.2", "date": "YYYY-MM-DD"}` for the exact published version; contributors must not self-assert it. GitHub Actions scans every registry archive on PRs, eligible pushes, weekly, and on request. It lists binary signatures, scripts, URLs, manifest fields, release source and license; findings marked review require human attention.

A listed plugin is third-party code and has not been independently audited or verified as safe. A clean scan does not guarantee absence of risk; checksum verification only confirms the downloaded bytes match the registry. Users install it at their own risk.
