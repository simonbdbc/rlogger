# Local Logs — lecteur 0.4.0

Lecteur local Expo web, gluestack et compagnon Rust. Aucun compte ni backend
métier : un chemin absolu, un arbre, le fichier sélectionné et ses ajouts en direct.

## Démarrage

Prérequis validés : macOS 26.6 Apple Silicon, Rust 1.95+ ; Node 24 et npm pour le frontend. Depuis ce dossier :

```sh
npm ci
npm run build
npm start
```

Pour modifier l’interface, arrêter d’abord tout lecteur déjà lancé sur le port
4317, puis exécuter dans ce dossier :

```sh
npm run dev
```

Ouvrir `http://127.0.0.1:4317`, choisir **Journaux RLOGGER** ou
**Journaux externes** dans le menu latéral, saisir un chemin absolu puis
**Ouvrir** ou Entrée. Le premier parcours attend la racine privée `…/rlogger`
et permet sa maintenance automatique lorsqu’elle est reconnue. Le second sert
aux dossiers d’applications tierces : consultation,
téléchargement et suppression explicite restent disponibles, sans maintenance
automatique ni avertissement lié au format RLOGGER. Le compagnon applique aussi
ce choix ; ouvrir une racine RLOGGER dans le parcours externe ne lance pas sa
maintenance.
La commande `pwd` exécutée dans le dossier de logs donne ce chemin. Le build et
ses polices système/assets sont locaux ; après installation et build, la lecture
fonctionne sans internet. Aucun tunnel, CDN ou déploiement.

`npm run dev` lance le compagnon Rust en mode développement. Il surveille les
sources Expo, relance leur export après chaque modification et actualise
automatiquement la page ouverte sur `http://127.0.0.1:4317`. Il s’agit d’un
rechargement complet de la page : les chemins mémorisés localement sont rouverts,
mais l’état provisoire de l’interface est réinitialisé. Aucun serveur Node
n’est lancé ; Expo est appelé uniquement pour exporter le frontend. Les exports
de développement sont placés dans `.local-logs-dev/`, ignoré par Git.
Si un export échoue, le dernier build réussi reste servi ; corriger la source
déclenche un nouvel essai. Les changements du code Rust demandent de relancer
`npm run dev`.

`npm run preview` conserve l’ancien parcours : un export puis le compagnon,
sans surveillance. Avec `npm start`, exécuter d’abord `npm run build`. Pour ces
deux commandes statiques, une modification du frontend demande un nouvel export,
un redémarrage du compagnon et une actualisation du navigateur. Après build,
le binaire Rust peut être exécuté directement sans Node :
`../local-logs-server/target/release/local-logs-server --dist ./dist`. Un port occupé est signalé ; choisir par exemple
`LOCAL_LOGS_PORT=4320 npm start`. Arrêt par Ctrl+C : connexions et ressources fermées.

L’exemple Rust du compagnon crée des fichiers synthétiques ; chaque commande
affiche son dossier privé unique :

```sh
npm run fixtures
npm run fixtures -- --large
npm run fixtures -- --managed
```

Le générateur préserve les fixtures existantes, sauf le fichier synthétique `large.log`
explicitement régénéré avec `--large`. Un dossier fourni doit déjà exister,
appartenir au compte courant, être privé et ne contenir aucun lien symbolique.

Pour la fixture gérée, choisir **Journaux RLOGGER** puis ouvrir le sous-dossier
`rlogger` du chemin affiché.

## Lire

- Déplier les dossiers et cliquer sur un fichier. Les fichiers et dossiers cachés
  (nom commençant par un point, dont `.rlogger`) ne sont jamais affichés dans l’arbre.
  Ils restent inclus dans les tailles, archives et suppressions de leur dossier parent.
  Structure générique,
  sans hypothèse de date/profondeur métier. Arbre et lecteur défilent séparément.
- Le fichier s’ouvre sur les derniers 256 Kio ; **Charger plus ancien** relit des
  pages contiguës. Les positions affichées sont des octets, pas des lignes.
- Le défilement suit les ajouts seulement si vous étiez en bas. En remontant,
  la vue reste stable et signale les nouveaux octets. **Retour en bas** recharge
  la fin actuelle. Le nombre de nouvelles lignes n’est pas inventé.
- Le texte conserve l’ordre disque, Unicode, CRLF, dernière ligne incomplète,
  `xN` et `LATENCY`. Des contrôles de connexion ne sont jamais injectés dans le texte.
- La fenêtre conserve au plus 1 Mio / 20 000 lignes. Le rendu est virtualisé :
  sélection/copie du texte rendu, pas du fichier entier non chargé. Une seule ligne
  géante reste bornée ; scroll horizontal. UTF-8 invalide signalé et remplacé par �,
  caractères coupés entre blocs conservés pour l’append suivant.
- Flèches haut/bas, Home/End et Entrée permettent de parcourir l’arbre ; gauche/droite
  replient/déplient un dossier. Le séparateur se règle au pointeur ou au clavier.
- Chaque parcours mémorise son dernier chemin ; le choix du parcours et la largeur
  sont également enregistrés dans le navigateur, jamais le contenu. Changer de
  parcours ferme la racine affichée puis ouvre automatiquement le chemin mémorisé
  pour la nouvelle entrée. Le parcours sélectionné s’ouvre aussi au démarrage.
  L’ancien chemin générique est replacé dans l’entrée appropriée lors de la
  première ouverture après mise à jour. Aucun chemin vers des journaux tiers
  n’est préconfiguré dans le dépôt : chacun le saisit sur sa machine. Les
  préférences peuvent être réinitialisées.

