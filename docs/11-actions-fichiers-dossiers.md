# Actions sur les fichiers et dossiers — 23 septembre 2026

À la demande de l’utilisateur, les actions manuelles ne dépendent plus du format
RLOGGER ni d’une heure terminée. Cet amendement remplace ces restrictions dans
le contrat horaire et les preuves historiques du 13 septembre.

- Tous les fichiers réguliers non cachés apparaissent dans l’arbre, quelle que soit
  leur extension. Les fichiers et dossiers cachés (nom commençant par un point),
  dont `.rlogger`, ne sont pas affichés, à tous les niveaux. Cette règle d’affichage
  ne les supprime pas du disque : leur contenu et leur allocation restent comptés
  dans les totaux des dossiers ; les archives et suppressions récursives les incluent.
- Un fichier se télécharge tel quel, actif ou non. Le transfert se limite à la taille
  observée au retrait du ticket ; les ajouts suivants n’en font pas partie.
- Deux petites icônes à droite de chaque fichier et dossier dans l’arbre (même
  pour un fichier non sélectionné ou un dossier replié) permettent son
  téléchargement et sa suppression, avec infobulles et libellés accessibles.
  Le lecteur n’affiche pas de boutons d’action en double. Le téléchargement d’un
  dossier produit un TAR non compressé conservant arborescence, dossiers vides,
  noms longs et octets des fichiers.
- La suppression d’un dossier inclut récursivement tous ses descendants, même cachés.
  La confirmation affiche le nom, le chemin relatif et la portée récursive. Annuler
  ou Échap ne modifie rien et rend le focus à l’icône de suppression ; le retrait
  de l’arbre intervient après succès serveur.
- Un fichier sélectionné situé dans un dossier supprimé est désélectionné ; les autres
  branches restent utilisables. Les erreurs sont affichées, sans retrait optimiste.

Les actions portent sur une entrée dans la racine ouverte. Pour agir sur le dossier
racine lui-même, ouvrir son parent puis choisir ce dossier. La racine d’autorisation
n’est pas supprimée par une route d’entrée.

## Contrôles et ressources

Host/Origin, session, identifiants opaques et `If-Match` obligatoire sont conservés.
Les chemins restent résolus par des descripteurs sous la racine autorisée ; aucun
symlink descendant ni fichier spécial n’est suivi. Un dossier contenant une telle
entrée est refusé avant suppression, sans supprimer une partie de son contenu.
Les permissions du système s’appliquent. Une précondition périmée impose d’actualiser.

Cette interface est une démo locale sur une machine à compte fiable. Les jetons
opaques servent au protocole et ne prouvent pas l’identité d’un utilisateur.
Une application intégratrice doit contrôler l’accès à son propre point d’entrée
et empêcher les clients non fiables de joindre directement le compagnon.
Une racine RLOGGER non privée reste consultable et autorise les actions manuelles
selon les permissions du système ; seules récupération et maintenance automatiques
sont désactivées, avec une raison affichée. Un `refresh=1` ne lance pas plus d’un
nouveau scan par racine toutes les 60 secondes, sessions confondues ; les
invalidations internes après mutation sont regroupées.

Les transferts gardent les tickets uniques de 30 s, un par session, un transfert par
session et quatre globaux, sans fichier entier en mémoire ni Blob frontend. Les
fichiers sont verrouillés en partage ; une opération sur un dossier prend un verrou
exclusif sur ce dossier et des verrous partagés sur ses ancêtres. Les compagnons
coopérants ne peuvent donc pas supprimer un parent pendant un transfert descendant,
ni modifier un sous-arbre pendant son archivage. Aucun verrou RLOGGER de racine
n’est gardé pendant le transfert réseau.

L’inventaire préalable d’un dossier est borné à 64 niveaux, 200 000 entrées et un
budget de 32 Mio pour les chemins et une estimation des métadonnées. Au dépassement,
sélectionner un sous-dossier. Le contenu est diffusé par tampons bornés à 64 Kio.
L’archive annonce sa longueur ; une erreur de lecture coupe le transfert plutôt que
de livrer silencieusement une archive complète contenant des octets manquants.

L’inventaire n’est pas un snapshot transactionnel du disque. Les fichiers sont
revalidés avant lecture/suppression. Si une mutation, permission ou interruption
survient après les premières suppressions, l’erreur indique explicitement une
suppression partielle et demande d’actualiser. Aucun rollback de fichiers supprimés.
La suppression explicite d’actifs ou de métadonnées peut interrompre leur producteur ;
elle est désormais permise par la demande utilisateur. Les verrous restent coopératifs.

La récupération automatique et le nettoyage des vieux dossiers vides conservent
leurs conditions RLOGGER. Aucun fichier non vide n’est supprimé automatiquement.
Le writer, son API, ses baux et le format RLOG/1 ne sont pas modifiés.
