# Contributing plugins

The community registry is maintained by HatiFlash for NovyDeck. Contributions are welcome, but every submission is reviewed and may be declined or removed by the maintainer.

## Submit a plugin

1. Fork this repository.
2. Publish your free/open-source plugin under a verifiable free license and provide an official release archive (`.streamDeckPlugin`, or a `.zip` containing an `.sdPlugin` directory with a readable `manifest.json`) at a stable HTTPS URL. Test it before submitting.
3. Calculate the archive SHA-256: on Windows, run `certutil -hashfile path-to-file SHA256`; on Linux/macOS, run `sha256sum path-to-file`.
4. Add an entry to `registry.json`, following [the schema](schema/registry.schema.json) and [the example](scripts/example-entry.json). Include the license, tested decks (do not claim untested versions), accurate version, HTTPS homepage/download URLs, lowercase 64-character SHA-256, and required `min_novydeck`. Optional: `description_fr`, `uuid` (lowercase letters, digits, dots, hyphens), `category` (`system`, `audio`, `streaming`, `productivity`, `smart-home`, `communication`, `gaming`, `utilities`), `summary`/`summary_fr` (140 characters), square PNG `icon` (256 KiB), up to six PNG/JPEG `screenshots` (2 MiB, 4096 px), up to 100 `actions` (name, optional name_fr, 120 UTF-8 bytes), eight `tags` (40 characters) and `requires` (one string, 200 characters). All image URLs must be HTTPS.
5. Open a pull request and complete its checklist. CI checks the registry, downloads each archive, verifies its checksum, and inspects its ZIP contents.

Do not submit plugins you do not have the right to redistribute. Disclose relevant behavior and dependencies. No malicious behavior, hidden data collection, or misleading metadata is allowed. The maintainer moderates submissions and is not required to accept or retain any entry.

A listed plugin is third-party code and has not been independently audited or verified as safe. Users install it at their own risk; checksum verification confirms the downloaded bytes match the registry, not that the code is trustworthy.
