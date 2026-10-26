# First-party plugins / Plugins maison

## Wallpaper Engine Stop / Play — 1.0.6

EN: Update Wallpaper Engine to **1.0.6**, then choose **Stop / Play (all screens)** from the installed plugin's actions. One key alternates Stop and Play; explicit commands and Pause / Play remain available. Playback state is shared locally across the plugin's keys and only changes after a successful CLI command. It is not synchronized with changes made outside the plugin. The initial state is unknown and the first press sends Stop. Windows tests use an inert CLI fixture, not a real Wallpaper Engine installation.

FR : Mettez à jour Wallpaper Engine en **1.0.6**, puis choisissez **Stop / Play (all screens)** dans les actions du plugin installé. Une seule touche alterne Arrêt et Lecture ; les commandes séparées et Pause / Lecture restent disponibles. L'état est partagé localement entre les touches et ne change qu'après la réussite de la commande CLI ; il ne suit pas les modifications effectuées ailleurs. Au départ, l'état est inconnu et le premier appui envoie Arrêt. Les tests Windows emploient un exécutable témoin inerte, pas une installation Wallpaper Engine réelle.

| English inspector | Inspecteur français |
| --- | --- |
| ![Stop Play settings](screenshots/wallpaperengine/wallpaperengine-stopplay-en.png) | ![Réglages Arrêt Lecture](screenshots/wallpaperengine/wallpaperengine-stopplay-fr.png) |

Offline inspector captures; no real wallpaper was controlled. / Captures d'inspecteur hors ligne ; aucun fond réel n'a été piloté.

## Macro ON/OFF / Macro ON/OFF

The first-party v1.0.6 bundle includes OLED Saver 1.0.4 and the new SignalRGB Lighting 1.0.3; Google Home remains 1.0.5. The plugin store points to each plugin's own version, not to every archive in a bundle. / Le lot maison v1.0.6 contient OLED Saver 1.0.4 et le nouveau SignalRGB 1.0.3 ; Google Home reste en 1.0.5. Le magasin référence la version propre à chaque plugin, pas chaque archive du lot.

EN: Add two OLED Saver blocks set to ON, one per screen, then one SignalRGB Lighting block set to OFF, and a Google Home 1.0.5 group block set to OFF with the selected lamps (1–20 device IDs). To restore, use OLED Saver OFF for each screen, SignalRGB ON, and Google Home group ON. An existing OLED key without `mode` remains Toggle. SignalRGB affects the entire lighting canvas, not individual lights: OFF sends black but does not cut physical power. SignalRGB's Local API may require Pro and reject a Free installation with HTTP 403. The Windows VM build, offline plugin tests and NovyDeck block-settings test passed; SignalRGB, a Google account and real hardware were not exercised. Macro execution/order has not been end-to-end tested against devices.

FR : Ajoutez deux blocs OLED Saver en mode ON, un par écran, puis un bloc Éclairage SignalRGB en mode OFF et un bloc Google Home 1.0.5 en scope groupe / mode OFF pour les lampes choisies (1 à 20 ID). Pour restaurer : OLED Saver OFF sur chaque écran, SignalRGB ON et Google Home groupe ON. Une touche OLED existante sans `mode` reste en bascule. SignalRGB agit sur le canvas global et OFF envoie du noir sans couper l'alimentation physique. Son API locale peut exiger SignalRGB Pro et refuser une installation gratuite avec HTTP 403. Compilation VM Windows, tests hors ligne des plugins et test des réglages de blocs NovyDeck réussis ; ni SignalRGB, ni compte Google, ni matériel réel n'ont été exercés. Exécution et ordre d'une macro non validés de bout en bout sur des appareils.


## New in 1.0.4 / Nouveautés 1.0.4

**Google Home:** choose **Toggle ON/OFF** to read and invert the device's real state. Select **Group (selected devices)** and use **+** to add up to 20 devices from the same home, or **−** to remove one. For groups and rooms, all ON turns all OFF; otherwise all turn ON. Missing state or an unsupported target blocks the whole update before any write. Saved single-device settings and encrypted credentials are preserved.

**Google Home :** choisissez **Basculer ON/OFF** pour lire et inverser l'état réel. Sélectionnez **Groupe (appareils choisis)** puis **+** pour ajouter jusqu'à 20 appareils de la même maison, ou **−** pour en retirer un. Groupe/pièce : tous allumés → tous éteints ; sinon tous allumés. Un état inconnu ou un appareil incompatible bloque l'envoi avant toute écriture. Les réglages existants et la connexion chiffrée sont conservés.

**Wallpaper Engine:** the existing Pause action now alternates **Pause / Play** globally. Its state is local to the plugin, resets when it restarts and does not detect changes made outside the plugin. Explicit Play/Stop remain available. The official Next Wallpaper command and the installed 1.0.3 plugin were tested on a real Windows installation and changed only the selected display; this does not establish the cause of every NovyDeck key-trigger issue.

**Wallpaper Engine :** l'action Pause existante alterne désormais **Pause / Lecture** sur tous les écrans. Son état local repart à zéro au redémarrage du plugin et ne détecte pas les changements effectués ailleurs. Lecture et Arrêt explicites restent disponibles. La commande officielle et le plugin 1.0.3 installé ont changé le fond de l'écran ciblé lors d'essais réels ; le déclenchement depuis chaque touche NovyDeck reste à confirmer.

| New settings / Nouveaux réglages | English | Français |
| --- | --- | --- |
| Google Home group and toggle / groupe et bascule | ![Google Home group settings](screenshots/googlehome/googlehome-group-en.png) | ![Groupe Google Home](screenshots/googlehome/googlehome-group-fr.png) |
| Wallpaper Engine Pause / Play | ![Pause and play](screenshots/wallpaperengine/wallpaperengine-pause-en.png) | ![Pause et lecture](screenshots/wallpaperengine/wallpaperengine-pause-fr.png) |

