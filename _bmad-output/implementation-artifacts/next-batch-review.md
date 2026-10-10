# Revue du lot US-020, US-021, US-071 et US-080

Date : 2026-10-10. Baseline : `b9297cd3a94293a913b2b79ba66c06e3acf4024c`.
Trois revues indépendantes exécutées en parallèle avec GPT-6 Luna, raisonnement
moyen : diff aveugle, cas limites et lacunes de vérification. Le diff initial
contenait 38 fichiers (259 069 octets). Tous les résultats ont été reçus avant
le triage. Chaque constat est conservé séparément, y compris ceux réfutés.

| ID | Source | Constat | Évaluation | Résolution / preuve |
| --- | --- | --- | --- | --- |
| B1 | Aveugle | Documentation compression affirmant que Cargo.lock ne change pas malgré l'ajout render→geom du lot. | `low`, formulation ambiguë ; `patch`. | Corrigé : compression sans dépendance ajoutée, mise à jour locale render→geom explicitement distinguée. |
| B2 | Aveugle | Incohérence présumée entre quatre corpus et seize flux annoncés. | `false`, rejeté. | Quatre corpus × deux niveaux × deux framings = seize ; les vecteurs de décodage forment un ensemble distinct. |
| B3 | Aveugle | « écrit dans directement dans » dans la documentation. | `low`, coquille ; `patch`. | Corrigé. |
| B4 | Aveugle | Mémoire O(n) de l'encodeur non explicitée. | `low`, contrat documentaire incomplet ; `patch`. | Corrigé : tokens et chaînes de positions proportionnels à l'entrée ; mémoire bornée indépendamment du volume seulement pour inflate. |
| B5 | Aveugle | Tri répété des nœuds Huffman supposé quadratique sur une grande entrée. | `false`, rejeté. | Les alphabets sont bornés par le format : 286 littéraux/longueurs, 30 distances et 19 longueurs de codes. Leur taille ne croît pas avec l'entrée. |
| B6 | Aveugle | L'encodeur ATDL contrôle 64 Mio après une commande et peut allouer au-delà du budget. | `medium`, confirmé ; `patch`. | Corrigé : pré-calcul exact avec arithmétique vérifiée avant validation/allocation/écriture ; tests des dix opcodes et refus avec budget réduit. |
| B7 | Aveugle | Composition de transformations validée en fin de décodage avec InvalidValue. | `false`, rejeté. | Les compositions sont bien validées avant tout retour ; une valeur numérique composée invalide relève de InvalidValue. Aucun code InvalidState n'est promis pour ce cas. |
| B8 | Aveugle | Rejet de version mineure inconnue prétendument non documenté. | `false`, rejeté. | Le format fixe 1.0 et la documentation précise que les versions inconnues sont refusées. Aucun contrat de compatibilité future n'est promis. |
| B9 | Aveugle | « charges utiles vérifiées » pourrait impliquer les préconditions de l'état dès decode. | `low`, précision utile ; `patch`. | Corrigé : decode valide le format sans état ; invert/apply contrôlent les préconditions sur l'état. |
| B10 | Aveugle | Commandes documentées insuffisantes pour reproduire interopérabilité et ratios. | `low`, confirmé ; `patch`. | Commande de l'oracle indépendant documentée ; son JSON contient les tailles et empreintes, régénération des références explicite et conditionnée au succès. |
| V1 | Vérification | Les allers-retours internes seuls peuvent laisser passer une erreur commune aux deux codecs en CI. | `medium`, confirmé ; `patch`. | Ajout de seize sorties connues validées par zlib indépendant et comparées exactement par un test Rust dans la CI normale. Python reste un outil d'agent selon D11. |
| R1 | Coordinateur | Le test aléatoire de transactions repartait d'un état vide après chaque annulation, sans suppression ni vraie séquence. | `medium`, confirmé ; `patch`. | Test renforcé : arbre peuplé, 200 transactions de 3–6 opérations, Insert/Remove, codec, modèle Vec, ordre/valeurs/IDs, compteur monotone et non-réutilisation après annulation. |

La revue des cas limites a rendu une liste vide. Aucun constat différé ni
décision restante pour l'utilisateur. La validation globale après correctifs
est consignée dans [next-batch-validation.json](next-batch-validation.json),
l'interopérabilité dans [compression-oracle.json](compression-oracle.json).

Les contrôles préalables du coordinateur ont aussi corrigé les arbres Huffman
incomplets non autorisés, une comparaison de distance tronquée sur WASM32,
les réservations ATDL à partir de compteurs forgés et la profondeur des piles
pendant le décodage. Les régressions correspondantes sont testées. Les alertes
Clippy du premier contrôle global ont été corrigées avant la validation finale.
