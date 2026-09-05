# Sécurité

Noosphere manipule des clés privées, un jeton GitHub et des messages personnels.
N’ouvrez pas d’issue publique contenant l’une de ces données.

## Signaler une vulnérabilité

Utilisez le signalement privé de GitHub dans l’onglet **Security** du dépôt.
Indiquez la version concernée, les étapes de reproduction et l’impact possible.
Retirez les jetons, messages et informations personnelles des journaux joints.

## Modèle de sécurité

- Les messages et les signaux WebRTC sont chiffrés avec libsignal avant d’être
  écrits sur GitHub.
- Les clés privées et l’état des conversations restent sur l’ordinateur.
- Sous Windows, les secrets locaux sont protégés avec Credential Manager et
  DPAPI. Sous Linux, ils utilisent Secret Service.
- Une clé déjà connue ne peut pas être remplacée silencieusement. Noosphere
  signale le changement au lieu de poursuivre la conversation.
- Les réponses GitHub, profils publics, chemins et données reçues d’un pair sont
  validés avant leur utilisation.
- Le frontend n’a pas accès directement au jeton GitHub ni aux clés de
  chiffrement.
- Chaque profil ouvert possède son propre stockage local et son propre verrou.

## Limites

Noosphere n’a pas encore fait l’objet d’un audit de sécurité indépendant.

GitHub voit l’existence du dépôt, les dates de commit, le volume d’activité et
les signaux d’étoile. Les fichiers supprimés peuvent rester présents dans
l’historique Git. WebRTC expose les informations réseau nécessaires à la mise en
relation des pairs et peut échouer sur un réseau qui exige un relais TURN.

La vérification initiale d’une clé repose sur TOFU. Pour une conversation
sensible, comparez les identifiants de clé par un autre canal. Un ordinateur ou
un compte GitHub compromis ne fait pas partie des menaces contre lesquelles le
client peut se protéger.

Le binaire Windows fourni n’est pas signé avec un certificat Authenticode.
