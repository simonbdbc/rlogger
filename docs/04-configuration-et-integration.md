# Configuration, intégration et cycle de vie

## Valeurs retenues pour 0.2.0

Ces réglages sont implémentés et évalués sur Apple M1. Les mesures archivées
localement caractérisent cet environnement ; ces limites ne sont pas des promesses de débit.

| Paramètre | Valeur | Portée |
| --- | --- | --- |
| Événements en attente | 8 192 | Moteur entier |
| Budget de données capturées | 32 Mio | Captures réservées, file, groupes retenus |
| Taille d’un événement | 8 Kio maximum | Message et contexte effectif compris |
| Instances | 32 maximum | Par moteur |
| Destinations | 128 maximum | Total de toutes les instances |
| Fichiers ouverts | 32 maximum | Cache du worker |
| Buffer par fichier | 64 Kio | Au plus 2 Mio pour 32 buffers |
| Durée de groupe | 200 ms maximum | Depuis la première occurrence |
| Intervalle de flush | 100 ms maximum | Fichiers contenant des données |
| Résumé de surcharge | Au plus un par seconde et par instance | Solde au rétablissement et à l’arrêt |
| Rotation | Horaire obligatoire | Intervalles UTC, classement par capture locale |

Les limites en événements et en octets s’appliquent simultanément : réserver 8 Kio
par événement autorise au plus 4 096 réservations avec 32 Mio. Un transfert
de la file vers un groupe ne doit pas permettre de dépasser le budget en rendant
prématurément ses crédits disponibles.

Les 32 Mio ne représentent pas la mémoire totale : les métadonnées, les registres,
la pile du worker, les buffers et le coût de l’allocateur s’ajoutent. Les contextes
partagés, champs, destinations et allocations transitoires ont les
[bornes explicites](07-contrats-implementation.md) du moteur. Aucun « maximum mémoire » global ne sera revendiqué sans
comptabilité complète et mesures.

Les événements trop volumineux sont refusés avec un motif distinct,
sans tronquer silencieusement. Les allocations réalisées volontairement par le
code métier avant l’appel ne sont pas contrôlées par le logger.

## API native livrée

L’API compilable et ses exemples sont dans le [guide Rust](../lib-rust-logger/README.md).
`Runtime::new(Config::new(directory))` démarre le moteur ;
`Runtime::instance(InstanceConfig::new(name))` enregistre une instance.
`Logger::with_context(&Fields)` retourne un handle contextualisé immuable.
`Config::directory` désigne le parent de `rlogger`. Rotation horaire obligatoire :
`InstanceConfig.rotation` et `Rotation` sont retirés. `log_root()` et `run_id()`
remplacent `run_path()` ; voir les [ruptures de stockage](09-migration-arborescence-rlogger.md).

Les macros de niveaux prennent un message formaté et retournent `EmitResult` ;
elles évitent l’évaluation des arguments filtrés. La méthode `emit` prend les champs
typés et la destination explicite. Le filtrage par niveau est configuré par instance ;
les préfixes de module servent au routage. Aucun singleton n’est imposé.
Résultats : `Accepted(sequence)`, `Filtered`, `Refused(reason)` ; capacité, taille,
fermeture, panne, champs invalides et destination inconnue sont distingués.

## Adaptateur tracing livré

La feature `tracing` expose `LoggerLayer`, composable avec le subscriber choisi
par l’application. Le cœur sans feature ne dépend pas de tracing. Le champ
`log_instance` sélectionne une instance préenregistrée via `with_instance`.
Événements natifs et tracing partagent capture et admission après collecte des champs.

Spans : 1 024 entrées maximum, profondeur 32, 64 collectes simultanées, valeurs
bornées. Champs enregistrés ultérieurement et parents explicites sont pris en compte.
Les futures utilisent `Instrument`, sans guard conservé à travers un await.
Le formatage Debug/Display et le subscriber de l’application gardent leurs propres
coûts. Voir les [bornes détaillées](07-contrats-implementation.md).

## Flush et arrêt

`flush()` est une barrière : les événements acceptés avant son point de
synchronisation doivent avoir été traités, leurs groupes terminés et leurs
buffers vidés lorsque l’appel réussit. Les producteurs peuvent continuer après
cette barrière. Le contrôle doit rester possible avec une file pleine.

`shutdown()` :

1. Ferme l’admission au même point de coordination que les émissions.
2. Draine les événements déjà acceptés.
3. Termine les groupes et les soldes de surcharge.
4. Vide les buffers, ferme les fichiers puis publie les segments sans écrasement.
5. Attend la fin du worker et retourne un bilan ou une erreur avec bilan partiel.

L’application devrait arrêter et attendre les producteurs avant cette opération.
Une émission concurrente à la fermeture est soit acceptée avant la barrière et
drainée, soit refusée comme logger fermé. Les handles survivants ne paniquent pas.
Les refus après fermeture peuvent être consultés dans les statistiques mais ne
peuvent plus être promis dans un fichier fermé ni dans un bilan déjà retourné.

La première version fournit un arrêt explicite sans délai arbitraire.
Il peut attendre si une opération disque reste bloquée. Une variante avec timeout
nécessiterait un contrat distinct : un délai dépassé ne permet pas de tuer
sûrement un thread bloqué ni d’annoncer que toutes les données sont écrites.

Les appels d’attente sont bloquants et doivent être déportés hors des workers du
runtime async de l’application. Le cœur reste indépendant de Tokio.

`Drop` du runtime effectue le drainage bloquant, sans remonter les erreurs ; celles-ci
doivent être traitées par shutdown explicite. Drop d’un clone ne ferme pas le moteur.

## Visibilité et durabilité

Le flush vide les buffers applicatifs vers le système. Il n’est pas une garantie
de persistance en cas de panne d’alimentation. Une synchronisation du stockage
est une option distincte, proposée pour un travail ultérieur sauf besoin validé.

## Références techniques

- [BufWriter : flush explicite et limites de Drop](https://doc.rust-lang.org/std/io/struct.BufWriter.html)
- [tracing-subscriber : interface Layer](https://docs.rs/tracing-subscriber/latest/tracing_subscriber/layer/trait.Layer.html)
- [Chrono : heure locale et décalage UTC](https://docs.rs/chrono/latest/chrono/struct.Local.html)

Ces références étayent les points d’intégration. Elles ne figent pas les versions
de dépendances et complètent les versions verrouillées dans Cargo.lock.

## Lecteur local indépendant

Le lecteur Expo/gluestack et son compagnon local font partie de
`front-react-logger`. Ils ouvrent les fichiers via le chemin saisi par l’utilisateur.
Installer ou initialiser la bibliothèque ne doit pas lancer ce service ; arrêter
le lecteur ne ferme pas le logger. Les commandes de vérification figurent dans
le [guide de lancement](../README.md) et la [CI](../.github/workflows/ci.yml).
