---
title: 'US-081 — Backend SVG'
type: 'feature'
created: '2026-10-10'
status: 'done'
route: 'dispatch'
review_loop_iteration: 0
baseline_commit: '3aa4d19bef7e8e917b90136f80252bd988e1f7b0'
context:
  - '{project-root}/docs/us-080-display-list.md'
  - '{project-root}/_bmad-output/implementation-artifacts/epic-0.9-context.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** L’impression doit produire une page vectorielle à partir de la liste d’affichage validée, tout en préservant le dessin des chemins, des glyphes et des images. Le modèle garde volontairement les références de ressources opaques.

**Approach:** Ajouter un backend isolé dans `atelier-render` qui transforme une `DisplayList` validée en document SVG déterministe. Les ressources de glyphes et d’images proviennent d’un fournisseur fourni par l’appelant; le backend ne charge ni fichiers, polices système, ni ressources réseau.

## Boundaries & Constraints

**Always:** Créer `crates/render/src/svg.rs` et n’ajouter qu’un export public minimal dans `lib.rs`. Valider la liste avant rendu. Le fournisseur résout `(font_id, glyph_id)` vers un `DisplayPath` dont les contours sont en unités de page relatives à l’origine du glyphe; revalider ces chemins, car les champs publics permettent de construire une valeur invalide. Il résout `image_id` vers MIME et octets. Accepter uniquement les types MIME PNG, JPEG, GIF et BMP avec signature initiale correspondante; intégrer les octets en URI `data:` Base64 sans décoder l’image. Définir chaque contour glyphique une fois par paire police/glyphe dans `<defs>`, puis le réutiliser par `<use>`. Préserver l’ordre de peinture, couleurs et alpha, règle de remplissage, caps/joints/mitre, lignes, quadratiques, cubiques et fermeture de chemin. Limiter la sortie et les ressources par comptage vérifié avant toute croissance excédant le budget.

**Never:** Ajouter une dépendance de production pour les polices, images, Base64 ou XML; décoder ou interpréter les formats d’image; accéder aux polices système ou au réseau; émettre script, URL active, balisage non échappé ou XML mal formé. Ne pas modifier `atelier-geom`, le codec ATDL ou les contrats de US-080.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Page valide | Liste complète et ressources disponibles | Un SVG autonome, déterministe, avec `viewBox` des bornes page | Aucune |
| Ressource absente | Identifiant image ou police/glyphe non résolu | Aucun SVG partiel | `MissingResource` |
| Données image invalides | MIME inconnu ou MIME autorisé avec octets vides/invalides selon le contrôle de signature retenu | Aucun SVG partiel | `InvalidMime` ou `InvalidBytes` |
| Liste invalide | Valeurs, chemins ou piles incohérents | Aucun rendu | `InvalidList` |
| Budget dépassé | Image trop grande, trop de données embarquées ou sortie au-delà de la limite | Refus avant dépassement mémoire | `LimitExceeded` |

</frozen-after-approval>

## Code Map

- `crates/render/src/svg.rs` -- sérialisation SVG, résolution des ressources, limites et tests.
- `crates/render/src/lib.rs` -- export public minimal et types validés déjà exposés (`DisplayList`, `DisplayPath`, `DisplayTransform`, `Command`).
- `crates/geom/src/lib.rs` -- contrat de `Point`, `Rect`, coordonnées et `then`; référence en lecture seule.
- `docs/us-080-display-list.md` -- unités et sémantique des états; référence en lecture seule.

## Tasks & Acceptance

**Execution:**
- [x] `crates/render/src/svg.rs` -- fournir l’API ressource, erreurs typées, validation/rendu borné et sérialisation déterministe.
- [x] `crates/render/src/lib.rs` -- exposer le module et l’entrée de rendu sans élargir l’API existante inutilement.
- [x] `crates/render/src/svg.rs` -- tester déduplication glyphes, Base64 images, transformations non commutatives, clips croisés, échappement, répétabilité, limites et erreurs.
- [x] `crates/render/src/svg.rs` -- analyser le SVG produit par un parseur XML indépendant; placer `atelier-xml` en dépendance de développement seulement si son API convient.

- [x] `docs/us-081-svg-backend.md` -- documenter API, ressources, unités, limites et exemple.

**Acceptance Criteria:**
- Étant donné une liste validée et toutes ses ressources, quand le backend la rend, alors il retourne exactement un SVG complet par liste/page et deux appels identiques donnent les mêmes octets.
- Étant donné un glyph répété dans la page, alors sa définition unique est référencée par plusieurs `<use>`; chaque image est embarquée en Base64 avec MIME autorisé.
- Étant donné une pile de clips et une pile de transformations dont les pushes/pops se croisent, alors les clips restent en coordonnées page et ne sont jamais transformés par les matrices de dessin. Une transformation est composée par `current.then(next)`; un test translation/échelle ou rotation non commutatif prouve l’ordre.
- Étant donné un lien ou rôle contenant `&`, `<`, guillemets ou caractères Unicode, alors ses métadonnées sont échappées correctement et ne créent ni attribut actif ni élément XML injecté.
- Étant donné une ressource absente, octets/MIME invalides, liste invalide ou limite dépassée, alors le rendu renvoie l’erreur typée correspondante sans exposer de sortie partielle ni franchir le budget.
- Le document est analysable par `atelier-xml` ou un parseur XML indépendant; il ne contient aucun script ni référence externe. Préparer une validation visuelle dans navigateur pour la revue d’implémentation.

