---
title: 'DOCX — Aperçu complet des guides longs'
type: bugfix
created: '2026-10-13'
status: done
route: oneshot
baseline_commit: b3525756b0cf7bbf4732c9ea4ba8ef20d11e485d
---

## Intent

Le DOCX signalé comporte 109 671 octets de texte, 36 paragraphes et 21 964 runs, sous les plafonds d'import. L'aperçu échoue d'abord au plafond de 65 536 glyphes, puis au budget ATDL lorsque ce premier plafond est porté à 131 072. Permettre l'aperçu complet, sans troncature, en conservant les courbes des polices plutôt que leur approximation par de nombreuses lignes. Synchroniser le plafond de glyphes avec le budget de texte existant. Conserver les plafonds de 128 KiB de texte, 26 MiB de listes graphiques, 32 MiB de réponse et le budget WASM existant.

## Implementation Notes

API d'itération immutable sur les éléments de Path dans geom ; transformation exacte vers les éléments ATDL déjà pris en charge par le renderer. Facturation des octets selon chaque type d'élément avant allocation. La conservation des courbes seule dépasse encore le budget sur le cas réel : pour les documents de plus de 65 536 octets de texte, un atlas partage chaque contour par clé police/glyphe/taille. Les pages utilisent les DrawGlyphRun ATDL existants ; aucun moteur de texte navigateur.

ATRP v1 reste le format des aperçus courts. ATRP v2 garde l'en-tête de 20 octets et le texte, puis ajoute une longueur u32 et un atlas ATDL avant les pages habituelles. L'atlas A4 est un conteneur de FillPath noirs NonZero, sans rendu de page ; l'index de commande devient glyph_id, font_id vaut 0, et les positions des pages sont absolues. Limites : 4 096 entrées d'atlas, un million d'éléments de chemin cumulés d'atlas, 26 MiB pour atlas et pages ensemble, 32 MiB pour la réponse. Le décodeur valide forme, références et bornes avant installation ; le cache du renderer est invalidé au changement de document.

Une tâche Luna moyen implémente les courbes et l'atlas Rust, une autre le décodeur, le renderer et les régressions ; le parent intègre, vérifie le document réel localement, reconstruit le moteur embarqué et le binaire. Revue indépendante avant livraison. Les documents privés restent hors dépôt, logs suivis et prompts ; seules des métriques anonymes sont publiées.

## Acceptance

- Aperçu réel ouvert par Worker/WASM/UI, texte complet et état TXT conservé ; fichier source inchangé.
- Régression synthétique dépassant 65 536 glyphes, pages ATDL décodables ; dépassements bornés toujours rejetés.
- Courbes et contours fermés préservés, tests géométrie/rendu et compatibilité DOCX existante réussis.
- fmt, Clippy hôte/WASM, tests ciblés, package exact, budget WASM, navigateur et ressources natives vérifiés.
- Documentation et cartographie actualisées après validation ; livraison sur branche isolée pour préserver les développements parallèles.

## Review Triage Log

Revue indépendante Luna moyen : deux défauts intermédiaires identifiés et corrigés avant validation. La validation initiale imposait un seul contour par glyphe, ce qui rejetait les contre-formes et lettres accentuées ; les FillPath multi-contours sont désormais acceptés et testés. Un atlas vide pour un long document sans contours était rejeté ; les tables vides sont désormais acceptées, avec références toujours vérifiées. La borne frontend a été alignée à 4 096 entrées. Passe finale : aucun autre défaut actionnable.

Validation ciblée finale : 13 contrôles et 376 tests Rust des six paquets concernés réussis ; 1158 entrées sources inchangées. Clippy hôte/WASM, fmt, tests JS, parcours navigateur, ressources natives, budget WASM inchangé et correspondance du moteur embarqué passent. Le document privé exact ouvre 35 pages et 110 286 octets de texte dans la vraie interface avec Worker/WASM ; la dernière page contient des pixels de texte, le retour TXT conserve son état, et le hash du fichier source reste identique. Aucun test workspace complet ni certification de fidélité Word n’est revendiqué.
