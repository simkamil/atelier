---
title: 'US-030/031 — XML en flux et écriture OOXML'
type: 'feature'
created: '2026-10-10'
status: 'done'
route: 'dispatch'
review_loop_iteration: 0
context:
  - '{project-root}/SPEC.md'
  - '{project-root}/US-V0-V1.md'
baseline_commit: 'ff7f3a3097ae23792afda188c3b5d0b739b76b5d'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** Les formats OOXML et ODF ont besoin d’une lecture XML sûre et incrémentale, puis d’une sortie XML reproductible. Un parseur qui remplace les octets malformés, pré-décode tout le document ou construit un DOM ne répond ni aux garanties de fidélité ni aux limites de ressources du projet.

**Approach:** Ajouter à `atelier-xml` un lecteur événementiel générique sur `std::io::Read` et un écrivain sur `std::io::Write`. Le lecteur décode UTF-8/UTF-16 au fil des octets et émet des événements XML avec noms d’éléments et attributs qualifiés et résolus. L’écrivain produit une représentation déterministe, échappée et relisible par le lecteur.

## Boundaries & Constraints

**Always:** Implémentation Rust stable, sans dépendances externes ni `unsafe`, SPDX `AGPL-3.0-or-later`. Les buffers, piles de scopes et événements sont bornés par des limites configurables; vérifier une limite avant toute réservation/allocation qu’elle gouverne. Valeurs par défaut sûres, erreurs structurées avec position (ligne et colonne, plus offset d’octet utile), et aucune panique sur une entrée invalide. Le mode strict rejette erreurs de décodage, caractères interdits par XML et syntaxe malformée; il ne transforme jamais silencieusement ces entrées en U+FFFD. L’API tolérante est explicitement opt-in, documente chaque réparation permise et conserve les diagnostics; elle ne récupère pas les erreurs de codage ni les caractères XML illégaux par remplacement silencieux. Les scopes de namespaces suivent l’imbrication, préservent les bindings hérités, appliquent la règle du namespace par défaut aux éléments mais pas aux attributs sans préfixe, et détectent les attributs en double selon leur nom étendu `(URI, local)`. DTD ignorée sans résolution externe, accès réseau, expansion d’entités ou expansion récursive. Seules les entités prédéfinies et numériques sont décodées.

**Never:** Lire le fichier entier pour produire une chaîne avant de parser, construire/retourner un DOM, ajouter une dépendance Cargo, exécuter des DTD, ou capturer/réémettre des sous-arbres opaques (US-032). Les consommateurs peuvent agréger eux-mêmes les événements si leur modèle métier le permet.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| UTF frontières | UTF-8 multi-octet ou paire UTF-16 fragmentée entre lectures, y compris une paire de substitution | Texte exact, événement seulement après décodage complet | Fin tronquée ou surrogate invalide: erreur de codage stricte positionnée |
| XML courant | Éléments, attributs, espaces de noms, commentaire, PI, CDATA et références prédéfinies/numériques | Événements ordonnés; CDATA/text distingués ou documentés comme texte équivalent | Entité inconnue, référence invalide, doublon de nom étendu: erreur XML |
| DTD | Déclaration interne/externe avec entité imbriquée ou URI | DTD ignorée sans expansion ni I/O externe | Refus/diagnostic sûr selon la syntaxe; aucune requête ni croissance d’expansion |
| Limite dépassée | Profondeur, nombre d’attributs, longueur d’attribut/texte ou budget de mémoire au-dessus du plafond | Arrêt avant allocation dépassant le plafond | Erreur de limite avec position |
| Écriture | Texte/attribut contenant `&`, `<`, guillemets, caractères Unicode; namespaces imbriqués | Échappement valide, déclaration et préfixes déterministes, structure équilibrée | Écriture déséquilibrée ou I/O interrompue: erreur, jamais de fermeture inventée |

Propriété exclusive : `crates/xml/**`, `docs/us-030-031-xml.md`, ce spec. Aucun fichier partagé, Git, Graphify ou sous-agent ; le coordinateur gère CI/backlog et graphe déjà rafraîchi. Cargo utilise `--locked --offline`, fmt limité à la crate. Références indépendantes dans `tests/format-reference/xml/`, en lecture seule. Le lecteur strict vise XML 1.0 avec normalisation de fins de ligne/attributs et noms Unicode valides ; la tolérance doit réparer au moins un cas précisément testé, par exemple fermeture finale manquante, sans assouplir les limites. Writer contrôle aussi noms, caractères, déclarations et doublons : les champs publics d'événements ne sont pas réputés valides par construction. Tester qu'un premier événement arrive avant consommation de la fin du flux, et chaque frontière de BOM/multioctet/surrogate.

</frozen-after-approval>

## Code Map

- `crates/xml/src/lib.rs` — façade publique et modules d’événements, tokenizer, décodage incrémental, namespaces, écrivain et erreurs.
- `crates/xml/Cargo.toml` — crate workspace sans dépendance externe; respecter les métadonnées et lints du workspace.
- `crates/unicode/src/lib.rs` — fournit `Encoding`, mais `decode()` remplace les séquences invalides; le parseur strict ne doit pas l’utiliser comme chemin de validation.
- `crates/fmt-ooxml/` et `crates/fmt-odf/` — consommateurs futurs des événements XML; garder l’API indépendante de ces formats.
- `SPEC.md` §6.2/§6.13 et `US-V0-V1.md` US-030/031 — exigences d’architecture, sécurité et traçabilité.

## Tasks & Acceptance

