# Instances, contexte et fichiers

## Instance logique et worker

Une instance est une identité de journalisation utilisée par l’application, avec
un contexte et des sorties propres. Le worker est le thread d’exécution qui
traite les événements et les fichiers.

```text
Instance backend   ─┐
Instance agents    ─┼─→ moteur partagé → worker commun → fichiers séparés
Instance inference ─┘
```

Cloner le handle d’une instance conserve son identité et ses fichiers. Créer une
instance distincte crée une séparation logique de contexte et de destinations.
L’ordre global ne concerne que les instances rattachées au même moteur.

Le traitement et les ressources étant partagés, une instance très active peut
affecter les autres. Les quotas ou garanties d’équité par instance ne sont pas
actés ; leurs besoins seront évalués par des scénarios de charge.

## Contexte

| Catégorie | Informations possibles | Provenance |
| --- | --- | --- |
| Exécution physique | Identifiant et nom du thread courant | Capture automatique optionnelle |
| Exécution logique | Worker, tâche, service | Application |
| Origine | Crate, module, fichier, ligne | Macros ou métadonnées disponibles |
| Fonction ou méthode | Fonction émettrice, méthode appelée | Champ explicite en première approche |
| Fonctionnalité | Authentification, recherche, inférence | Contexte ou routage |
| Opération | Workflow, agent, appel, parent | Application ou contexte des spans |
| Dépendance | Bibliothèque, API, endpoint, modèle | Application |
| Extension | Champs propres au backend | Application |

La crate correspond au nom compilé, qui peut différer du nom du package Cargo.
Une fonctionnalité métier n’est pas une feature Cargo : il n’existe pas de
« feature courante » unique à déduire automatiquement à l’exécution.

L’origine de l’émission reste distincte de l’API ou bibliothèque appelée. Les
valeurs sont capturées comme un instantané ; le worker ne relit pas un état métier
mutable plus tard.

## Héritage et propagation

Proposition de priorité, de la moins spécifique à la plus spécifique :

```text
Contexte application → contexte instance → contexte opération → champs événement
```

Un handle contextualisé permet d’enrichir une opération sans créer de nouvelle
instance. Pour éviter les interférences entre requêtes concurrentes, les contextes
d’opération sont immuables ou copiés à l’enrichissement, pas modifiés globalement
sur l’instance partagée.

La priorité est application < instance < opération < événement ; les clés internes
réservées sont refusées, comme décrit dans le contrat livré. L’application ne doit pas pouvoir remplacer involontairement la
séquence interne ou l’identité physique de l’instance par un champ homonyme.

Le contexte logique est transmis explicitement aux tâches async ou récupéré via
leur instrumentation `tracing`. Un stockage limité au thread ne suffit pas.
Le thread physique, lorsqu’il est activé, est relevé à chaque émission.

## Routage

Chaque instance peut router ses événements vers des destinations selon :

1. Une cible explicite reconnue par sa configuration.
2. Une correspondance de module ou de cible vers une fonctionnalité.
3. La crate d’origine comme repli.

Ordre livré : cible explicite configurée, plus long préfixe par segments `::`,
crate si sa destination existe, sinon première destination déclarée.

Les destinations sont bornées et leurs noms validés. Un champ de contexte libre
ne crée pas automatiquement un fichier. Une chaîne fournie par un utilisateur
ne doit pas pouvoir devenir un chemin arbitraire.

## Rotation horaire et cycle de vie — logger 0.2.0

`Config::directory/rlogger/<jour>/<instance>/`
`<destination>-HH-<run-id>-h<début-UTC>-s<segment>.active.log` est le stockage livré.
Voir le [contrat complet](09-migration-arborescence-rlogger.md).
Rotation horaire obligatoire, création exclusive à la demande ; clôture en `.log`
à l’échéance même au repos, à l’éviction LRU et à l’arrêt normal.
Le compagnon récupère les actifs abandonnés en `.recovered.log` sous bail système.

Jour/heure viennent de la capture. Une capture avant minuit reste classée dans
l’ancien jour ; une capture retardée peut créer un segment supplémentaire.
Un fichier finalisé ne sera jamais rouvert. Les groupes sont coupés aux frontières.
Le run-id isole runtimes et processus ; le segment empêche la réutilisation des noms.

## Heure locale et changements d’horloge

L’ordre interne et les durées sont monotones. L’heure civile peut reculer sans
inverser l’admission. Une heure répétée produit deux intervalles UTC distincts,
même si HH est identique ; une heure sautée ne crée aucun fichier artificiel.
Le décalage applicable à la capture est utilisé, pas celui du moment d’écriture.
Recréer le runtime si le fuseau de la machine est changé manuellement.

## Reconstruction entre fichiers

Les événements simples exposeront leur séquence globale ; les groupes exposeront
leurs bornes de séquences. Ces informations aident à corréler les fichiers mais
ne reconstruisent pas les instants individuels perdus par regroupement.

Le nom du fichier et son ordre de flush ne sont jamais une preuve de causalité
entre opérations ou d’ordre universel entre plusieurs processus.

## Parcours dans le lecteur local

Le chemin fourni au [lecteur](06-interface-web.md) peut viser la racine `rlogger`,
un jour, une instance ou un sous-dossier. L’arbre affiche les niveaux réellement
présents ; il ne suppose pas une date immédiatement sous la racine. Cliquer sur
un fichier suit ce fichier. Le suivi optionnel du fichier suivant à la rotation
doit rester limité à sa destination, son identité et son instance ; il ne doit
pas choisir un autre redémarrage sur le seul critère de dernière modification.
