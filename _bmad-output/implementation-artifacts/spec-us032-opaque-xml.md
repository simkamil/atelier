---
title: 'US-032 Sous-arbres XML opaques'
type: 'feature'
created: '2026-10-10'
status: 'done'
route: 'dispatch'
review_loop_iteration: 0
baseline_commit: '3aa4d19bef7e8e917b90136f80252bd988e1f7b0'
context:
  - '{project-root}/docs/us-030-031-xml.md'
  - '{project-root}/_bmad-output/implementation-artifacts/epic-0.4-context.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** Un consommateur XML qui ne comprend pas un élément doit le garder pour l’enregistrement. Les événements ordinaires perdent les déclarations namespace lexicales, et le Writer peut renommer les préfixes; ce renommage peut invalider des valeurs QName, sans préfixe, dans les attributs ou le texte.

**Approach:** Ajouter une capture bornée d’un sous-arbre à partir d’un `StartElement` déjà lu, puis une réémission sémantique autonome dans n’importe quel contexte namespace. Conserver événements, préfixes effectifs et mappings en portée, puis laisser une voie opaque validée du Writer produire des déclarations locales cohérentes.

## Boundaries & Constraints

**Always:** « Brut » désigne une fidélité sémantique XML : noms développés, préfixes effectifs, valeurs et séquence d’événements (texte, commentaires, PI compris). L’encodage, les espaces entre balises, les choix d’entités, la forme vide/pleine et les octets exacts ne sont pas garantis. Le captureur inclut les mappings hérités et locaux nécessaires, y compris ceux utilisés seulement dans une valeur QName; le parseur XML générique ne peut identifier ces valeurs par schéma, donc il préserve les mappings du scope. Vérifier profondeur, événements, taille cumulée et allocations avant croissance. Traiter un `StartElement` déjà consommé sans le relire; supporter son cas vide. Garder DTD sans expansion ni accès externe. Les erreurs et dépassements sont explicites et terminaux pour la capture en cours.

**Never:** Accepter une chaîne/XML brute arbitraire ou écrire par injection non vérifiée. Changer la sémantique des événements Reader/Writer existants, autoriser les entités externes ou prétendre préserver les octets exacts.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Racine capturée déjà lue | Start non vide, descendant préfixé | Opaque complet jusqu’à sa fermeture | Retourne erreur de limite avant allocation excessive |
| Élément vide | Start `empty=true` | Capture complète immédiatement, aucun événement suivant consommé | — |
| Préfixe hérité ou redéfini | Shadowing, héritage, reset du défaut | Noms et préfixes effectifs conservés par scope | Namespace invalide propagé |
| Valeur QName | `xsi:type="p:Widget"` ou texte QName | Mapping `p` conservé, même dans contexte cible différent | — |
| UTF-16 / fin tronquée | Source UTF-16, capture incomplète | Capture sémantique décodée; aucune fausse complétion | Erreur encodage/syntaxe du Reader |
| Fragment trop grand | Dépasse limite événement/profondeur/octets | Aucun résultat partiel publiable | Erreur `Limit` |

</frozen-after-approval>

## Code Map

- `crates/xml/src/lib.rs` -- `Reader`, `Limits`, `Event`, résolution et pile namespace bornée; `Writer`, échappement, validation et préfixes générés.
- `docs/us-030-031-xml.md` -- contrats publics, normalisations, limites et comportement encodage/DTD des US-030/031.
- `US-V0-V1.md` -- dépendances et critères de la story opaque.
- `SPEC.md` -- préservation de l’inconnu et exigence de robustesse des entrées non fiables.

## Tasks & Acceptance

**Execution:**
- [x] `crates/xml/src/lib.rs` -- concevoir une capture opaque bornée depuis Start déjà lu et une méthode Writer dédiée qui réémet noms/préfixes et mappings validés, sans injection brute.
- [x] `crates/xml/src/lib.rs` -- couvrir héritage, shadowing, reset du namespace défaut, valeurs QName, UTF-16, limites et troncature.
- [x] `docs/us-030-031-xml.md` -- documenter le contrat, la signification de « brut », les limites et l’API publique après implémentation.

**Acceptance Criteria:**
- Given un start déjà retourné par le Reader, when la capture consomme son contenu, then elle produit un unique sous-arbre autonome sans consommer l’événement suivant; si vide, elle termine immédiatement.
- Given des mappings namespace hérités, redéfinis ou réinitialisés, when le sous-arbre est écrit sous un parent avec d’autres bindings, then éléments, attributs et préfixes enregistrés gardent leur association URI/local.
- Given un préfixe utilisé dans une valeur d’attribut ou texte de forme QName, when l’arbre est déplacé, then son mapping reste celui capturé même si le Writer normal génère d’autres préfixes.
- Given une limite dépassée ou un XML tronqué, when on capture, then aucune capture partielle n’est émise et une erreur bornée est retournée; aucun contenu DTD externe n’est chargé.
- Given commentaires, PI et texte dans l’arbre, when réémis puis reparsé, then leur ordre et contenu sémantique sont conservés conformément aux normalisations XML du Reader.

## Implementation Notes

Propriété exclusive : `crates/xml/**`, `docs/us-032-opaque-xml.md` (nouveau), `docs/us-030-031-xml.md` et ce spec. Un module `opaque.rs` peut isoler la logique et ses tests. Le coordinateur gère Cargo.lock, backlog, Graphify, preuves globales et Git. Aucun sous-agent supplémentaire. Utiliser Cargo `--locked --offline`; formatage limité à atelier-xml. Le graphe a été rafraîchi par le coordinateur; le lire suffit ici.

La capture doit vérifier qu'il s'agit du dernier StartElement effectivement retourné par ce Reader, avant tout événement suivant; ne pas permettre à un Event fabriqué ou périmé de devenir une capture fiable. Les bindings de l'élément vide doivent rester disponibles pour capture, tout en étant retirés avant le prochain événement. Éviter une copie de tous les namespaces à chaque événement ordinaire.

## Spec Change Log


## Review Triage Log

- B05 : medium ; les map_err de réservation opaque ne nettoient pas systématiquement failed/capture_memory. Correctif terminal commun avec test déterministe.
- E01 : medium ; confirmation indépendante de B05, même correctif.
- B08 : false-positive ; compteurs d’allocations réelles bornées, saturation sans retour à une valeur faible, réservations impossibles refusées.
- B09 : false-positive ; réparation tolérante uniquement à EOF, aucun nouveau Start après diagnostic.
- B10 : low ; dater la preuve XML historique et relier le nouveau lot.

Voir [revue complète](editing-review.md), triée après réception des trois rapports.


## Verification

**Commands:**
- `cargo test -p atelier-xml --locked --offline` -- réussite des tests unitaires de capture, limites et aller-retour.
- `cargo fmt -p atelier-xml -- --check` -- code formaté.

**Manual checks (if no CLI):**
- Inspecter une sérialisation capturée déplacée sous des bindings conflictuels, puis reparsée pour comparer événements, noms développés, préfixes et valeurs QName.

## Validation finale — 2026-10-10

Implémentation et correctifs ciblés validés ; 160 tests du workspace et 76 tests release passent. Les quatorze contrôles globaux et l’oracle indépendant réussissent, avec empreintes de sources inchangées pendant les contrôles. Revue triée sans constat différé. Preuves : [validation](editing-validation.json), [oracle](editing-oracle.json), [revue](editing-review.md) et [rendu SVG](editing-svg-visual.json). La vérification navigateur est indisponible ; le témoin SVG exact a été rasterisé localement et inspecté, avec huit points de contrôle.
