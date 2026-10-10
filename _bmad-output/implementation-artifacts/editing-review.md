# Revue US-032, US-072, US-081 — 2026-10-10

Trois agents neufs GPT-6 Luna, raisonnement moyen, ont examiné en parallèle le diff complet depuis `3aa4d19bef7e8e917b90136f80252bd988e1f7b0`. Tous leurs rapports ont été reçus avant le triage. Diff initial : 461 343 octets, SHA-256 `981196b01339eb995a196b2390723d2d3d800c5c602f494075541070e55a8146`. Le seuil du reviewer aveugle était `min(floor(sqrt(461343/1024)+1),10)=10`.

## Triage individuel avant regroupement

| ID | Constat | Verdict | Preuve et disposition |
|---|---|---|---|
| B01 | Une image entièrement clippée est encore résolue et comptée. | false-positive | La première passe valide toutes les références de la DisplayList avant toute sortie, conformément au contrat de ressources valides. Les plafonds sont conservateurs ; aucun dessin masqué ne permet de contourner le contrôle d’une ressource invalide. Préciser cette portée dans la doc. |
| B02 | Risque de désynchronisation de clip_cursor après un clip vide. | low | Les deux passes emploient la même condition de clip vide ; active_clip_id avance seulement si command_index correspond. Aucun défaut constaté. Ajouter néanmoins une régression qui encadre une intersection vide par deux dessins visibles pour protéger cette frontière. |
| B03 | color_rgb alloue une String temporaire avant l’écriture bornée. | low | L’allocation est petite et bornée par les trois valeurs numériques, mais facilement évitable. Remplacer par un formatage direct, à octets SVG identiques. |
| B04 | Les quatre MIME acceptés ne seraient pas documentés. | false-positive | SvgImage, SvgResourceProvider et docs/us-081-svg-backend.md listent déjà PNG, JPEG, GIF et BMP canoniques, le contrôle de signature et l’absence de décodage. Les autres encodages relèvent du fournisseur appelant. |
| B05 | Les erreurs try_reserve d’une capture opaque ne rendent pas systématiquement le Reader terminal. | medium | Le rapport cite history.rs par erreur ; la surface existe dans xml/src/lib.rs, aux réservations items/copy. Ces map_err sortent sans nettoyer capture_memory ou positionner failed. Centraliser le nettoyage après le début d’une capture valide, avec régression déterministe. |
| B06 | Le test de plafond d’opérations ne prouve pas l’ordre undo/redo des deux groupes. | low | Le test existant couvre déjà le plafond exact et l’ajout suivant, mais n’observe que undo_len avec opérations neutres. Renforcer valeurs et snapshots lors de quatre navigations. |
| B07 | Une sous-estimation caller d’inverses futurs ne serait pas détectée ou documentée. | false-positive | Le contrat public de apply et la section Budget logique imposent déjà une borne couvrant au maximum les directions futures, snapshots, capacités et heap. Une mesure universelle d’opérations génériques est explicitement exclue ; ce contrat est une précondition caller. |
| B08 | Les additions saturantes opaques masqueraient un overflow. | false-positive | Les compteurs portent sur des allocations réelles et des événements bornés, sans compteurs forgés. La saturation ne revient jamais à une petite valeur et excède tout plafond fini inférieur à usize::MAX ; les réservations de conteneurs refusent les capacités impossibles. Le cas exigeant une allocation réelle supérieure à l’espace d’adressage ne justifie pas une nouvelle branche. |
| B09 | Un diagnostic de réparation antérieur au start capturé serait ambigu. | false-positive | new_tolerant ne répare que les fermetures manquantes à EOF. Après ces diagnostics aucun nouveau StartElement ne peut être lu ; la capture rejette déjà une réparation survenue pendant son parcours. Le scénario proposé n’est pas atteignable via l’API. |
| B10 | Les chiffres XML antérieurs seraient périmés. | low | La phrase finale décrit le lot précédent sans le dater explicitement, alors que l’API US-032 est ajoutée au même document. Dater la preuve historique et relier le nouveau lot avec ses propres comptes. |
| E01 | Une réservation opaque échouée laisse la capture partielle comptabilisée. | medium | Confirmation indépendante de B05 sur xml/src/lib.rs:1446-1464. Même correctif, mais verdict enregistré séparément. |
| V01 | Les tests SVG n’assertent pas précisément x/y des deux glyphes. | medium | Constat de vérification confirmé sur le test de déduplication et l’oracle XML ; les pixels du témoin apportent une preuve locale complémentaire, mais les coordonnées ne sont pas directement protégées en CI Rust. Ajouter les assertions sur les attributs parsés. |
| V02 | Les tests SVG n’assertent pas x/y/width/height de l’image. | medium | Constat de vérification confirmé sur le test Base64 ; il observe le payload et le nombre d’images. Ajouter les quatre assertions géométriques sur les attributs parsés. |
| R01 | Undo peut rouvrir le groupe de frappe d’une entrée plus ancienne. | medium | Audit coordinateur : Typing A, Other B, undo B, Typing C de même segment dans la seconde. undo ferme seulement l’entrée déplacée vers redo ; A conserve typing ouvert et C fusionne avec A. Fermer le groupe exposé uniquement après un undo réussi, avec régression état/sélection. |

## Correctifs retenus

- XML : B05/E01, nettoyage terminal commun ; les deux signalements désignent le même défaut.
- Historique : R01, frontière de frappe après undo ; B06, vérification du plafond et de l’ordre des navigations.
- SVG : B02, régression de clips vides ; B03, suppression du tampon couleur ; V01/V02, assertions de géométrie ; documentation de crate corrigée.
- Documentation : B10, preuves historiques datées et liens vers ce lot ; B01, portée conservatrice des plafonds explicitée.

Tous les correctifs retenus sont appliqués et relus. Les tests ciblés des agents passent, puis les quatorze contrôles globaux réussissent : 160 tests workspace, 76 tests release, Clippy hôte/WASM, rustfmt, licence et builds Windows/WASM/musl. L’oracle indépendant passe à nouveau ; le SVG témoin reste identique octet par octet au document rasterisé et inspecté. Aucun constat différé.

Bilan des 14 constats : 5 medium (dont deux signalements du même défaut XML), 4 low et 5 false-positive. Les verdicts individuels ci-dessus font foi.