## Implementation Notes

Propriété exclusive : `crates/render/**`, `docs/us-081-svg-backend.md` (à créer) et ce spec. Le coordinateur gère Cargo.lock, backlog, preuves globales, Graphify et Git. Aucun sous-agent supplémentaire. Ceci est le handoff d’implémentation du workflow BMAD déjà rendu et suivi par le coordinateur; ne pas réinvoquer de skill ni refaire le planning. Cargo `--locked --offline`, formatage limité à atelier-render. Graphe initial rafraîchi par le coordinateur : lecture du rapport uniquement ici. Le coordinateur prépare une dépendance de développement locale atelier-xml pour les tests; aucune dépendance de production nouvelle.

Avant validation/copie d'un contour fourni, vérifier son nombre d'éléments et le cumul de chemins résolus, afin que la validation géométrique ne puisse allouer hors budget. Les limites incluent copies/mappings de ressources et tampons temporaires; une allocation d'un fournisseur externe reste sous responsabilité de ce fournisseur. Documenter les valeurs par défaut et ce que les signatures d'image ne prouvent pas (décodage/validité complète).

Garder les ID SVG stables, dérivés de l’ordre d’apparition ou de clés déterministes, et l’ordre des attributs fixe. Écrire nombres finis avec représentation décimale round-trip stable. Pour chaque primitive, émettre l’état de clip page dans un groupe extérieur puis la transformation composite dans un groupe intérieur; ainsi les portées de balises n’ont pas à simuler les deux piles indépendantes. Calculer l’intersection rectangulaire cumulée en coordonnées page et supprimer ou clipper à vide les dessins correspondants. Un marqueur `Structure` n’indique pas de portée dans le contrat actuel : le préserver à sa position comme métadonnée, sans inventer de groupe englobant. Un lien peut être conservé comme métadonnée inerte associée à sa zone page, sans produire de navigation active.

API indicative : `SvgResourceProvider::glyph_outline(font_id, glyph_id) -> Option<&DisplayPath>` et `image(image_id) -> Option<SvgImage<'_>>`, où `SvgImage` emprunte un type MIME canonique et des octets. `render_svg(list, provider, limits) -> Result<String, SvgError>` reçoit des limites explicites (sortie, taille d’image et cumul embarqué). Erreurs minimales : `MissingResource`, `InvalidBytes`, `InvalidMime`, `InvalidList`, `LimitExceeded`. Les accès fournisseur sont en mémoire et synchrones; le backend ne définit pas le chargement des ressources.

## Verification

**Commands:**
- `cargo test -p atelier-render --locked --offline` -- tests du backend et analyse XML réussis.
- `cargo clippy -p atelier-render --all-targets --locked --offline -- -D warnings` -- aucune alerte.
- `cargo fmt -p atelier-render -- --check` -- format conforme.

**Manual checks (if no CLI):**
- Ouvrir un SVG représentatif dans un navigateur et vérifier courbes, glyphes, transparence, images, clips croisés et transformations.
- Vérifier qu’aucune dépendance de production ni accès externe n’a été ajouté.

## Spec Change Log

## Review Triage Log

- B01 : false-positive ; première passe volontairement stricte sur toutes les ressources, plafonds conservateurs ; expliciter la portée.
- B02 : low ; command_index empêche la désynchronisation suspectée, ajouter une régression visible/vide/visible.
- B03 : low ; supprimer la petite allocation temporaire color_rgb via formatage direct.
- B04 : false-positive ; quatre MIME canoniques et contrôle de signature déjà documentés.
- V01 : medium ; ajouter assertions x/y de chaque glyphe parsé dans les tests CI.
- V02 : medium ; ajouter assertions x/y/width/height de l’image parsée dans les tests CI.

Voir [revue complète](editing-review.md), triée après réception des trois rapports.

## Validation finale — 2026-10-10

Implémentation et correctifs ciblés validés ; 160 tests du workspace et 76 tests release passent. Les quatorze contrôles globaux et l’oracle indépendant réussissent, avec empreintes de sources inchangées pendant les contrôles. Revue triée sans constat différé. Preuves : [validation](editing-validation.json), [oracle](editing-oracle.json), [revue](editing-review.md) et [rendu SVG](editing-svg-visual.json). La vérification navigateur est indisponible ; le témoin SVG exact a été rasterisé localement et inspecté, avec huit points de contrôle.
