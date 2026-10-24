# First-party plugins / Plugins maison

## New in 1.0.3 / Nouveautés 1.0.3

Install from NovyDeck's plugin store, then return to the deck and find the installed plugin in **Action library → Plugins**. Drag its action onto a key to configure it. Google Assistant Command and Google Home are two separate plugins.

Installez depuis le magasin de plugins de NovyDeck, puis revenez au deck : retrouvez le plugin installé dans **Bibliothèque d'actions → Plugins**, et glissez son action sur une touche pour la régler. Commande Google Assistant et Google Home sont deux plugins distincts.

| Plugin | English inspector | Inspecteur français |
| --- | --- | --- |
| Google Home (experimental / expérimental) | ![Google Home setup](screenshots/googlehome/googlehome-en.png) | ![Connexion Google Home](screenshots/googlehome/googlehome-fr.png) |
| Wallpaper Engine — wallpaper / fond | ![Wallpaper and display](screenshots/wallpaperengine/wallpaperengine-wallpaper-en.png) | ![Fond et écran](screenshots/wallpaperengine/wallpaperengine-wallpaper-fr.png) |
| Wallpaper Engine — playlist | ![Playlist and display](screenshots/wallpaperengine/wallpaperengine-playlist-en.png) | ![Playlist et écran](screenshots/wallpaperengine/wallpaperengine-playlist-fr.png) |

These are real inspector captures initialized with local protocol fixtures, not proof of a Google account connection or a wallpaper change. / Captures réelles des inspecteurs initialisés avec des messages locaux de test, pas une preuve de connexion Google ou de changement de fond.

This workspace contains the SDK and nine Windows plugins: Clock, Counter, System Monitor, Now Playing, OLED Saver, Home Assistant, Google Assistant Command, Google Home and Wallpaper Engine. The SDK speaks the local Stream Deck WebSocket plugin protocol and renders 144×144 PNG keys with bundled DejaVu Sans (license in `novydeck-sdk/assets/DEJAVU-LICENSE.txt`). No Node runtime or CDN is required. Source and bundles are MIT licensed; the font retains its own license. The nine plugin manifests and the Rust workspace use version 1.0.3.

On Windows: `cargo generate-lockfile`, `cargo build --release --locked`, `cargo test --locked`, `cargo run --release --locked --bin render-previews`, `python tools/make_icons.py`, `python tools/capture_inspectors.py`, `python tools/package.py`. Icon generation needs Pillow; inspector previews use installed Microsoft Edge, offline with a fake local WebSocket. CI builds/tests on Windows and uploads plugin archives, `SHA256SUMS.txt` and previews for manual runs. Only `first-party-v*` tags publish releases; a branch run does not publish. Do not copy the registry template into `registry.json` until release URLs, SHA-256 and schema-validated metadata exist.

Home Assistant requires your own server and token. Google Assistant Command uses your own Google Cloud project, the Assistant API and desktop OAuth to send typed phrases; it does not directly select a device or room. Google Home instead controls devices or rooms through an unofficial private Google Home API: supply your own account email and a one-time `oauth2_4/` token from `accounts.google.com/EmbeddedSetup`. Treat it as account-wide access, never share it. Only an encrypted master token is retained under the Windows user's DPAPI; button settings must not contain tokens. The private API may change or stop working. Neither integration is verified against a real account/server.

Wallpaper Engine requires your own installed, running Steam version on Windows 10+. Choose `wallpaper32.exe` or `wallpaper64.exe` if not in standard Steam directories. Open a local project/media file or a previously saved playlist on one display (screen number starts at 1), go to the next wallpaper on a display, or apply a saved profile globally. Pause/play/stop/mute/unmute are global. Wallpaper Engine software and wallpaper content are proprietary and are NOT included or redistributed in these plugin archives; no real installation was exercised by CI.

En français : les neuf plugins Windows et le workspace sont en version 1.0.3. « Commande Google Assistant » transmet une phrase via votre projet Cloud/OAuth ; « Google Home » pilote directement appareils ou pièces avec une API privée non officielle, un jeton de compte personnel à usage unique, puis stockage DPAPI. Ne partagez jamais ce jeton : l'API peut changer et aucun compte réel n'a été testé. Wallpaper Engine exige votre propre installation Steam déjà lancée ; seuls les contrôles du plugin sont distribués, pas l'application ni les fonds. Les aperçus hors ligne et les tests CI ne constituent pas une validation sur compte, appareil ou service réel.
