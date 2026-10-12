# Noosnote

Noosnote rassemble des pages et des tables dans Noosphere. Le bouton **+** de
la barre latérale permet de créer une discussion ou un Noosnote. Chaque
Noosnote dispose d’une icône, d’une couleur et d’un arbre de pages.

Le clic droit sur une page, un résultat de recherche ou un élément de table
permet d’ouvrir, renommer, changer l’icône, créer une sous-page, dupliquer,
déplacer, exporter ou mettre à la corbeille. **Maj+F10** ouvre aussi ce menu.
Les éléments de table gardent leur place dans la table ; leurs catégories
se changent depuis les propriétés ou les colonnes.

Les icônes se choisissent dans un catalogue avec recherche en français,
catégories et teintes de peau. Les emojis utilisent la police du système ;
il est aussi possible de coller un emoji depuis le sélecteur du bureau.
Les couleurs comprennent une palette étendue, le sélecteur du système et
un code hexadécimal personnalisé.

Le format de stockage reste en version 1, indépendamment du numéro de
l’application. Les notes de la 0.2.0 restent accessibles et éditables après
la mise à jour sans recréer le dépôt. Les nouveaux emojis conservent
une icône de remplacement compatible avec la 0.2.0.

## Fichiers dans une page

Le bouton **+** de l’éditeur propose **Fichier**. Il est aussi possible de
glisser un fichier dans le contenu d’une page. Chaque fichier apparaît sous
forme de carte, avec son nom, son type et sa taille, jusqu’à 20 Mio.

**Aperçu** déplie un PDF directement dans la page. Le lecteur permet de
changer de page, zoomer, tourner le document, sélectionner le texte et
télécharger le fichier. Un PDF protégé demande son mot de passe.

**Modifier** ouvre un document Word comme une page Noosnote : texte, titres,
listes et tableaux sont éditables et enregistrés automatiquement. **Exporter
en Word** télécharge la version modifiée en `.docx` ; **Original** conserve
le fichier importé. Les anciens `.doc` sont convertis en texte. Les images
et les mises en page complexes restent disponibles dans l’original.

Les originaux sont stockés séparément du contenu éditable, dans
`notes/<noosnote-id>/attachments/<fichier-id>/`. Les fichiers sont découpés
en parties de 4 Mio pour la synchronisation ; leur page et leurs parties
sont publiées dans le même commit privé. Les parties inchangées sont
réutilisées. L’original se charge à l’ouverture de son aperçu ou à son
téléchargement et reste ensuite disponible dans le cache local protégé.

## Premier Noosnote

1. Cliquer sur **+**, puis **Noosnote**.
2. Choisir un nom, une icône, une couleur et une page vierge ou le modèle École.
3. Accepter la création du dépôt privé dédié.
4. Si GitHub demande une configuration, créer le dépôt depuis le lien proposé,
   puis l’ajouter aux dépôts sélectionnés dans l’installation Noosphere.
5. Revenir dans Noosphere et cliquer sur **Vérifier et créer**.

L’application utilise `noosnote_<identifiant GitHub>`. Cet identifiant numérique
reste stable lors d’un changement de pseudonyme. Tous les Noosnotes du compte
partagent ce dépôt privé. Une autre machine connectée au même compte découvre
le dépôt existant et récupère les espaces et les tables.

L’installation GitHub conserve le mode **Only select repositories** : le dépôt
public de messagerie et, après activation, le dépôt privé de notes. Elle
n’obtient pas accès aux autres dépôts. La création automatique nécessite la
permission GitHub App `repository_creation: write`. Avec l’autorisation
actuelle, les liens de configuration permettent de terminer sur GitHub.

Les notes sont des fichiers JSON lisibles dans ce dépôt privé. Les personnes
auxquelles le propriétaire donne accès au dépôt, ainsi que GitHub, peuvent en
lire le contenu. Le chiffrement de bout en bout de la messagerie reste distinct.
Les brouillons et les copies de récupération sur la machine utilisent le
stockage protégé de Noosphere.

