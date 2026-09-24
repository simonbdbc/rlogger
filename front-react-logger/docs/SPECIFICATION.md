# Spécification fonctionnelle du lecteur local

Lecteur 0.5.1 : socle initial conservé. L’extension du protocole 0.3.0 est
décrite dans
l’[architecture livrée](ARCHITECTURE-LOCALE.md#gestion-horaire--extension-030),
le [guide](../README.md) et le [contrat actuel des actions](../../docs/11-actions-fichiers-dossiers.md).

La consultation et les tailles sont génériques : contenu et allocation par
fichier/dossier et total racine, état de calcul, partiel et indisponible explicites.
Tout fichier régulier ou dossier visible peut être téléchargé ou supprimé
explicitement, indépendamment du format et de l’heure, selon les permissions du
système. Un dossier se télécharge en TAR non compressé et se supprime avec tout
son contenu après confirmation modale nom/chemin ; Échap annule et le retrait
intervient après succès.
Un actif publié conserve ID, sélection et fenêtre ; aucun segment suivant concaténé.
Les erreurs conservent l’entrée ; les récupérés sont identifiés. La maintenance
automatique n’efface que les dossiers anciens vides et récupère sous bail OS pour
une racine RLOGGER 2 privée. Une racine non privée reste consultable, avec raison
visible de la désactivation dans le parcours RLOGGER. Le parcours externe ne
demande jamais cette maintenance et ne montre pas cet avertissement. Les deux
parcours ont des chemins mémorisés distincts ; sélectionner une entrée ferme la
session précédente, puis ouvre automatiquement son dossier configuré.
Les rafraîchissements manuels de l’inventaire sont
limités à un nouveau scan par racine toutes les 60 secondes.

## Périmètre de la première livraison

L’utilisateur lance l’application sur sa machine, indique un dossier, parcourt
l’arbre à gauche, clique sur un fichier et lit son contenu mis à jour à droite.
Le dossier peut être la racine de tous les logs, un run, une instance ou un dossier
ordinaire. Aucune structure de noms particulière n’est obligatoire.

Expo et gluestack sont imposés. Le lecteur vise le web desktop. Aucun écran de
login, gestion de comptes, connexion réseau distante ou téléversement de logs
vers un service tiers n’est inclus.

## Écran principal

### Barre du dossier

- Champ « Dossier local » acceptant un chemin absolu.
- Action « Ouvrir », déclenchable avec Entrée.
- Chemin actif visible ; le texte en cours de saisie reste distinct du chemin actif.
- Erreur explicite : absent, non lisible, fichier fourni à la place d’un dossier,
  chemin non absolu ou service local indisponible.
- Une ouverture échouée conserve le dossier actif précédent ; une ouverture réussie
  annule l’ancien flux et réinitialise arbre et fichier sélectionné.
- Les espaces et caractères Unicode sont des caractères de chemin ordinaires.
  Ne pas interpréter `pwd`, `$(...)`, variables ou commandes shell. L’utilisateur
  colle le résultat de `pwd`, pas la commande.

Chaque parcours mémorise son dernier chemin ; le parcours choisi et la largeur
des panneaux sont aussi mémorisés localement. Aucun contenu de log n’est persisté
dans le stockage du navigateur par défaut. Le dossier mémorisé est automatiquement
revalidé à son ouverture.

### Arborescence gauche

- Dossiers dépliables/repliables avec icône, nom et état de chargement.
- Fichier actif identifiable autrement que par sa seule couleur.
- Scroll indépendant de celui du lecteur ; noms longs accessibles en entier.
- Mise à jour à la création, suppression ou rotation, plus bouton « Actualiser ».
- Conservation des dossiers ouverts pendant une actualisation.
- Chargement progressif des branches et pagination des grands répertoires.
- Tous les fichiers réguliers non cachés sont proposés, quelle que soit leur
  extension ; les dossiers restent explorables sans scanner récursivement tout
  leur contenu au démarrage. Les entrées cachées restent prises en compte dans
  les tailles et les actions récursives sur leur parent.

Tri livré : noms décroissants dans chaque dossier, sans interpréter les dates,
le métier ou le type d’entrée. L’arbre ne doit
pas se réordonner à chaque append sur la seule base du mtime. Ce tri de navigation
n’a aucun effet sur l’ordre des lignes lues.

### Lecteur droit

- En-tête avec nom du fichier et état du suivi ; chemin relatif complet accessible
  dans l’infobulle de l’entrée sélectionnée dans l’arbre.
- Texte brut monospace, fond sombre et contraste lisible, inspirés de CardLog.
- Espaces, retours à la ligne et ordre du fichier conservés ; aucun rendu HTML ou
  exécution de séquences terminales provenant du contenu.
- Texte sélectionnable et copiable, scroll vertical et gestion des lignes longues.
- Au clic : état de chargement lié à ce fichier, puis contenu récent et suivi actif.
- Pour un petit fichier, afficher tout le contenu. Pour un gros fichier, ouvrir une
  tranche de fin bornée et annoncer que l’historique antérieur peut être chargé.
- « Charger plus ancien » donne accès au contenu précédent sans tout conserver.
- Fichier vide : état explicite tout en continuant à attendre des ajouts.

La lecture brute n’a besoin d’aucun parseur RLOGGER. Les logs Java ou autres
fichiers texte restent affichables. Une coloration ou un panneau de contexte
structuré est une évolution, avec repli sur le texte original si le format est inconnu.

## Trois comportements distincts

### Actualisation du contenu sélectionné

Toujours active quand le fichier est ouvert : les nouvelles données rejoignent
le modèle du lecteur. Cliquer sur un autre fichier ferme/annule le suivi précédent.
Une réponse retardée de l’ancien fichier ne doit jamais apparaître dans le nouveau.

### Défilement automatique

Actif lorsque l’utilisateur reste au bas de la vue récente. S’il remonte dans
l’historique, conserver son point de lecture et afficher « N nouvelles lignes »
si ce nombre est connu, sinon « Nouvelles données ». « Aller en bas » retrouve le
flux récent. Recevoir des données ne signifie pas forcer le scroll.

Sous forte charge pendant la lecture historique, préserver la tranche consultée
et le curseur ; les nouveautés au-delà du budget seront relues depuis le fichier
au retour en bas. Ne pas accumuler une file illimitée dans le navigateur.

### Suivi du fichier successeur

Option différée à FRONT-028. Les paragraphes suivants
décrivent son cadrage futur ; aucun réglage de suivi du successeur n’est livré
dans le lecteur 0.5.1.

Si activée, suivre une destination précise dans un run et une instance connus.
Une rotation `http-08.log` → `http-09.log` peut faire changer la sélection et ouvrir
les dossiers parents. Ne pas passer au fichier le plus récemment modifié de toute
la racine, qui pourrait appartenir à une autre instance ou destination.

Pour une arborescence générique sans notion fiable de successeur, ne pas deviner
à partir du seul mtime : proposer le nouveau fichier sans bascule automatique.
Un clic manuel désactive ce suivi, comme dans la référence. Le passage à un autre
run reste manuel dans la première approche.

## Cas de vie du fichier

| Événement | Comportement attendu |
| --- | --- |
| Append | Ajouter les octets décodés sans doublon, dans l’ordre |
| Ligne incomplète | Afficher une fin provisoire, complétée par les octets suivants |
| Troncature détectée | Signaler « Fichier réinitialisé », ouvrir une nouvelle génération |
| Remplacement détecté | Nouvelle génération, sans mélanger ancien et nouveau contenu |
| Suppression | Conserver la vue déjà chargée avec état « Fichier supprimé » |
| Renommage | Signaler le changement ; ne pas présenter silencieusement un autre fichier |
| Rotation vers un nouveau nom | Actualiser l’arbre ; garder le fichier sauf suivi du successeur activé |
| Déconnexion du compagnon | Conserver la vue, indiquer le problème et tenter une reprise bornée |
| Changement de dossier | Annuler lectures, flux et réponses périmées de l’ancienne racine |

Le mode de référence est append-only. Une réécriture arbitraire non détectable par
les contrôles retenus ne peut pas bénéficier d’une garantie absolue de reprise.
Les contrôles et leurs limites sont précisés dans l’architecture livrée.

## Performance et affichage

Le lecteur vise moins d’une seconde entre disponibilité des nouveaux octets et
affichage en charge nominale, à mesurer sur un environnement identifié. Le budget
de regroupement/flush de la lib Rust s’ajoute en amont : ce n’est pas une promesse
de moins d’une seconde entre action métier et écran en toute circonstance.

Les lignes LATENCY ne sont ni recalculées ni supprimées ; elles mesurent le
traitement du logger. Les notifications de transport sont dans le chrome de
l’interface, pas insérées au milieu des lignes du fichier.

La vue est virtualisée et la mémoire bornée. L’historique évincé reste sur disque
et peut être relu si le fichier n’a pas changé ou disparu. Une indisponibilité
doit être annoncée, pas remplacée par un contenu inventé.

## Ergonomie de base

- Deux colonnes, environ un tiers/deux tiers, séparateur ajustable.
- Sur petite largeur, arbre de hauteur bornée au-dessus du lecteur.
- Navigation clavier pour champ, arbre, ouverture de fichier et retour en bas.
- Texte de statut en plus des couleurs ; focus visible et labels accessibles.
- États vide, chargement, dossier inaccessible, fichier disparu et reprise explicites.
- Aucune modale de détail obligatoire ni tableau de bord de métriques avant le lecteur.

## Évolutions hors première livraison de base

Recherche sur tout le fichier côté compagnon, filtres structurés, extraction du
contexte, multi-onglets dans l’interface, fusion de destinations et thèmes avancés.
Une recherche dans la tranche déjà chargée doit annoncer sa portée. Un groupe
`x23` ne pourra pas être déplié en occurrences absentes des données sources.
