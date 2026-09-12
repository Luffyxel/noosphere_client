# Laboratoire bureau à distance

Le laboratoire lance deux processus indépendants qui représentent deux ordinateurs Noosphere reliés au même compte GitHub. Chaque processus possède sa propre identité cryptographique et son propre `machineId`.

La signalisation GitHub est remplacée par un dossier local. Les inscriptions de machines sont signées et l’offre de session est chiffrée pour la machine destinataire. Le média utilise une vraie connexion QUIC UDP avec authentification mutuelle, liaison à l’identité Noosphere et QUIC DATAGRAM.

```powershell
npm run test:remote:e2e
```

Le scénario vérifie automatiquement :

- la découverte de deux machines d’un même compte ;
- le refus d’une substitution d’identité ou de machine ;
- l’établissement de la session QUIC authentifiée ;
- la transmission de trames fragmentées et réordonnées ;
- l’abandon d’une trame incomplète sans bloquer les suivantes ;
- l’envoi d’une commande d’entrée autorisée ;
- l’application immédiate d’une révocation.

Le résultat se trouve dans `remote-test-results/two-node-lab/report.json`. Les échanges du faux dépôt GitHub sont conservés sous `github-folder` pour faciliter le diagnostic. Aucune clé privée n’y est écrite.

Ce laboratoire valide le protocole et ses limites de sécurité sans VM. Une VM reste utile pour valider les différences de pilote graphique, de pare-feu et de système d’exploitation. Elle n’est pas nécessaire pour détecter les erreurs de signalisation, d’identité, de packetisation ou de permissions couvertes ici.
