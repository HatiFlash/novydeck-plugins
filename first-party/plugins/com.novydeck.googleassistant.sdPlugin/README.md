# NovyDeck Google Assistant Command / Commande Google Assistant

EN: Send a typed Google Assistant command to devices linked to your Google Home account, without Home Assistant. Windows 10+, internet access and your own Google Cloud project are required.

FR : Envoyez une commande écrite aux appareils liés à votre compte Google Home, sans Home Assistant. Windows 10+, Internet et votre propre projet Google Cloud sont nécessaires.

## Setup / Configuration (6 steps)

1. Create your own project at https://console.cloud.google.com/ / Créez votre projet Google Cloud.
2. Enable **Google Assistant API** / Activez « Google Assistant API ».
3. Configure OAuth consent in **Testing** mode / Configurez l'écran de consentement en mode **Test**.
4. Add your Google email as a **test user** / Ajoutez votre adresse Google comme utilisateur de test.
5. Create an OAuth client of type **Desktop application**, download its JSON / Créez un client OAuth « Application de bureau », téléchargez son JSON.
6. In the button inspector, import this JSON, click Connect, authorize in the browser, enter a phrase and language (`fr-FR` by default), then press Test / Dans l'inspecteur, importez le JSON, connectez-vous, autorisez dans le navigateur, saisissez phrase et langue, puis testez.

Client credentials and refresh token are encrypted for the current Windows user with DPAPI at `%APPDATA%\NovyDeck\plugins\com.novydeck.googleassistant\secrets.bin`. Only phrase and language are stored in exportable action settings. No telemetry. Disconnect removes the encrypted credentials file. OAuth uses a one-time loopback callback (5 minutes) and PKCE. Commands only run when pressing the button or Test.

Identifiants et jeton de rafraîchissement chiffrés par DPAPI pour l'utilisateur Windows dans ce fichier. Seules la phrase et la langue apparaissent dans les réglages exportables. Aucune télémétrie. Déconnecter supprime les identifiants. OAuth : retour local à usage unique (5 minutes) + PKCE. Rien n'est envoyé à Google au démarrage.

## Limitations / Limites

The Google Assistant SDK `assistant-sdk-prototype` scope and API may stop working or be quota-limited; the account must have the devices in Google Home. Replies are generally audio-only; this plugin discards audio and cannot verify physical execution, only successful gRPC completion. A Google-side OAuth grant may need separate revocation via your account. OAuth apps in Testing mode may require reauthorization when refresh tokens expire. This plugin uses `device_model_id=default`, `device_id=default`, as in `gassist-text` used by Home Assistant, rather than inventing an unregistered model; Google's protocol describes device registration and may reject an unregistered model for some accounts.

L'API prototype peut disparaître et être limitée par quota ; les appareils doivent figurer dans Google Home du compte. La réponse est généralement audio uniquement : le plugin l'ignore et ne vérifie que la fin gRPC, pas l'action physique. La révocation côté Google est distincte de « Déconnecter ». En mode Test, Google peut imposer une nouvelle autorisation. Identifiants d'appareil « default » comme `gassist-text` (Home Assistant) ; l'enregistrement d'un modèle peut être exigé selon le compte.

Protocol reference: https://github.com/googleapis/googleapis/blob/master/google/assistant/embedded/v1alpha2/embedded_assistant.proto
Practical defaults: https://github.com/tronikos/gassist_text/blob/main/src/gassist_text/textinput.py
