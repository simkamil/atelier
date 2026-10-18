# Composants, thèmes et ruban — US-104/105/106

Le shell de l’éditeur TXT et de l’aperçu DOCX utilise désormais des composants JavaScript natifs, un ruban déclaratif et les thèmes clair, sombre et système. Les commandes disponibles conservent leurs nœuds, IDs et handlers métier. Les commandes de mise en forme, insertion et références restent désactivées tant que leurs US ne sont pas raccordées.

## Essayer le lot

Construire et lancer `cargo run -p atelier-desktop-win --release --locked --offline` dans la branche du lot. Le binaire est `target/release/atelier.exe`.

- **Fichier** : Nouveau, Ouvrir TXT, Ouvrir DOCX et Enregistrer sous.
- **Accueil** : Annuler, Rétablir et Options → Espaces françaises.
- **Affichage → Apparence** : choisir Clair, Sombre ou Système sans rechargement. Le choix est conservé sous `atelier.theme`; un stockage indisponible laisse le choix actif pour la session.
- **Alt**, puis la lettre de l’onglet, puis celle de la commande : navigation par info-touches. **Échap** quitte les info-touches et retrouve le focus précédent. **Ctrl+F1** replie/développe le ruban. Sélectionner un onglet révèle ses commandes.
- **Retour au texte** reste visible pendant l’aperçu DOCX, même si un autre onglet est sélectionné ou le ruban replié.

Le thème système suit `prefers-color-scheme` en direct. `forced-colors` utilise les couleurs système pour les contrôles et le focus. Les Canvas conservent les pixels et couleurs du document : le papier reste blanc en mode sombre. Les groupes et onglets se redistribuent quand la fenêtre est étroite; toutes les commandes restent présentes.

## Contrat des modules

| Module | Contrat |
|---|---|
| `web/ui-components.mjs` | Bouton, bascule, bouton partagé, liste déroulante, combo de police, couleurs, nombre/unité, dialogue modal, menu contextuel, onglets et info-bulle. Factories avec `element` et `dispose()`, sauf `attachTabs` qui enrichit les nœuds fournis. |
| `web/theme.mjs` | `createThemeController({root,storage,matchMedia,onChange})` expose `getTheme`, `getResolvedTheme`, `setTheme` et `dispose`. Les valeurs sont `light`, `dark`, `system`; préférences invalides stockées → système. |
| `web/ribbon.mjs` | `createRibbon({root,tabs,onCommand,labels,initialTab})` expose `selectTab`, `setCollapsed`, `refresh`, `dispose`. Onglets/groupes/commandes sont des données; les éléments fournis sont déplacés, jamais clonés. |
| `web/editor-shell.mjs` | Intègre le ruban, le dialogue Apparence et les contrôles existants; libère les listeners/DOM au démontage. |

Les textes lisibles sont injectés par le propriétaire depuis `web/i18n/fr-FR.txt`. US-102, attribuée à l’autre développeur, conserve son outil de vérification, ses pluriels et son formatage; aucun second service d’internationalisation n’est introduit. Les erreurs de programmation restent des messages techniques.

Les listes et menus utilisent flèches, Début/Fin, Entrée/Espace et Échap; les boutons natifs gardent leurs actions clavier habituelles. Le combo conserve le focus dans l’input, expose l’option active et diffère le filtrage pendant la composition IME. Le dialogue natif gère la modalité et le cycle Tab/Maj+Tab; les raccourcis du document ne s’exécutent pas derrière lui. Les onglets ont une seule entrée Tab et des panneaux reliés par ARIA. Les info-bulles fonctionnent au focus/survol et se ferment avec Échap.

Un `spec.disabled` explicite du ruban est prioritaire. En son absence, le `disabled` métier du nœud garde la priorité; `refresh` ne l’écrase pas. `dispose` restaure uniquement les modifications dont le ruban est propriétaire. Les bornes numériques configurées et le pas HTML sont validés avant callback; les unités inconnues et couleurs non prises en charge ne sont pas publiées.

## Vérification

Les tests de composants sont une galerie réservée au harness, pas un écran de démonstration ajouté au produit. Les suites s’exécutent avec Edge installé, sans driver externe :

```text
node tests/theme.mjs
node tests/run-ui-browser.mjs
node tests/run-ui-browser.mjs --forced-colors
node tests/run-ui-browser.mjs --screenshots
node tests/run-editor-browser.mjs
py _bmad-output/implementation-artifacts/validate-ui-shell.py
```

La suite vérifie le DOM réel, les relations ARIA, le focus, les callbacks, l’IME simulé, les démontages et les limites d’entrée. Le runner teste aussi les changements réels de media query du navigateur via CDP et les vues 1280/420 px de l’application avec le WASM emballé. Le contraste élevé est réellement actif dans le second lancement. Les régressions de l’éditeur conservent saisie, historique, ouverture/sauvegarde TXT et retour de l’aperçu DOCX. La CI Windows lance les suites de thèmes/composants.

Les événements clavier synthétiques et l’émulation CDP ne remplacent pas les essais physiques AZERTY/AltGr/IME, ni une lecture avec NVDA/Narrator. Aucun audit complet de conformité WCAG ou de lecteur d’écran n’est revendiqué. Le parcours MVP reste du texte brut éditable et du DOCX en lecture seule.

Preuves : [spécification et triage](../_bmad-output/implementation-artifacts/spec-ui-components-ribbon.md), [validation locale](../_bmad-output/implementation-artifacts/ui-shell-validation.json), [intégrité du graphe](../_bmad-output/implementation-artifacts/ui-shell-graph.json). Les captures de validation restent dans `target/ui-screenshots/`.

Validation locale du 13 octobre 2026 : 17 contrôles réussis, 930 tests Rust (2 optionnels ignorés), 112/114 assertions UI normal/contraste élevé, 118 éditeur et 36 Canvas/saisie. La sonde native réelle vérifie 28 ressources et les refus CSP; le journal borné rapporte sa rotation sans perdre les sondes requises.

CI distante du commit `3aeab6e` : les trois jobs [du run 38354586127](https://github.com/simkamil/atelier/actions/runs/38354586127) n’ont pas démarré, bloqués par la facturation ou le plafond du compte GitHub. La [preuve des annotations](../_bmad-output/implementation-artifacts/ui-shell-ci.json) distingue ce blocage des 17 contrôles locaux réussis.
