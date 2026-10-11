# État local et reprise des opérations

Version : 0.1.49

L’identité Signal et les conversations sont enregistrées dans un seul instantané
chiffré par compte. Le remplacement atomique du fichier conserve ensemble les
compteurs cryptographiques, les identifiants traités et les messages à publier.
Sous Linux, le répertoire est synchronisé après le remplacement du fichier.

La migration lit l’ancien stockage, écrit l’instantané puis supprime les anciens
fichiers concernés. Le format local de cette version ne permet pas un retour à
un ancien client avec le même profil. Le format des messages sur GitHub reste
inchangé.

La file d’envoi conserve jusqu’à 500 publications, indépendamment des 500
messages du cache. L’instantané est limité à 24 Mio. Un envoi accepté localement
garde son identifiant lors des reprises ; un échec de publication le laisse en
attente. La lecture utilise les arbres Git pour dépasser la limite de l’API
Contents, avec au plus 10 000 fichiers par conversation et 8 Mio par réponse.
Un arbre tronqué est refusé.

Retirer un ami révoque ses permissions et termine la session distante.
Se déconnecter ou changer de compte arrête l’hôte local avant de modifier
l’identité. Une machine dont la clé change ne reçoit plus automatiquement les
permissions du compte ; sa nouvelle clé doit être validée explicitement.

Les requêtes GitHub partagent un délai de reprise et un budget entre clients
clonés. Les scans de machines sont espacés par compte, y compris après un échec.
L’installation automatique attend la fin des appels, des sessions distantes et
de la file d’envoi.
