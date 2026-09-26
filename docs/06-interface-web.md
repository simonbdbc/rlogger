# Interface web locale

## Périmètre acté

Le lecteur fonctionne uniquement sur la machine de l’utilisateur, avec Expo et
gluestack. L’utilisateur saisit le chemin absolu du dossier de logs, par exemple
celui affiché par `pwd` dans ce dossier. Ce chemin est une donnée, jamais une
commande à exécuter.

L’arborescence des dossiers et fichiers apparaît à gauche. Un clic sur un fichier
affiche son contenu à droite et les ajouts sont affichés en temps réel. Aucun
compte, mot de passe, URL de serveur distant, cloud ou application mobile n’est
nécessaire dans cette version.

Le lecteur complet reste autonome. Ses deux écrans peuvent aussi être copiés
dans une application Expo web locale et liés à son menu existant ; voir le
[guide d’intégration](../front-react-logger/docs/INTEGRATION-EXPO.md).
Les interactions du lecteur autonome sont couvertes par les tests navigateur.

## Parcours principal

1. Lancer le lecteur local.
2. Choisir « Journaux RLOGGER » ou « Journaux externes », saisir un chemin absolu,
   puis cliquer sur « Ouvrir ».
3. Déplier librement les niveaux de l’arborescence.
4. Cliquer sur un fichier régulier visible, quelle que soit son extension, pour en
   lire le contenu.
5. Voir les ajouts sans recharger la page ; consulter l’historique sans être ramené
   de force en bas du lecteur.
6. Sélectionner un autre fichier ou changer de dossier ; l’ancien suivi est arrêté.

Tous les fichiers réguliers non cachés sont proposés. Le chargement d’abord de la
fin des gros fichiers et le suivi conditionnel du scroll sont implémentés.
Le contenu plus ancien doit rester accessible, sans chargement complet en mémoire.

```text
Sources : [Journaux RLOGGER] [Journaux externes]
Dossier local : [/chemin/vers/logs                           ] [Ouvrir]
┌────────────────────────┬────────────────────────────────────────────┐
│ Dossiers et fichiers   │ rlogger/2026-09-08/backend/http-09-<id>.log│
│ [Actualiser]           │ ● En direct   [Retour en bas]             │
│ ▾ rlogger              ├────────────────────────────────────────────┤
│   ▾ 2026-09-08         │ 09:00:00 INFO [LATENCY — 2ms] Début…       │
│     ▾ backend           │ 09:00:01 INFO [LATENCY — 3ms] Fin…         │
│       http-08-<id>.log │                                            │
│       http-09-<id>.log←│ Texte monospace, sélectionnable           │
└────────────────────────┴────────────────────────────────────────────┘
```

La disposition desktop donne environ un tiers de la largeur à l’arbre et deux
tiers au lecteur, comme la référence. Le redimensionnement et l’adaptation aux
petites fenêtres sont détaillés dans la [spécification frontend](../front-react-logger/docs/SPECIFICATION.md).

## Accès au disque et fonctionnement local

