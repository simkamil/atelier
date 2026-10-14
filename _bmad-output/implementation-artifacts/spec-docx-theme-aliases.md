---
title: 'DOCX — Couleurs de thème et styles par défaut Word'
type: bugfix
created: '2026-10-13'
status: done
route: oneshot
baseline_commit: 47d07219246f9a84ca117573b5f167669b3ee027
review_loop_iteration: 0
context: []
---

<frozen-after-approval reason="Correction du refus DOCX signalé par l’utilisateur ; choix technique réversible et limité">

## Intent

Le document utilisateur de 2 149 773 octets échoue avec `unknown theme color "text1"`, malgré seulement 2 230 octets de texte et 63 paragraphes. Accepter les noms WordprocessingML standard text1/background1/text2/background2/hyperlink/followedHyperlink dans le résolveur existant, en conservant les noms déjà acceptés et le rejet des valeurs inconnues. Le mapping du document signalé est le mapping standard t1→dark1, bg1→light1, t2→dark2, bg2→light2. Ne pas élargir les budgets ni prétendre implémenter les remappings personnalisés, les teintes ou l’édition riche. Vérifier que le document exact s’ouvre dans le moteur WASM et l’UI, sans le modifier ni le versionner.

</frozen-after-approval>

## Implementation Notes

Cause reproduite par appel natif direct de preview_docx. Le helper `crates/fmt-ooxml/src/docx.rs::theme_color_slot` sert les propriétés directes, styles et tables ; il reconnaît tx1/bg1 mais pas text1/background1. Correctif dans ce helper partagé, régression synthétique vérifiant les couleurs effectives (slots distincts), styles et valeurs inconnues ; aperçu synthétique si nécessaire. Aucun contenu personnel dans le dépôt, les logs suivis ou les prompts d’agents. Référence primaire : [ThemeColorValues Microsoft](https://learn.microsoft.com/en-us/dotnet/api/documentformat.openxml.wordprocessing.themecolorvalues?view=openxml-3.0.1).

Vérification ciblée : fmt-ooxml/core-wasm tests, Clippy hôte/WASM, fmt, package WASM exact et budget inchangé ; tests navigateur et sonde native. Lecture du fichier réel en local uniquement, preuve par hash et métriques sans texte. Mettre à jour docs/rich-docx-preview.md, reçu technique et graphe ; commit/push autorisés par les instructions persistantes de la session.

Après la correction des alias, le même fichier expose une seconde cause dans parse_styles : le type Word `numbering`, non modélisé et classé par repli comme Paragraph, est pris à tort pour un deuxième défaut de paragraphe. La détection du défaut utilise maintenant le type XML déclaré. La régression vérifie les défauts numbering/character/table indépendants et conserve le rejet de deux véritables défauts de paragraphe. Aucun nouveau type de style ni moteur de numérotation ajouté. L’appel natif exact ouvre ensuite 3 pages / 2 296 octets de texte ; source inchangée.

## Review Triage Log

Une revue indépendante Luna moyen a pu s’exécuter après la reprise. L’agent d’implémentation avait atteint son quota après ses modifications ; le parent a repris les vérifications et le deuxième correctif.

| Remarque | Verdict | Décision |
|---|---|---|
| text1 manquant dans style nommé | low | Test ajouté : six styles nommés couvrent les six alias, en plus des runs et docDefaults. |
| Couverture highlight | false | w:highlight WordprocessingML prend une couleur nommée ST_HighlightColor ; ce correctif concerne themeColor de w:color. Le chemin non standard proposé ne démontre pas une régression. |
| Héritage basedOn absent | low | Test ajouté : un style dérivé de Style1 hérite de la couleur text1 résolue. |
| Slot de thème absent | low | Comportement préexistant du résolveur : style → MissingThemeColor, couleur directe → absence. Les alias utilisent les mêmes slots et n’ajoutent pas de politique ; aucun garde ou changement de repli ajouté hors de ce correctif. |

Validation finale : 13 contrôles ciblés, 109 tests Rust OOXML/core-WASM et 219 assertions éditeur réussis ; 1152 sources immuables. Clippy hôte/WASM, fmt, budget et package exact, sonde WebView2 passent. WASM 4 214 052 octets, sous le plafond inchangé 4 271 903. Le document privé exact ouvre 3 pages ; son empreinte est identique avant/après et aucun contenu personnel n’est distribué. Les deux remarques de couverture retenues sont corrigées ; aucune régression de ce correctif n’est différée.

Intégration parallèle : main a avancé vers `4162b1bf8bd9574a6bebd096cee59c80c1da0dad` avec US-316 pendant le correctif. Fusion conservant les deux apports, conflits uniquement dans le WASM/manifest résolus par reconstruction depuis les sources réunies. Nouvelle validation ciblée après intégration dans docx-theme-merged-validation.json ; le premier reçu reste la preuve de la version isolée. Le test local du document exact a de nouveau réussi avec ce moteur réuni. Les fusions US-291/306 ont ensuite continué sur main : le correctif est livré sur codex/docx-theme-aliases avec un binaire distinct ; aucun remplacement des commits parallèles sur main.

Validation après intégration US-316 : 13 contrôles, 264 tests Rust des trois paquets concernés passent, avec 1158 entrées immuables. WASM reconstruit : 4 214 304 octets, SHA-256 `33876ba217103f50bafdfcefb91d908d9f40f151720a9c8dcb16ee1eceef0090` ; plafond inchangé. Les parcours éditeur, ressources natives sont revalidés. La tentative workspace complète a dépassé 300 secondes et n’est pas déclarée réussie ; son reçu incomplet reste séparé. Aucun résultat de la passe isolée n’est substitué à ces contrôles ciblés après fusion.
