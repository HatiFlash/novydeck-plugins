# NovyDeck Community Plugins

[English](README.md) | [Français](README.fr.md)

A directory of open-source plugins for [NovyDeck](https://github.com/HatiFlash/NovyDeck). See [CONTRIBUTING.md](CONTRIBUTING.md) to propose a plugin.

## Official NovyDeck plugins

Source code and packaging: [first-party/](first-party/). The nine native plugins are Clock, Counter, System Monitor, Now Playing, OLED Saver, Home Assistant, Google Assistant Command, Google Home and Wallpaper Engine. Home Assistant needs your own server and token. Google Assistant Command sends typed phrases via your own Google Cloud project and desktop OAuth; Google Home instead controls devices or rooms directly via an unofficial private API and a one-time account token (no Cloud project). That private API can change without notice; the account token grants broad access. Wallpaper Engine requires a separately installed, running Steam copy; its proprietary executable and wallpapers are not redistributed. These integrations have not been verified against real accounts/services or a local Wallpaper Engine installation.

## Community plugins

The registry lists official release archives with verified checksums. See the [1.0.3 setup guide and bilingual inspector previews](first-party/README.md#new-in-103--nouveautés-103) for Google Home, Google Assistant and Wallpaper Engine. Google Home is experimental; no real Google account or Wallpaper Engine installation has been qualified.

An entry includes an HTTPS homepage and official release archive, SHA-256, free/open-source license, exact version, tested decks (empty if untested), and required `min_novydeck`. Optional metadata: `description_fr`, `uuid`, `category` (`system`, `audio`, `streaming`, `productivity`, `smart-home`, `communication`, `gaming`, `utilities`), `summary`/`summary_fr` (at most 140 characters), HTTPS PNG square `icon` (at most 256 KiB), up to six HTTPS PNG/JPEG `screenshots` (each at most 2 MiB and 4096 px per side), up to 100 `actions` (`name` and optional `name_fr`, up to 120 UTF-8 bytes), up to eight `tags` (at most 40 characters), and `requires` (one string, at most 200 characters). See [example entry](scripts/example-entry.json).

## Security badges

NovyDeck shows up to three badges for each plugin in its store:

- **Official**: published by NovyDeck (`com.novydeck.*`) from this repository's releases.
- **Scanned**: the exact archive listed in the registry (same SHA-256 and version) passed the automatic check. A GitHub Actions workflow downloads every archive, verifies its SHA-256, scans it with Microsoft Defender and looks for risky content (executables and their signatures, obfuscated scripts, download-and-run patterns, startup registry keys, manifest consistency, licence and official source). It runs on every change to the catalogue, every week, and on demand, without any manual step; results are published in [`security/scan-results.json`](security/scan-results.json).
- **Tested by NovyDeck**: that exact version was tried by hand in NovyDeck (`tested_by_novydeck` field).

A plugin whose scan needs review shows a warning in NovyDeck. A scan reduces risk; it cannot rule it out completely, and plugins run with your user rights.

## Use the community library in NovyDeck

The registry metadata supports three distinct badges: “Official” (`com.novydeck.*`, publisher identity), “Scanned” (automated archive check, see `security/scan-results.json`; only a passed result for the matching SHA-256), and “Tested by NovyDeck” (manual test of the exact version, `tested_by_novydeck` date/version). GitHub Actions checks SHA-256, Microsoft Defender and static risk heuristics on PRs and weekly. A scan does not guarantee absence of risk; review findings require human evaluation.

In the app, open **Library → Plugins → Community**. Connecting to GitHub requires your consent; after you agree, confirm the connection. NovyDeck verifies the downloaded archive against its published SHA-256 before import. Community plugins without the Official badge are marked **“Community — unverified”**: a matching checksum verifies the download, not the safety of third-party code. Review each plugin before installing.

See [contribution guidelines](CONTRIBUTING.md) and the [registry schema](schema/registry.schema.json).