Inspector images use offline test devices, not live account data. / Les images des inspecteurs utilisent des appareils de test hors ligne, pas des données de compte réel.

## New in 1.0.3 / Nouveautés 1.0.3

Install from NovyDeck's plugin store, then return to the deck and find the installed plugin in **Action library → Plugins**. Drag its action onto a key to configure it. Google Assistant Command and Google Home are two separate plugins.

Installez depuis le magasin de plugins de NovyDeck, puis revenez au deck : retrouvez le plugin installé dans **Bibliothèque d'actions → Plugins**, et glissez son action sur une touche pour la régler. Commande Google Assistant et Google Home sont deux plugins distincts.

| Plugin | English inspector | Inspecteur français |
| --- | --- | --- |
| Google Home (experimental / expérimental) | ![Google Home setup](screenshots/googlehome/googlehome-en.png) | ![Connexion Google Home](screenshots/googlehome/googlehome-fr.png) |
| Wallpaper Engine — wallpaper / fond | ![Wallpaper and display](screenshots/wallpaperengine/wallpaperengine-wallpaper-en.png) | ![Fond et écran](screenshots/wallpaperengine/wallpaperengine-wallpaper-fr.png) |
| Wallpaper Engine — playlist | ![Playlist and display](screenshots/wallpaperengine/wallpaperengine-playlist-en.png) | ![Playlist et écran](screenshots/wallpaperengine/wallpaperengine-playlist-fr.png) |

These are real inspector captures initialized with local protocol fixtures, not proof of a Google account connection or a wallpaper change. / Captures réelles des inspecteurs initialisés avec des messages locaux de test, pas une preuve de connexion Google ou de changement de fond.

This workspace contains the SDK and ten Windows plugins: Clock, Counter, System Monitor, Now Playing, OLED Saver, SignalRGB Lighting, Home Assistant, Google Assistant Command, Google Home and Wallpaper Engine. The SDK speaks the local Stream Deck WebSocket plugin protocol and renders 144×144 PNG keys with bundled DejaVu Sans (license in `novydeck-sdk/assets/DEJAVU-LICENSE.txt`). No Node runtime or CDN is required. Source and bundles are MIT licensed; the font retains its own license. See each manifest for its version; not every plugin is bumped in a bundle.

On Windows: `cargo generate-lockfile`, `cargo build --release --locked`, `cargo test --locked`, `cargo run --release --locked --bin render-previews`, `python tools/make_icons.py`, `python tools/capture_inspectors.py`, `python tools/package.py`. Icon generation needs Pillow; inspector previews use installed Microsoft Edge, offline with a fake local WebSocket. CI builds/tests on Windows and uploads plugin archives, `SHA256SUMS.txt` and previews for manual runs. Only `first-party-v*` tags publish releases; a branch run does not publish. Do not copy the registry template into `registry.json` until release URLs, SHA-256 and schema-validated metadata exist.

Home Assistant requires your own server and token. Google Assistant Command uses your own Google Cloud project, the Assistant API and desktop OAuth to send typed phrases; it does not directly select a device or room. Google Home instead controls devices or rooms through an unofficial private Google Home API: supply your own account email and a one-time `oauth2_4/` token from `accounts.google.com/EmbeddedSetup`. Google Home 1.0.4 adds a toggle mode alongside the saved on/off/brightness modes: each press reads `HomeControlService/GetTraits`, then reverses the selected device; for a room, all on turns all off, otherwise all turn on. It reads every selected device before sending one update; unsupported devices, missing/unknown state, or HTTP failures send no update. Two overlapping button presses are rejected while a command is running. Room selection uses stable home and room IDs, not names; button images are not live state indicators. Treat the token as account-wide access, never share it. Only an encrypted master token is retained under the Windows user's DPAPI; button settings must not contain tokens. The private API may change or stop working. Tests use anonymized offline shapes from piekstra/google-home-cli MIT commit `1370939af0bd3c1b85bd68701832833a8b7a045e` (`src/traits.rs`, `src/foyer.rs`), not live Google responses; no account or device was exercised.

Wallpaper Engine requires your own installed, running Steam version on Windows 10+. Choose `wallpaper32.exe` or `wallpaper64.exe` if not in standard Steam directories. Open a local project/media file or a previously saved playlist on one display (screen number starts at 1), go to the next wallpaper on a display, or apply a saved profile globally. Pause/play/stop/mute/unmute are global. Wallpaper Engine software and wallpaper content are proprietary and are NOT included or redistributed in these plugin archives; no real installation was exercised by CI.

En français : Google Home et Wallpaper Engine passent en 1.0.4 ; les sept autres plugins et le workspace restent en 1.0.3. « Commande Google Assistant » transmet une phrase via votre projet Cloud/OAuth ; « Google Home » pilote directement appareils ou pièces avec une API privée non officielle et stockage DPAPI. Sa nouvelle option « Basculer ON/OFF » lit l’état réel à chaque appui : appareil → état inverse ; pièce → si tous allumés, tout éteindre, sinon tout allumer. Si la lecture échoue, si un état manque ou si un appareil n’annonce pas OnOff, aucune commande n’est envoyée. Deux appuis simultanés ne sont pas exécutés en parallèle. Les images personnalisées ne reflètent pas l’état en direct. Ne partagez jamais le jeton : l’API peut changer et aucun compte réel n’a été testé. Wallpaper Engine exige votre propre installation Steam déjà lancée ; seuls les contrôles du plugin sont distribués, pas l'application ni les fonds. Les aperçus hors ligne et les tests CI ne constituent pas une validation sur compte, appareil ou service réel.
