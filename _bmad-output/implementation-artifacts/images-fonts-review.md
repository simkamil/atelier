# Revue US-040, US-043 et US-056

Diff initial : 8 759 618 octets, SHA-256 `45b9b0f575dee6738d321dde2f1f89f243fbee8ce9dd2343376674388a7df57b`. Trois revues indépendantes lancées ensemble avant tout tri.

Chaque constat est trié individuellement avant regroupement. Les corrections sont locales, sans nouvelle API produit. Les trois agents de revue sont GPT-6 Luna moyen. La revue Edge Case et la revue Verification Gap n’ont signalé aucun constat.

| ID | Constat | Verdict | Preuve et résolution |
|---|---|---|---|
| B1 | Virgule absente entre XML et font dans README | low | Constat exact sur le diff initial; correction directe de la liste. |
| B2 | Énumération des suivis avec deux et | low | Constat exact dans README; énumération corrigée directement. |
| B3 | Annonce obsolète du futur corpus images | low | tests/README annonce un futur corpus déjà livré; phrase remplacée. |
| B4 | Graphify pourrait traiter les fichiers RGBA | false | Les .rgba ne sont pas des extensions prises en charge : la CLI les classe explicitement non classifiés et ne les soumet ni à AST ni au LLM. Exclure leur extension ne corrige aucun traitement réel. |
| B5 | Comptage du corpus trop permissif | medium | valid >= 180 laisserait disparaître sept références; assert_eq!(valid,187) détecte désormais toute perte. |
| B6 | Empreintes du manifeste absentes de CI | medium | Le test de pixels ne validait pas les empreintes documentées. tests/check-assets.ps1 vérifie les 201 entrées et 187 références, l’égalité TSV/manifeste, et les 20 polices; exécuté dans le job Windows. |
| B7 | Mention des agents inutile au suivi | false | Le suivi est daté et cette information documente la préférence explicite de l’utilisateur pour GPT-6 Luna moyen. Aucun comportement ni invariant de produit ne diverge. |
| B8 | Rapport de revue exclu du diff/lien absent | false | prepare-images-fonts-review.py exclut le préfixe images-fonts-review- ; le rapport images-fonts-review.md ne correspond pas à ce motif. Il est produit après les revues et livré à ce chemin; il ne pouvait exister avant leur résultat. |
| B9 | PngSuite distribué depuis HTTP | low | Les deux hôtes HTTPS échouent à TLS; conserver le lien officiel, documenter le SHA figé et le corpus versionné sans téléchargement CI. Correction documentaire. |
| B10 | Validation de schéma annoncée trop générale | low | Commentaire remplacé par les contrôles réels; garde natif ajoutant types, valeurs, cardinalité, unicité, commits et chemins. Onze inventaires volontairement altérés doivent échouer. |
| R1 | Portée GCE après extension Plain Text | medium | Test ajouté d’abord en échec confirmé par l’agent : la transparence était héritée après un bloc graphique Plain Text. Nettoyer control après en-tête 12 octets et données ignorées; préserver contrôle après commentaire/application. Test Cargo passe. |
| R2 | Pixels sautés par RLE rendus transparents | medium | Le corpus indépendant échouait sur bmp-rle4-delta.bmp : alpha 0 aux pixels non peints. Initialisation avec palette[0] opaque; références RLE4/RLE8 et test dédié passent. |
| R3 | biSizeImage tronqué silencieusement | medium | Le min(file_end) masquait une taille déclarée excessive; garde renvoie Truncated. Régression : flux RLE réel de six octets annoncé à sept. |
| R4 | Entiers PNG dépassant 31 bits | medium | Les premières sources ne refusaient pas explicitement les champs supérieurs à 2^31-1. Guardes chunk et IHDR avant allocation; test max_pixels=u64::MAX couvre largeur, hauteur et longueur. |
| R5 | Signature de test &mut Vec inutile | low | Clippy all-targets refusait ptr_arg; set_file_size prend &mut [u8]. |
| R6 | matches!(Err(_)) redondant en test | low | Clippy all-targets refusait redundant_pattern_matching; assertion .is_err(). |

Douze constats corrigés, quatre réfutés. Aucun constat différé. La validation après corrections et les empreintes finales sont conservées dans images-fonts-validation.json, images-fonts-oracle.json et images-fonts-review-snapshot.json.
