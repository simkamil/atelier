# Revue du premier éditeur intégré — 2026-10-11

Trois revues indépendantes GPT-6 Luna moyen : blind hunter, edge-case hunter et verification gap. Les trois résultats ont été collectés avant triage. Diff initial exhaustif depuis 3cb335442bc3b199b9e25ab177bcce396cf2cc55 : voir editor-integration-review-snapshot.json.

| ID | Verdict | Route | Preuve / décision |
|---|---|---|---|
| B1 | medium | patch | Le Canvas seul ne rend pas le texte accessible. Ajout d’une représentation textuelle DOM lisible avec textContent et vérification navigateur ; la validation assistive complète reste ouverte. |
| B2 | false | reject | Le shell natif refuse toute seconde navigation top-level dans Queue.begin_navigation, même propre : le lien ne détruit pas le document dans le produit Windows. Le wordmark devient néanmoins statique pour éviter une action sans effet et protéger les aperçus web. |
| B3 | medium | defer | Absence de copy/cut vérifiée dans text-input.mjs, déjà présent avant ce lot. Le presse-papiers relève explicitement de la story V1 US-225, distincte du démonstrateur V0 US-120 ; limitation conservée pour les prochains travaux. |
| B4 | medium | defer | Ctrl+flèche par mot est reconnu par l’adaptateur préexistant mais refusé explicitement par ce contrôleur. La story V1 US-221 porte ce comportement ; erreur visible et contenu préservé, pas de revendication que cette story est livrée. |
| B5 | false | reject | Le produit V0 est un exécutable Windows/Win32 ; il ne peut être lancé sous macOS. Les raccourcis Command du navigateur ne prouvent aucune prise en charge macOS et ce lot n’en revendique pas. |
| B6 | low | reject | Le wrapping aux graphèmes est effectif, sans altération du texte. La mise en page du démonstrateur est explicitement simplifiée ; préférer UAX14 correspond à US-210. La correction demanderait un nouvel algorithme plutôt qu’une correction directe. |
| B7 | low | defer | La colonne visuelle préférée n’est pas conservée après une ligne courte ; US-214 contient explicitement ce critère V1. La navigation simplifiée actuelle est documentée sans clôturer US-214. |
| B8 | medium | patch | Sélectionner uniquement un LF/CRLF/CR produit zéro rectangle. Ajouter un fin rectangle de fin de ligne pour un saut explicite sélectionné et un test de régression. |
| B9 | low | reject | Tab conserve la navigation de focus standard de la textarea ; les tabulations chargées sont rendues comme quatre espaces. US-211 porte les tabulations du traitement de texte. Intercepter Tab sans mode d’échappement créerait un piège clavier ; pas de changement dans ce prototype. |
| B10 | low | patch | Le heading reste Document sans titre malgré une ouverture. Le protocole ne divulgue ni nom ni chemin ; remplacer le heading par Document texte évite cette qualification incorrecte sans nouvelle métadonnée. |
| E1 | medium | patch | BigInt accepte un Number non sûr déjà arrondi. Refuser Number non entier sûr avant conversion, en conservant la capacité u64 complète pour BigInt ; test codec. |
| V1 | medium | patch | Les tests injectent une sélection après le dialogue et ne prouvent pas le routage Open/Save. Factoriser la route privée de production pour injecter des résultats de dialogue contrôlés et vérifier read/write/cancel/error via le même chemin ; les dialogues réels restent manuels. |

Les corrections retenues sont confiées aux trois agents d’implémentation d’origine, avec tests ciblés avant validation globale. Les fonctions V1 différées restent dans leurs stories ouvertes ; elles ne sont pas incluses dans une annonce de MVP complet.

Le contrôle pré-revue a échoué au contrôle SPDX du nouveau test Rust (en-tête absent), après les vérifications fonctionnelles debug/release réussies. Le relevé est conservé dans editor-integration-pre-review-validation.json ; il ne constitue pas la validation finale.

## Vérification visuelle complémentaire

Le vrai point d’entrée, initialisé avec le moteur emballé, a été inspecté dans Edge headless à 1280×900. La page A4 était transparente sur le fond gris et le body prenait le défilement. Le Canvas propose maintenant un fond de page optionnel (transparent par défaut pour les autres consommateurs), activé en blanc dans l’éditeur. Le shell est borné au viewport et le document défile dans son propre conteneur. Les nouveaux tests vérifient pixels blancs/gris, absence de scrollbar externe et plage de défilement interne ; les suites Canvas historiques passent.

Les cinq patches de revue sont appliqués. Les tests ciblés vérifient le miroir accessible, les u64 sûrs, la sélection du saut explicite et les routes fichiers communes. Le défaut SPDX est corrigé. La validation globale finale est relancée sur les sources stabilisées.

## Résolution des tests ciblés

- B1/B10/E1 : codec u64 (Number non sûr refusé et BigInt u64 maximal conservé), miroir DOM textuel accents/CRLF, marque statique et titre générique vérifiés dans le navigateur réel.
- B8 : `selecting_only_an_explicit_newline_emits_a_thin_selection_rectangle` passe ; LF, CRLF et CR sont enregistrés comme sauts explicites dans le layout, contrairement aux wraps automatiques.
- V1 : `route_file_request` partage la route de production Open/Save avec les tests contrôlés. Ils vérifient les types, lecture/écriture réelles, annulation et erreurs. Les appels COM/dialogues eux-mêmes restent à valider manuellement.
- Le fond de page blanc est optionnel dans le renderer générique ; les anciens tests Canvas et les nouvelles assertions de pixels/défilement passent.

Les commandes globales et leurs résultats complets sont conservés dans editor-integration-validation.json.