**Execution:**
- [x] `crates/xml/src/lib.rs` — définir API `Reader<R: Read>`, événements, options/limites, erreurs positionnées, résolution namespace et `Writer<W: Write>` équilibré; garder la consommation incrémentale et bounded.
- [x] `crates/xml/src/lib.rs` — couvrir frontières de codage, grammaire XML, namespaces, DTD et plafonds par tests unitaires adversariaux et de round-trip.
- [x] `docs/us-030-031-xml.md` — documenter API, sémantique stricte/tolérante, défauts et exemples d’usage pour les consommateurs.

**Acceptance Criteria:**
- Given un flux UTF-8 ou UTF-16 valide livré en fragments arbitraires, when le lecteur est consommé, then les événements et scalaires Unicode sont identiques à ceux du flux complet, sans bufferiser le document entier.
- Given une entrée à octets invalides, caractère XML interdit, entité inconnue ou namespace/attribut en conflit, when le mode strict parse, then il retourne une erreur avec ligne/colonne sans remplacement silencieux ni panique.
- Given une DTD malveillante ou un plafond dépassé, when le lecteur avance, then aucune entité n’est développée ni ressource externe consultée et l’erreur intervient avant allocation au-delà du plafond.
- Given événements équilibrés avec noms et namespaces, when l’écrivain sérialise puis le lecteur relit, then l’arbre d’événements et les noms étendus sont équivalents; les mêmes événements produisent les mêmes octets.
- Given des événements de fermeture sans ouverture correspondante, when l’écrivain écrit, then il retourne une erreur d’état sans produire une sortie prétendument valide.

## Implementation Notes

État du 2026-10-10 : lecteur/écrivain et documentation d'usage présents. Le lecteur conserve une table de bindings namespace avec checkpoints d'imbrication, sans recopier les scopes hérités; le budget cumulé inclut les bindings actifs, les noms/piles et les buffers d'événements, y compris la cible PI et les diagnostics de réparation EOF. Le scanner DOCTYPE ignore les déclarations sans expansion externe/interne et traite les commentaires, quotes et crochets sans croissance proportionnelle à leur contenu. Les erreurs rendent `next_event()` terminal. La normalisation CR/CRLF dans texte, attributs, commentaires et PI est documentée/testée.

Validation finale : 140 tests du workspace, dont 21 ZIP et 28 XML; les 49 tests ZIP/XML passent aussi en release. Treize contrôles globaux réussissent, 31 paquets locaux et lock préservé. Oracles finaux : XML 105 lectures/105 réécritures/20 rejets, ZIP 52 lectures/48 écritures et quatre variantes de descripteurs; Explorer classique/Zip64 réussi. Trois revues indépendantes triées, correctifs vérifiés et aucun constat différé. Preuves dans `formats-validation.json`, `format-oracle.json`, `explorer-zip.json` et `formats-review.md`.

## Spec Change Log


## Review Triage Log

Trois revues indépendantes reçues et triées avant regroupement; voir [revue complète](formats-review.md). Correctifs validés; aucun constat différé.

| ID | Domaine | Verdict | Constat | Preuve / décision | Route |
|---|---|---|---|---|---|
| B6 | XML | false | Deux déclarations xmlns:p identiques ne seraient pas rejetées. | parse_start compare chaque nom lexical dans raw_attrs avant résolution ou traitement xmlns. Le test strict_grammar_rejects_non_xml_whitespace_references_and_prolog contient les déclarations répétées et obtient une erreur. | reject |
| B7 | XML | false | Le nom DOCTYPE différent de la racine violerait la bonne formation XML. | W3C XML 1.0 §2.8 classe Root Element Type comme contrainte de validité, pas de bonne formation. Ce lecteur ignore la DTD et ne fait pas de validation DTD. | reject |
| B8 | XML | false | La pile est modifiée avant une erreur de fermeture, compromettant la reprise. | Après toute erreur, next_event est terminal et la pile est privée; aucune reprise après balise invalide n’est exposée. direct_next_event_is_terminal_after_parse_error protège ce contrat. | reject |
| B9 | XML | false | Des bindings au même préfixe et URI différentes seraient supprimés silencieusement. | qname génère nsN à partir de l’index unique de chaque URI dans Writer.uris. Aucun binding de préfixe fourni par l’appelant n’entre directement dans bindings; les deux URI reçoivent des préfixes distincts. | reject |
| B10 | XML | false | finish pourrait réussir après une erreur antérieure du writer. | write_event positionne finished=true pour tout Err de write_inner; finish refuse finished, une racine absente ou une pile ouverte. Les tests writer_requires_one_root_and_preserves_numeric_carriage_return et writer_round_trip_and_balancing vérifient les états en erreur. | reject |
| E1 | XML | medium | L’événement N+1 est construit avant le refus max_events. | Confirmé : next_inner précédait le contrôle du compteur. Le garde est maintenant placé avant le parsing; après une racine complète au budget exact, seule la fin blanche/EOF est consommée sans buffer d’événement. Tests zéro lecture, budget exact et payload non consommé. | patch |
| E2 | XML | medium | La promesse de plafonnement des événements est falsifiée par cette construction N+1. | Même chemin démontré que E1; ce constat de claim est consigné séparément avant regroupement. Le correctif E1 et ses tests s’appliquent aussi à cette promesse. | patch — groupe E1 |

## Verification

**Commands:**
- `cargo test -p atelier-xml --locked --offline` — réussite, y compris tests de flux fragmentés, limites et aller-retour.
- `cargo check -p atelier-xml --target wasm32-unknown-unknown --locked --offline` — réussite sans nouvelle dépendance ni `unsafe`.
- `cargo clippy -p atelier-xml --all-targets --locked --offline -- -D warnings` — réussite.

**Manual checks:** Inspecter que les tests fragmentent volontairement UTF-8 au milieu d’une séquence et UTF-16 entre octets/entre surrogates; vérifier que les compteurs et plafonds précèdent les allocations et que le mode tolérant est documenté sans affaiblir le strict.
