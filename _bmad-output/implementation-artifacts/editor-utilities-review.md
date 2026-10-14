# Revue du lot US-227/230/361 — 2026-10-14

Trois revues indépendantes GPT-6 Luna moyen : critique à contexte neuf, cas limites et lacunes de vérification. Chaque constat est trié ci-dessous ; les doublons restent visibles. Les remarques retenues sont corrigées et testées. Le parent a terminé les corrections des agents après leur limite d’usage. Aucun constat retenu de ce lot n’est différé.

| ID | Verdict | Vérification et décision |
|---|---|---|
| B1 | medium | Une recherche non préfixée composée de lettres A–F était interprétée comme code ; des fragments de noms tels que AC (ACUTE) perdaient leur sens. Codes désormais préfixés U+/0x, test FACE dans le catalogue synthétique. |
| B2 | false | Le focus initial de createDialog restait sur la cellule spéciale car focus() sur le champ caché est sans effet. Appel redondant supprimé ; réouverture spéciale testée, sans attribuer une perte de focus non démontrée. |
| B3 | medium | Le déplacement vertical était fixé à huit, alors que les grilles spéciales et étroites ont six/quatre colonnes. Le nombre de colonnes rendu pilote désormais la navigation. |
| B4 | medium | Le remplacement des cellules après clic supprimait la cible de focus. Focus restauré par scalaire dans la grille, y compris après redimensionnement. |
| B5 | medium | Un indice des récents était appliqué aux résultats. Les positions actives sont désormais indépendantes par grille et testées. |
| B6 | medium | Les cellules de grille n’étaient pas possédées par des lignes. Lignes ARIA, indices et nombres de lignes/colonnes ajoutés, recalculés selon le rendu. |
| B7 | low | L’option globale réutilisait le libellé du champ Bloc. Clé dédiée Tous les blocs ajoutée. |
| B8 | low | U+2011, spécial connu mais absent de la cmap, ne pouvait être trouvé par code. Les codes explicites consultent aussi les onze spéciaux connus, jamais les scalaires arbitraires non couverts. |
| B9 | false | Le TXT courant et l’aperçu de glyphes utilisent réellement Carlito ; aucune autre police n’est promise. Le catalogue est comparé à la cmap du moteur et le seul repli U+2011 est testé séparément sans modifier le texte. |
| B10 | medium | Le parent signalait déjà le refus hors du dialogue mais ce message était masqué par le modal. Le dialogue annonce maintenant le refus dans sa région de statut, en gardant le choix et les récents intacts. |
| E1 | medium | Même cause que B1 ; recherche de nom et recherche de code explicite sont distinguées. |
| E2 | medium | toLocaleLowerCase pouvait modifier les I des noms anglais Unicode selon la locale. Recherche désormais avec toLowerCase stable et régression dédiée. |
| E3 | false | Le diff initial utilisait une échéance de 500 ms, sans le reset de 50 ms cité ; chaque ouverture remplaçait/disposait le popup précédent. Aucun double popup démontré. Le problème réel de clic droit après fermeture est traité séparément en P1. |
| V1 | medium | Les tests du convertisseur de dates dans build.rs avaient été exécutés explicitement en local mais pas par cargo test en CI. Fonction pure déplacée dans src/build_date.rs, partagée avec build.rs et testée par le paquet desktop normal : jours UTC, années bissextiles, borne 9999. |
| P1 | medium | Le clic droit réel immédiatement après fermeture d’un menu clavier était ignoré pendant la fenêtre temporelle. Coalescence maintenant liée à la visibilité du popup, sans minuterie ; tests ouverture clavier/native et fermeture/clic droit. |
| P2 | low | Le test de coupe différée se fondait sur 50 ms d’attente. Il attend maintenant une nouvelle mutation du statut d’erreur avant de vérifier texte et révision. |
