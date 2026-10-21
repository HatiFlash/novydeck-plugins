# Contributing plugins

The community registry is maintained by HatiFlash for NovyDeck. Contributions are welcome, but every submission is reviewed and may be declined or removed by the maintainer.

## Submit a plugin

1. Fork this repository.
2. Publish your plugin archive (`.streamDeckPlugin`, or a `.zip` containing an `.sdPlugin` directory with a readable `manifest.json`) at a stable HTTPS URL.
3. Calculate the archive SHA-256: on Windows, run `certutil -hashfile path-to-file SHA256`; on Linux/macOS, run `sha256sum path-to-file`.
4. Add an entry to `registry.json`, following `schema/registry.schema.json`. Include the license, tested decks, accurate version, HTTPS homepage/download URLs, and the lowercase 64-character SHA-256.
5. Open a pull request and complete its checklist. CI checks the registry, downloads each archive, verifies its checksum, and inspects its ZIP contents.

Do not submit plugins you do not have the right to redistribute. Disclose relevant behavior and dependencies. No malicious behavior, hidden data collection, or misleading metadata is allowed. The maintainer moderates submissions and is not required to accept or retain any entry.

A listed plugin is third-party code and has not been independently audited or verified as safe. Users install it at their own risk; checksum verification confirms the downloaded bytes match the registry, not that the code is trustworthy.
