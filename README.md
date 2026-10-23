# NovyDeck Community Plugins

[English](README.md) | [Français](README.fr.md)

A directory of open-source plugins for [NovyDeck](https://github.com/HatiFlash/NovyDeck). See [CONTRIBUTING.md](CONTRIBUTING.md) to propose a plugin.

## Official NovyDeck plugins

Source code and packaging: [first-party/](first-party/). The seven native plugins are Clock, Counter, System Monitor, Now Playing, OLED Saver, Home Assistant and Google Assistant Command. Home Assistant needs your own server and token; Google Assistant needs your own Google Cloud project and desktop OAuth credentials. Neither integration has been tested against a real account/server.

## Community plugins

The registry is being prepared; only entries with verified release archives and checksums will be published.

An entry includes an HTTPS homepage and official release archive, SHA-256, free/open-source license, exact version, tested decks (empty if untested), and required `min_novydeck`. Optional metadata: `description_fr`, `uuid`, `category` (`system`, `audio`, `streaming`, `productivity`, `smart-home`, `communication`, `gaming`, `utilities`), `summary`/`summary_fr` (at most 140 characters), HTTPS PNG square `icon` (at most 256 KiB), up to six HTTPS PNG/JPEG `screenshots` (each at most 2 MiB and 4096 px per side), up to 100 `actions` (`name` and optional `name_fr`, up to 120 UTF-8 bytes), up to eight `tags` (at most 40 characters), and `requires` (one string, at most 200 characters). See [example entry](scripts/example-entry.json).

## Use the community library in NovyDeck

The registry metadata supports three distinct badges: “Official” (`com.novydeck.*`, publisher identity), “Scanned” (automated archive check, see `security/scan-results.json`; only a passed result for the matching SHA-256), and “Tested by NovyDeck” (manual test of the exact version, `tested_by_novydeck` date/version). GitHub Actions checks SHA-256, Microsoft Defender and static risk heuristics on PRs and weekly. A scan does not guarantee absence of risk; review findings require human evaluation. The app's current “Community — unverified” label is not automatically replaced by these metadata badges.

In the app, open **Library → Plugins → Community**. Connecting to GitHub requires your consent; after you agree, confirm the connection. NovyDeck verifies the downloaded archive against its published SHA-256 before import. Community entries are marked **“Community — unverified”**: a matching checksum verifies the download, not the safety of third-party code. Review each plugin before installing.

See [contribution guidelines](CONTRIBUTING.md) and the [registry schema](schema/registry.schema.json).
