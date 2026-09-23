# Fonctionnement et chronologie

## Pipeline retenu

```text
Threads et tâches async
          ↓
Handles des instances de logger
          ↓
Filtrage et préparation bornée des données possédées
          ↓
Admission sérialisée : ordre + capture + publication coordonnés
          ↓
File mémoire bornée
          ↓
Worker commun
          ↓
Routage → regroupement par destination → formatage → fichiers → flush
```

Le logger ne réserve pas un numéro d’ordre pour ensuite laisser un producteur
publier arbitrairement plus tard. Le point d’admission doit coordonner capture,
attribution de séquence et publication. Une erreur ou un refus ne doit pas créer
une capture manquante que le worker attendrait indéfiniment.

Le mécanisme exact de synchronisation sera conçu et mesuré. Les opérations disque,
la conversion de date, les comparaisons de répétitions et le formatage final ne
doivent pas se trouver dans cette section sérialisée. Les données empruntées
nécessaires au worker doivent devenir possédées avant le retour de l’appel ; le
formatage d’un argument arbitraire peut donc encore coûter côté producteur.

## Ce que signifie « chronologique »

L’ordre de référence est celui de la prise en charge par le moteur, commun à ses
instances. Si un appel est terminé avant qu’un autre commence, leur ordre est
préservé. Pour des appels concurrents qui se chevauchent, la sérialisation établit
un ordre commun sans prétendre reconstituer un ordre métier invisible.

Chaque événement accepté porte au minimum :

- un identifiant d’exécution ;
- une séquence globale au moteur ;
- un instant monotone capturé à l’admission ;
- une date civile capturée au même point logique ;
- une instance, une source, un niveau, un message et son contexte.

La séquence fixe l’ordre. L’instant monotone mesure les délais. L’horloge civile
sert à l’affichage et au classement dans les fichiers, même lorsqu’elle recule.

Il n’y a ni fenêtre de réordonnancement, ni recherche d’événements antérieurs
hypothétiques, ni mode qui abandonne l’ordre après une seconde. Ces pistes ont
été remplacées par la sérialisation à l’admission.

## Flux logique, fichiers et répétitions

Le worker consomme le flux admis dans l’ordre global. Chaque destination reçoit
la projection de ce flux dans le même ordre. Les flushs de fichiers différents
peuvent rendre leurs données visibles à des moments différents : leur ordre
physique de visibilité n’est pas une nouvelle garantie globale.

Le regroupement est une compression, effectuée après routage. Pour une destination :

```text
A A B A A  →  A x2 / B / A x2
```

L’égalité comprend instance, destination, niveau, source, message et contexte
significatif. Horodatages, latence et séquence ne font pas partie de cette égalité.
Un autre identifiant d’appel ou de worker produit un événement distinct.

Un groupe conserve : compteur total, première et dernière capture, première et
dernière séquence, ainsi que latence maximale à sa préparation pour écriture.
Une occurrence unique n’affiche pas `x1`.

Un groupe se termine lors d’un changement d’événement, d’une frontière de rotation,
d’une rupture de continuité par refus, d’un flush explicite, de l’arrêt ou de sa
durée maximale. Cette durée est mesurée depuis la première occurrence : une série
continue ne repousse pas indéfiniment son échéance.

Les séquences d’un groupe peuvent être non contiguës globalement si d’autres
destinations ont reçu des événements entre ses occurrences. Les bornes ne
permettent pas de reconstituer tous ces entrelacements. La sortie compacte ne
conserve pas tous les horodatages individuels ; le journal brut est différé.

## Latence sur toutes les lignes

Définition retenue :

```text
latence = instant monotone de préparation de la ligne − instant de capture
```

```text
14:00:00.010+02:00 INFO [LATENCY — 2ms] [seq=41 call=c1] Début appel modèle
14:00:00.020+02:00 INFO [LATENCY — 3ms] [seq=42 call=c2] Début appel modèle
```

L’annotation est présente même sans retard notable. Pour un groupe, c’est le
maximum des latences de ses occurrences au moment de préparer la ligne, et non
le maximum de mesures prises à leur réception par le worker.

Cette latence inclut l’attente dans la file et dans un groupe. Elle exclut l’attente
avant admission, le travail métier antérieur, ainsi que le temps d’écriture et de
flush après préparation. La latence complète de l’appel au logger et le délai de
visibilité devront être mesurés séparément dans les benchmarks.

Un retard important ne change ni l’horodatage ni l’ordre. `LATENCY` ne signifie pas
« hors chronologie ». Le format exact des lignes est encore à stabiliser.

## Visibilité et budget d’attente

L’utilisateur accepte une légère latence mais ne souhaite pas plus d’une seconde
de rétention volontaire. La proposition est de limiter les groupes à 200 ms et
l’intervalle entre flushs à 100 ms, soit un budget nominal d’environ 300 ms hors
traitement et contraintes système. Le worker doit exécuter ses échéances même
sous une arrivée continue de messages.

Ces réglages ne garantissent pas une visibilité réelle sous une seconde en cas de
backlog, de disque bloqué ou de suspension du worker. La capacité bornée limite
les ressources ; elle ne garantit pas à elle seule un délai de service. Les
retards réels devront être exposés et testés, sans sacrifier l’ordre admis.

## Saturation

Le refus comptabilisé et le message de surcharge sont actés. Une file pleine ne
doit pas provoquer une attente de capacité côté producteur.

Le signalement nécessite un canal logique de contrôle ou un état réservé borné
qui reste disponible quand la capacité des événements est épuisée. Il ne doit pas
réémettre récursivement un événement dans la même file pleine.

La conception doit distinguer :

- les compteurs de refus, globaux et par instance ;
- la rupture de continuité située entre événements admis, qui coupe les groupes ;
- le résumé de surcharge, horodaté à sa propre admission dans le flux de contrôle.

Un résumé ne doit pas être ajouté avec un ancien horodatage comme s’il avait été
admis avant des lignes déjà écrites. L’association des refus à une frontière
d’ordre sera spécifiée avant l’implémentation.

Implémentation : un résumé par seconde et par instance pendant une surcharge
continue, puis le solde au retour à la normale ou à l’arrêt. Son compteur décrit
les nouveaux refus depuis le précédent résumé. Les données manquantes ne sont
pas récupérables.

## Erreurs et observabilité

Prévoir des statistiques sans repasser par le logger : événements acceptés,
filtrés, refusés par motif, en attente, messages de surcharge, erreurs d’écriture
et état du worker. Les refus pour événement trop volumineux et logger fermé
doivent être distingués de la saturation.

Les erreurs disque et les événements acceptés mais non écrits doivent apparaître
dans le bilan. La politique retenue est un fail-stop global, sans rejeu d’écriture incertaine ;
aucun succès d’écriture ne doit être annoncé lorsque sa confirmation manque.

## Consultation locale

Le [lecteur web local](06-interface-web.md) affiche les fichiers dans leur ordre
physique et montre les annotations telles qu’écrites. Son suivi intervient après
disponibilité des données dans le fichier ; son délai s’ajoute au délai du logger.
Il n’accède pas automatiquement aux compteurs internes en mémoire : seuls les
messages et champs réellement présents peuvent être affichés comme données.