Un navigateur ne peut pas ouvrir un chemin arbitraire simplement parce qu’il est
saisi dans un champ. Les API web de fichiers demandent une sélection/permission
explicite via des handles. Pour conserver le parcours demandé avec un chemin
saisi, le compagnon Rust lit le disque et sert le lecteur.
[Référence : File System API](https://developer.mozilla.org/en-US/docs/Web/API/File_System_API)

```text
Application Rust → lib-rust-logger → fichiers locaux
                                         ↓ lecture et gestion encadrée
                              Compagnon local du lecteur
                                         ↓ HTTP / WebSocket loopback
                               Expo web + gluestack
```

Le compagnon appartient au chantier frontend, pas au cœur Rust. Rust, Axum et
Tokio sont les choix livrés, avec HTTP/WS v1. Il écoute uniquement sur loopback.
Il permet le téléchargement et la suppression explicite des fichiers réguliers
et dossiers de toute racine ouverte, selon les permissions du système. La
récupération et le nettoyage automatiques exigent le parcours RLOGGER et une
racine RLOGGER 2 privée ; une racine non privée reste consultable et affiche la
raison du refus dans ce parcours. Le parcours externe n’active pas la maintenance
et n’affiche pas d’avertissement à ce sujet.
Il ne réécrit pas le contenu des logs. L’application sert localement ses assets
du build ; aucune ressource distante n’est requise pour lire les logs.

La démo suppose un compte local fiable. Le jeton de session est un état opaque
du protocole, sans preuve d’identité. Une application intégratrice doit contrôler
son propre point d’entrée et empêcher l’accès direct au compagnon depuis des
clients non fiables.

Le dossier fourni borne la lecture. L’exploration est générique : elle ne suppose
pas que le premier niveau soit une date et accepte aussi bien la cible
`rlogger/jour/instance` que les anciens niveaux `run/instance/jour`. Cette
cohabitation est détaillée dans la [migration `rlogger`](09-migration-arborescence-rlogger.md).
Un changement de dossier invalide les requêtes et flux précédents.

## Interactions attendues

- Arbre/liste à gauche et lecteur sombre monospace à droite.
- Sélection visible et états « Sélectionnez un fichier » / « Chargement ».
- Flux du fichier sélectionné, ancien flux fermé au changement.
- Défilement automatique uniquement lorsque l’utilisateur reste en bas.
- Tailles contenu/allocation par fichier, dossier et racine, calcul/partiel/indisponible distincts.
- Icônes de téléchargement et de suppression confirmée sur chaque fichier régulier
  et dossier visible ; archive TAR non compressée et suppression récursive pour les
  dossiers. Les entrées cachées ne sont pas affichées, mais restent incluses dans
  l’archive et la suppression de leur parent.
- Rafraîchissement explicite des tailles et de la maintenance limité à un nouveau
  scan par racine toutes les 60 secondes, sessions confondues.
- Renommage de finalisation suivi par identité, sans concaténer un segment suivant.

Le dossier choisi par l’utilisateur devient la racine unique. Le
[contrat actuel des actions](11-actions-fichiers-dossiers.md) précise préconditions,
confinement et limites. Aucune structure de noms métier ni accumulation sans
borne de texte ne fait partie du contrat du lecteur.

## Ordre, latence et temps réel

Le mode brut conserve l’ordre des lignes et le texte décodé du fichier ; les
offsets demeurent des positions en octets. Aucun tri par heure,
aucune nouvelle déduplication, aucun marqueur technique injecté dans les logs.
Dans **Journaux RLOGGER**, une présentation réversible affiche en en-tête
`LATENCY`, l’action et la séquence des lignes RLOG/1 complètes dont le message
est du JSON valide, puis indente ce JSON. Les autres lignes restent brutes ;
**Journaux externes** est toujours brut. Le bouton de bascule restitue les
lignes originales sans toucher au fichier. `x23` et les champs multiparties
ne sont pas fusionnés ; FRONT-029 garde le périmètre d’un parsing structuré
complet, sans inventer les détails perdus par regroupement.

Le délai de lecture du frontend s’ajoute à celui d’écriture du logger. Il doit être
mesuré séparément ; le lecteur ne remplace pas LATENCY par son propre retard et
ne voit pas les données encore dans les buffers applicatifs.

Objectif proposé : ajouts visibles en moins d’une seconde après leur disponibilité
dans le fichier, en charge nominale sur la machine de référence. Ce n’est pas une
garantie en cas de suspension du navigateur, de disque bloqué ou de très forte charge.

## Documentation du lecteur

- [Documentation frontend](../front-react-logger/docs/README.md)
- [Architecture et protocole local](../front-react-logger/docs/ARCHITECTURE-LOCALE.md)
- [Intégration des écrans dans une application Expo web](../front-react-logger/docs/INTEGRATION-EXPO.md)

La lecture brute se teste avec les fichiers synthétiques créés par
l’[exemple Rust du compagnon](../local-logs-server/examples/fixtures.rs).
