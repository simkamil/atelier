---
title: 'US-361 — À propos et licences hors ligne'
type: feature
created: '2026-10-13'
status: done
route: dispatch
baseline_commit: f419d339daef132a29fdbdeb68ca0093aaaa0a57
context: []
---

<frozen-after-approval reason="Poursuite autonome du backlog autorisée">

## Intent

Depuis Fichier → À propos, consulter version, date de build, adresse sélectionnable du code source et les textes intégraux AGPL-3.0-or-later, des licences OFL des polices et de la licence Unicode, même hors ligne. Le parent fournit les données provenant des fichiers distribués et les métadonnées de compilation réelles. Le dialogue réutilise les composants et thèmes actuels, sans navigation externe automatique.

## Boundaries & Constraints

DOM textContent uniquement, aucune interpolation HTML ni fetch par le composant. Métadonnées `{version,buildDate,sourceUrl}` ; licence AGPL complète, trois notices OFL Carlito/Caladea/Liberation et Unicode. Les textes restent sélectionnables et consultables dans un panneau défilant. Les changements d'onglet/notice gardent un nom accessible et le focus visible. Ne pas inventer une date inconnue : metadata manquantes sont rejetées ou annoncées indisponibles par une clé injectée. La consultation reste possible pendant l'aperçu DOCX sans le modifier.

## I/O & Edge-Case Matrix

| État | Action | Résultat |
|---|---|---|
| Données valides | Ouvrir | Version/date/source exactes et licence intégrale visible |
| Notices multiples | Choisir police/Unicode | Texte complet correspondant sélectionnable |
| Texte ressemblant à du HTML | Afficher | Texte littéral, aucune exécution |
| Dialogue ouvert | Tab/flèches/Échap | Navigation et restauration du focus |
| Dispose | Réouvrir | Contrôleur inutilisable sans écouteurs persistants |

</frozen-after-approval>

## Code Map

- `web/about-dialog.mjs`, `web/about-dialog.css` : agent.
- `tests/about-dialog-browser.mjs` : suite navigateur exportable, agent.
- `web/ui-components.mjs` : createDialog/attachTabs partagés inchangés.
- `LICENSE`, `data/fonts/licenses/*-OFL.txt`, `data/ucd/17.0.0/LICENSE.txt` : textes originaux fournis par parent, jamais réécrits.
- app/shell/catalogue/build.rs/resources.rs : intégration parent, ne pas les modifier.

## Tasks & Acceptance

- [x] API `createAboutDialog({labels,metadata,licenseTexts})`, retour `{element,open,close,dispose}`. `licenseTexts={agpl,fonts:[{name,text}],unicode}` ; communiquer les clés de labels attendues.
- [x] Mise en page adaptable à une petite fenêtre, hauteur bornée et scroll intérieur pour les notices.
- [x] Tests métadonnées exactes, intégralité du texte, sélection de chaque notice, clavier/focus, injection et nettoyage.
- Given une licence fournie, when son panneau est choisi, then son texte intégral est affiché sans modification.
- Given une source URL, when le dialogue est ouvert, then elle est consultable/copîable sans déclencher de requête ou navigation.

## Implementation Notes

L'agent travaille dans les seuls nouveaux fichiers. Parent branche le dialogue et vérifie les notices exactes servies par l'exécutable. Cartographie déjà rafraîchie, ne pas toucher sa sortie canonique.

## Review Triage Log

| ID | Verdict | Vérification et décision |
|---|---|---|
| V1 | medium | Les tests du convertisseur de dates dans build.rs avaient été exécutés explicitement en local mais pas par cargo test en CI. Fonction pure déplacée dans src/build_date.rs, partagée avec build.rs et testée par le paquet desktop normal : jours UTC, années bissextiles, borne 9999. |

## Verification

Suite réelle Edge/Chromium, contrôles des ressources natives et absence d'accès réseau automatique.

Validation finale du lot : Validation Windows du 2026-10-14 : 18 contrôles, 243 tests Rust des quatre paquets concernés, 1177 entrées immuables. Les suites Edge contrôlent les composants, le produit normal/contraste élevé et le vrai Worker/WASM ; Clippy hôte/WASM, fmt, licences, i18n sans nouvelle dérogation de référence, données générées, budget WASM inchangé et sonde WebView2 passent. La sonde vérifie 49 ressources et les textes de licence embarqués. Les événements clavier sont synthétiques ; aucun audit lecteur d’écran, essai matériel ou passage Linux distant n’est revendiqué.
