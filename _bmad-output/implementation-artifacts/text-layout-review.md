# Revue US-012/054/055 — 2026-10-11

Baseline : `6f4b0b34ab39d5a15a73d21f615d2cbf75c7edf6`. Trois nouveaux agents GPT-6 Luna moyen ont reçu le diff complet, fichiers non suivis compris, sans historique de la tâche. Blind hunter : onze constats ; edge-case hunter : `[]` ; verification-gap : `No verification gaps found.` Les trois couches ont été lancées avant collecte.

Les deux agents de développement US-012/055 ont atteint leur limite d'usage en fin d'implémentation. Le coordinateur a repris leurs fichiers et finalisé les corrections. Les agents frais de revue ont ensuite pu travailler. Cette limite ne constitue pas une validation ; les preuves ci-dessous viennent des tests exécutés sur les sources finales.

## Review Triage Log

| ID | Constat | Verdict | Vérification et décision |
|---|---|---|---|
| B1 | Le TSV binaire HarfBuzz est omis du diff ; ses dimensions/empreinte manquent. | false | Le TSV ne change pas depuis la baseline. Son manifeste suivi contient déjà 54 716 cas, vingt SHA de faces et le SHA du corpus. Le garde check-ucd vérifie cette empreinte et les deux tests publics le consomment intégralement. Aucun nouveau corpus opaque n'est introduit. |
| B2 | Les sorties Graphify volumineuses masquent les changements fonctionnels. | low | Le diff complet conserve les artefacts et son index énumère tous les fichiers. Un résumé d'impact explicite est ajouté ci-dessous pour accélérer la navigation ; aucun fichier n'est retiré de la revue. |
| B3 | line_break_class n'a pas de contrôle indépendant sur tous les scalaires. | false | crates/unicode/tests/ucd_reference.rs inclut déjà line_break parmi les propriétés dont l'empreinte est recalculée sur tous les scalaires (déclaration ligne 48 et alimentation ligne 89). L'oracle figé vient des données UCD indépendantes ; EAW complète ce contrôle existant. |
| B4 | line_breaks alloue en proportion du texte sans plafond configurable. | low | L'API &str vers Vec et sa complexité O(n) sont explicites. La documentation précise maintenant l'absence de plafond intégré et le budget fixé par l'appelant ; aucun paramètre ni garde non demandé n'est ajouté. |
| B5 | La direction Y et l'origine des placements ne sont pas documentées. | medium | Un futur appelant Canvas pourrait inverser les placements. La doc fixe X droite, Y haut, offsets relatifs au stylo courant et avances appliquées ensuite, conformément aux valeurs OpenType transmises. |
| B6 | La doc pipeline ne précise pas l'absence de positionnement des marques. | false | Le dernier paragraphe excluait déjà explicitement le positionnement des marques. Le pipeline mentionne aussi désormais « ajustements par paires », clarification sans changement de comportement. |
| B7 | Les plages génériques chevauchantes/non ordonnées ne sont pas clarifiées. | low | Le code accepte toute plage non vide et traite les groupes identiques consécutifs. La doc l'explicite et un test public utilise 8..12, 3..10, 3..10, 1..2, vérifiant maintien de l'ordre et deltas [17,0,17,0]. |
| B8 | EmptyRegistry désigne aussi une famille absente d'un registre non vide. | low | Display disait déjà qu'aucune face ne satisfait la requête. Le commentaire public et la doc précisent cette signification et is_empty() pour distinguer les situations ; aucune nouvelle variante API. |
| B9 | Les tâches restent non cochées malgré les pages d'API implémentées. | false | Le relevé et les trois frontmatters indiquent in-review et attendent expressément la validation finale. Les cases ne doivent être cochées qu'après les contrôles immuables ; leur fermeture est conditionnée aux preuves, pas anticipée. |
| B10 | Les commandes des oracles shaping et Windows ne sont pas explicites dans le relevé. | low | Les sources et le harnais contenaient les commandes ; une section Reproduction les rend directement consultables avec le lien vers chaque résultat. |
| B11 | Les quatre variantes Arial ne disposent pas d'un résumé lisible. | low | La preuve JSON contient quatre lignes famille/poids/style/ID. La doc US-055 ajoute un tableau des quatre sélections System pour faciliter la lecture. |

Chaque constat est classé avant regroupement. B4/B5/B6/B8/B10/B11 donnent des corrections documentaires directes ; B7 ajoute une régression dans le fichier d'oracle possédé par le coordinateur. B1/B3/B9 sont réfutés individuellement. Aucun constat différé.

## Contrôles du coordinateur

| ID | Défaut constaté | Correction et preuve |
|---|---|---|
| C1 | Recherche de fin de graphème par rposition répétée, coût quadratique. | Parcours avant unique des glyphes, puis conversion octets/scalaires par curseur monotone. Interlettrage signé et position exacte contrôlés sur les vingt faces. |
| C2 | Recherche 400..500 choisissait les poids décroissants. | Ordre croissant jusqu'à 500. Test de poids exact 450 contre 500 et treize demandes aux limites des intervalles CSS. |
| C3 | Générique absent renvoyait la première face sans proximité dans sa famille. | Choix de la première famille couvrante puis résolution source/style/poids ; test bold italic Document contre regular Embedded. |
| C4 | Plusieurs noms anglais Windows dépendaient de l'ordre des records. | Tri stable également dans le groupe préféré ; régression avec records inversés. |
| C5 | Contextes LB9 consultaient parfois les marques au lieu des bases (Brahmique, quotes, nombres). | Séquence logique de bases ignorantes des CM/ZWJ pour LB11 et suivantes ; LB8a garde le scalaire original. Tous les 19 338 cas officiels et les mêmes cas enrichis de deux CM concordent. LB10 fixe également GC/EAW/EP aux propriétés de U+0041. |
| C6 | Une phrase de doc disait que le générique n'appliquait aucun espacement. | Phrase contradictoire retirée ; règles de groupes consécutifs et test explicite conservés. |

## Impact architectural

Deux dépendances locales sont ajoutées à atelier-shape : atelier-font et atelier-unicode. Le registre reste un module de font, et les coupures un module de unicode. Le graphe Cargo garde 32 paquets locaux, avec huit relations de dépendance. Le rapport Graphify et son contrôle d'endpoints sont actualisés ; aucun nouveau pont natif ou Worker n'est simulé.

## Reproduction

```powershell
cargo test -p atelier-unicode --test line_break --test line_break_contexts --test east_asian_width --locked --offline
cargo test -p atelier-shape --test reference --locked --offline
cargo test -p atelier-font --test manager --test manager_reference --locked --offline
py -X utf8 _bmad-output/implementation-artifacts/check-system-font-manager.py
py -X utf8 _bmad-output/implementation-artifacts/validate-text-layout.py
```

Le [relevé final](text-layout-validation.json) enregistre chaque commande, son code retour, les comptes et les SHA-256 des sources ; la [preuve Windows](text-layout-system-fonts.json) conserve les quatre fontes réellement utilisées sans leurs octets. Le test de référence shaping parcourt 54 716 cas à taille 721/64 pt, chasse 113,7 %, compare placements/avances/clusters et vérifie l'espacement signé sur toutes les faces.

Diff reçu initialement par les réviseurs : SHA-256 `2f41b596d95821d08414d1cfdbef6c4965db59675b5c6848cc7923cf8a312732`. Les corrections directes post-revue sont consignées ci-dessus ; le diff final est régénéré avant commit.
