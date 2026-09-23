# Vision et périmètre

## Le besoin

Les logs d’un backend doivent permettre de comprendre les étapes d’un traitement,
leurs chevauchements et leurs résultats. Cette lecture doit rester possible
lorsque plusieurs threads, workers ou tâches async travaillent simultanément.

La bibliothèque doit préserver l’ordre dans lequel elle prend en charge les
événements et déléguer les opérations de fichiers à un worker. Son intégration
doit rester simple et son coût sur le traitement métier doit être faible et mesuré.

## Origine : StudyLogger

Le StudyLogger Java fourni est une référence fonctionnelle, pas un code à porter
à l’identique. Il apporte une lecture pratique des actions, des états et des
répétitions dans un environnement métier spécifique.

Ses méthodes `log()` et `setContext()` sont synchronisées par instance. Cette
sérialisation donne un ordre de prise en charge. Les écritures dans le
`BufferedWriter` ont lieu pendant ces appels ; le thread de fond ne fait que des
flushs périodiques. Il ne trie pas les événements.

Deux précisions évitent de reproduire des comportements involontaires :

- Des instances avec le même `studyId` peuvent ouvrir le même fichier, car son
  nom ne contient pas `instanceId`, alors que leurs verrous sont distincts.
- Le premier message d’une série est écrit immédiatement, puis un résumé peut
  être ajouté avec l’horodatage du premier message. Le compteur du résumé compte
  toutes les occurrences, y compris la première déjà affichée. La future lib
  produira une seule ligne par groupe, sans ce double affichage.

La future bibliothèque conserve le principe de sérialisation à l’entrée, la
lisibilité et les répétitions, en supprimant toute dépendance au métier d’origine.

## Utilisateurs et cas d’usage

| Utilisateur | Besoin |
| --- | --- |
| Développeur backend | Suivre une requête traversant plusieurs modules |
| Développeur de workflows agentiques | Relier workflow, agents, outils et opérations parentes |
| Développeur d’inférence locale | Suivre départ d’appel, premier token, fin, erreur et annulation |
| Exploitant de l’application | Repérer une surcharge, un retard de logs ou une erreur de fichier |
| Auteur de bibliothèque | Émettre des événements génériques sans imposer un runtime async |

Exemple de points d’observation pour deux actions concurrentes :

```text
Début de X
Début de Y
Fin de Y
Fin de X
```

L’application émet le début avant le travail et la fin après. Le logger conserve
cet ordre d’enregistrement. Il ne déduit pas automatiquement les actions absentes
de l’instrumentation.

## Périmètre initial

- Bibliothèque Rust indépendante du domaine métier et du runtime async.
- Prise en charge sérialisée et file bornée, avec un worker d’écriture commun.
- Plusieurs instances logiques avec contexte et fichiers séparés.
- Contexte structuré extensible et identifiants d’opérations.
- Niveaux de logs, filtrage et API native simple.
- Fichiers par instance et destination : crate, fonctionnalité ou cible.
- Rotation horaire obligatoire selon la capture locale, identifiée par intervalle UTC.
- Regroupement des répétitions consécutives et latence sur chaque ligne.
- Surcharge signalée, refus comptabilisés et erreurs observables.
- Fermeture explicite avec drainage et bilan.
- Adaptateur `tracing` optionnel prévu dans le plan de livraison.

Les détails d’API et les seuils de ressources restent des propositions à valider.

## Limites du contrat

La chronologie est commune aux instances rattachées à un même moteur dans un
processus. Elle ne constitue pas une horloge universelle entre applications,
machines ou processus d’inférence.

Un log avant un appel à un modèle local observe le départ côté backend ; il ne
prouve pas l’instant exact de démarrage du calcul dans le processus du modèle.
Ce dernier doit être instrumenté pour observer ses étapes internes.

La sérialisation peut occasionner une courte attente entre producteurs. La
bibliothèque ne promet donc ni zéro synchronisation, ni coût nul, ni temps réel
dur. Elle ne doit pas attendre de place dans une file pleine ou effectuer des
opérations disque dans le chemin d’émission.

## Critères de réussite

1. Aucun événement accepté ne dépasse un autre événement déjà admis dans le flux
   logique du moteur.
2. Les fichiers permettent de suivre les étapes d’un traitement, avec un contexte
   suffisant pour distinguer ses exécutions concurrentes.
3. Le traitement métier reste indépendant des écritures, rotations et flushs.
4. La mémoire, les destinations et les fichiers ouverts ont des limites explicites.
5. La saturation et les erreurs ne sont pas silencieuses.
6. Les latences d’émission et de visibilité sont mesurées séparément sous charge.

## Interface de consultation complémentaire

Une interface web Expo/gluestack fonctionne exclusivement sur la machine locale.
L’utilisateur fournit le chemin absolu du dossier de logs : une arborescence à
gauche permet de sélectionner le fichier affiché en temps réel à droite. Aucun
compte, login, cloud ou connexion à une machine distante n’est prévu.

Le cadrage est séparé dans [Interface web](06-interface-web.md). Un compagnon local
donne au navigateur l’accès en lecture au dossier choisi ; il ne rend pas la
bibliothèque dépendante du navigateur ou d’un serveur HTTP obligatoire.

Le lecteur doit aussi fonctionner sur des fichiers texte de logs génériques. La
lecture brute ne dépend ni d’un format JSON ni de l’achèvement de la bibliothèque.

## Hors périmètre initial de la bibliothèque

Interface graphique intégrée à la crate, collecte distribuée, réseau de transport obligatoire,
ordre global entre processus, journal d’audit durable et garantie sans perte en
cas de panne. JSON, journal brut optionnel, fuseau arbitraire, rétention et
compression des anciens fichiers sont différés.