## Pages et tables

Une page accepte du texte mis en forme, des titres, des listes, des tâches,
des citations, du code, des liens et des sous-pages. **+** dans l’éditeur, ou
**/** au début d’un paragraphe, insère une table ou une sous-page.

Chaque élément de table ouvre une page vierge. Cette page peut elle-même
contenir du texte et des tables. Le modèle École prépare une table avec une
catégorie **Cours**, les éléments **Anglais** et **Français**, et leurs pages
vierges. Dans Anglais, une table **Journées de cours** peut ensuite contenir
une page par journée.

Les propriétés comprennent titre, texte, nombre, catégorie, étiquettes, date,
case à cocher, URL, e-mail, téléphone, relation, formule et agrégation.
Les catégories ont un nom et une couleur. Une relation désigne des éléments
d’une autre table ; l’agrégation compte ou calcule les valeurs liées.

Les vues enregistrent leur nom, les propriétés visibles, les filtres, les tris
et les groupes. Les présentations disponibles sont table, colonnes, liste,
galerie et calendrier. Le calendrier utilise une propriété Date. Les colonnes
permettent de déplacer les cartes entre catégories ; les propriétés restent
modifiables depuis chaque carte.

Exemples de formules :

```text
prop("Note") * 2
if(prop("Terminé"), "Fait", "À faire")
round(average(prop("Anglais"), prop("Français")), 1)
```

L’évaluateur accepte les opérateurs arithmétiques, les comparaisons et des
fonctions de texte, de nombres et de dates. Il ne lance pas de JavaScript.
Une propriété manquante ou une référence circulaire affiche une erreur dans
la cellule.

## Enregistrement et synchronisation

Les modifications sont enregistrées localement après une courte pause de
saisie, avec un délai maximal de 1,8 seconde. Les envois automatiques sont
regroupés et espacés d’au moins une minute. **Ctrl+S**, **Cmd+S** ou le bouton
de synchronisation lance un envoi immédiat. L’état dans la barre latérale
distingue l’enregistrement local de la synchronisation terminée.

Les pages déjà ouvertes et les brouillons restent consultables hors ligne.
Les autres pages sont téléchargées à leur première ouverture. La recherche
porte sur les titres et le texte des pages déjà chargées.

Les modifications indépendantes sont fusionnées. Deux modifications
incompatibles d’un même contenu demandent un choix de version. Les deux
versions peuvent être téléchargées avant ce choix ; une copie de récupération
reste protégée sur la machine. Un nouvel envoi ne force jamais la branche
GitHub lorsqu’une autre machine l’a modifiée entre-temps.

Les pages mises à la corbeille, y compris les pages d’éléments de table,
restent restaurables. Les tables s’exportent en CSV ; une page s’exporte en
JSON depuis son menu. Ces exports sont des copies du contenu courant.

## Organisation du dépôt

```text
.noosnote.json
notes/
  <noosnote-id>/
    workspace.json
    pages/
      <page-id>.json
    databases/
      <table-id>.json
```

`workspace.json` contient le nom, l’apparence et l’index de navigation des
pages. Une page contient son document structuré. Une table contient ses
propriétés, ses lignes et ses vues ; chaque ligne référence l’identifiant de
sa page. Les noms affichés ne servent pas de chemins. Renommer une page ou
la déplacer conserve ses références et son fichier.

La synchronisation compare d’abord les identifiants Git des fichiers. Elle
récupère les métadonnées nécessaires et les pages ouvertes qui ont changé,
puis crée un arbre et un commit regroupant les brouillons. Les fichiers sans
modification ne sont pas réécrits.

Le format actuel accepte 4 096 pages par Noosnote, 5 000 éléments, 64 propriétés
et 24 vues par table, avec une taille maximale de 1 Mio par fichier. Les vues
affichent les éléments par lots de 100. La mémoire conserve 24 pages récentes
et les brouillons ; le cache protégé sur disque conserve les pages téléchargées.
