# Official NovyDeck plugins 1.0.4

## English

### Google Home 1.0.4
- New **Toggle ON/OFF** action mode reads the actual device state before sending the opposite state.
- New **Group (selected devices)** mode: add devices with **+**, remove them with **−**, and control up to 20 devices from the same home with one key.
- Group/room toggle: if all selected devices are ON, turn all OFF; otherwise turn all ON. Validate every target and read every required state before sending an update; missing state, invalid/duplicate IDs or unsupported capability refuse the operation.
- Preserve existing on/off/brightness settings and DPAPI-encrypted account credentials. Overlapping presses are rejected while an operation is running.

### Wallpaper Engine 1.0.4
- The existing **Pause** action now alternates global **Pause / Play** on repeated presses. Explicit Play and Stop remain available.
- Playback tracking is local to the plugin, resets on restart, and does not detect changes made elsewhere in Wallpaper Engine. Its indication changes only after a successful CLI process.
- Clarified screen/playlist guidance for Next Wallpaper. The official command and the installed 1.0.3 plugin successfully changed a selected screen on a real Windows PC. The cause of the user's key-trigger issue is not established; no universal Next fix is claimed.

Update Google Home and Wallpaper Engine from the refreshed NovyDeck plugin store. Other plugin versions remain 1.0.3. Wallpaper Engine software/content are not included. Google Home still uses an unofficial private API that may stop working; new toggle/group operations have not been tested against the user's Google devices. CI and offline fixtures are not a physical-device qualification.

## Français

### Google Home 1.0.4
- Nouveau mode **Basculer ON/OFF** : lit l'état réel avant d'envoyer l'état inverse.
- Nouveau mode **Groupe (appareils choisis)** : ajoutez avec **+**, retirez avec **−**, et commandez jusqu'à 20 appareils de la même maison depuis une seule touche.
- Bascule groupe/pièce : si tous sont allumés, tout éteindre ; sinon, tout allumer. Toutes les cibles et tous les états nécessaires sont validés avant l'envoi ; état manquant, identifiant invalide/dupliqué ou capacité absente font refuser l'opération.
- Réglages Allumer/Éteindre/Luminosité et connexion chiffrée DPAPI conservés. Les appuis concurrents sont refusés pendant une commande.

### Wallpaper Engine 1.0.4
- La touche **Pause** existante alterne désormais **Pause / Lecture** sur tous les écrans. Lecture et Arrêt explicites restent disponibles.
- État de lecture local au plugin, réinitialisé au redémarrage et non synchronisé avec les changements faits ailleurs dans Wallpaper Engine. L'indication change seulement après succès du processus CLI.
- Aide écran/playlist de Fond suivant clarifiée. La commande officielle et le plugin 1.0.3 installé ont changé le fond d'un écran ciblé sur le PC réel. La cause du problème de déclenchement depuis la touche reste indéterminée ; aucun correctif universel de Suivant n'est revendiqué.

Actualisez le magasin NovyDeck puis mettez à jour Google Home et Wallpaper Engine. Les autres plugins restent en 1.0.3. Le logiciel Wallpaper Engine et ses fonds ne sont pas inclus. Google Home utilise toujours une API privée non officielle pouvant cesser de fonctionner ; les nouvelles opérations groupe/bascule n'ont pas été essayées sur les appareils Google de l'utilisateur. CI et tests hors ligne ne constituent pas une qualification matérielle.