La disparition d’un fichier est signalée et le chemin reste observé. Troncature ou
remplacement détecté : nouveau snapshot, sans fusion des générations. Après coupure
WebSocket, reprise au dernier intervalle acquitté ; après redémarrage du compagnon,
nouvelle session et revalidation du dossier, puis sélection manuelle du fichier.
Une ouverture de dossier échouée conserve la racine précédente.

## Contrat et limites

Pour toute racine ouverte, l’arbre affiche **Contenu** et **Alloué** pour chaque
fichier/dossier et au total. L’estimation est horodatée ; calcul en cours, résultat
partiel et indisponible sont distincts de zéro. Les liens physiques comptent une
fois dans l’allocation d’un sous-arbre. APFS, clones et snapshots peuvent empêcher
de libérer immédiatement l’allocation affichée.

**Télécharger** utilise une pièce jointe HTTP diffusée par blocs, sans Blob géant.
**Supprimer** demande confirmation du nom/chemin et retire l’entrée après succès.
Tous les fichiers réguliers sont éligibles indépendamment du nom, du format ou
de l’heure, y compris les actifs. Chaque fichier et dossier possède les mêmes deux
petites icônes à droite de sa ligne dans l’arbre, accessibles au clavier et munies
d’infobulles, même sans sélection ou lorsque le dossier est replié. Le lecteur ne
duplique pas ces boutons. Pour un dossier : téléchargement TAR non compressé et
suppression de tout son contenu après confirmation.
Pendant un transfert, les suppressions incompatibles sont indisponibles. Les motifs de refus restent visibles.
La confirmation modale garde le focus et accepte Échap ; une erreur conserve l’entrée.
Un renommage actif → finalisé/récupéré conserve la sélection et sa fenêtre, sans
concaténer un autre segment.

Dans le parcours RLOGGER, récupération des actifs abandonnés et suppression des
vieux dossiers réellement vides à l’ouverture, après suppression et toutes les
60 s tant que la racine privée est ouverte. Les octets présents sont conservés,
y compris une dernière ligne incomplète ;
les buffers perdus avant écriture disque sont irrécupérables. Ni rétention de logs,
compression, ni surveillance persistante. Voir le [contrat](../docs/09-migration-arborescence-rlogger.md).

HTTP/WS v1 sur une seule origine loopback, Host/Origin vérifiés, jeton aléatoire par
onglet. La suppression explicite garde une précondition d’identité et une confirmation,
y compris pour les dossiers. Les IDs sont opaques et liés
à une racine de session. La racine saisie est canonicalisée ; aucun symlink descendant
n’est suivi. Les composants sont vérifiés avant/après ouverture avec identité du handle.
Le lecteur est une démo locale pour une machine à compte fiable. Les jetons de
session ne prouvent pas l’identité d’un utilisateur. Une application intégratrice
doit contrôler son propre point d’entrée et empêcher un client non fiable de
joindre directement le compagnon. Une racine RLOGGER non privée reste consultable,
mais récupération et nettoyage automatiques sont désactivés avec une raison visible.
Les actions manuelles restent accessibles selon les permissions système. Le service ne protège pas contre un administrateur local qui
contrôle le processus.

Le [contrat du 23 septembre](../docs/11-actions-fichiers-dossiers.md) remplace les
anciennes restrictions de téléchargement/suppression liées au stockage horaire.

Quatre sessions maximum, 10 000 nœuds par racine/cache, 64 branches ouvertes,
500 entrées/page. Un dossier >200 000 entrées est explicitement refusé, profondeur
64 max, chemins 4 096 caractères. Positions décimales exactes jusqu’à 2^53−1 octets ;
au-delà, erreur explicite liée au plafond de compatibilité v1. Snapshots 256 Kio, blocs
WebSocket 64 Kio, une tranche non acquittée et plafond réseau 1 Mio/client. Ack absent
10 secondes : fermeture/reprise explicite. Pas de queue de logs illimitée.

Rattrapage du fichier actif toutes les 250 ms et poursuite immédiate après ack d’un
backlog. Les branches ouvertes sont contrôlées chaque seconde. Zéro watcher natif :
surveillance par métadonnées, complétée par l’inventaire récursif borné toutes les
60 secondes des racines ouvertes, y compris après rafraîchissement manuel (mutualisé entre sessions).
Descripteurs ouverts pour une seule
lecture puis fermés. Le changement de racine libère son cache et ses observations.
Sessions déconnectées expirées après deux minutes ; pagehide demande la fermeture.

Contrat nominal append-only. Identité/inode, taille et empreinte des 64 derniers octets
détectent de nombreuses mutations ; une réécriture arbitraire de même taille qui restaure
les mêmes frontières entre observations n’est pas garantie détectée. L’arbre découvre
les fichiers nouveaux mais la sélection ne suit pas automatiquement un successeur.

FRONT-028/029/030 différés : suivi du successeur, parsing structuré, recherche dans le
fichier entier. Aucun panneau de métriques internes du logger n’est fabriqué depuis ses logs.

## Vérifier

```sh
npm run typecheck
npm run lint
npm test
npx playwright install webkit
npm run test:e2e
```

Les tests utilisent des dossiers temporaires ; WebKit est testé en 1280×800 et
390×844. Le navigateur intégré Codex a également été inspecté avec append réel,
historique, sélections A/B/A et petite largeur. Les tests portent sur les
interactions documentées et les captures du lecteur RLOGGER.

Le générateur de fixtures est un exemple Rust du compagnon, compilé par
`npm run test:e2e`. Les limites actuelles figurent dans la
[documentation du projet](../docs/README.md).
