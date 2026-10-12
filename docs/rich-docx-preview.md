# Aperçu DOCX dans le MVP

Le bouton **Ouvrir DOCX…** ouvre un document Word dans un aperçu en lecture seule. **Retour au texte** ou Échap retrouve le document TXT conservé, avec sa sélection, son historique et son état de sauvegarde. Nouveau, ouvrir TXT, enregistrer et les commandes d'édition restent bloqués pendant l'ouverture et l'aperçu.

Cette tranche connecte les API des US-301 et US-210 au produit ; ces US restent partielles. Il n'y a pas encore d'édition ni d'enregistrement DOCX. Les pages sont composées en A4 portrait avec marges de 25 mm. Les styles de texte pris en charge (polices Carlito ou incorporées, tailles, couleurs, gras/italique et soulignement) sont rendus en contours. Les tableaux, images, en-têtes et pieds de page, la géométrie des sections Word, les puces et numéros de listes, liens interactifs et contrôles de pagination ne sont pas reproduits fidèlement. La vue annonce cette mise en page simplifiée.

L'ouverture est limitée à 8 Mio de fichier, 32 Mio décompressés, 128 Kio de texte, 1 024 paragraphes, 256 pages, 65 536 glyphes et 26 Mio d'ATDL. Les polices incorporées sont limitées à 16 faces et 8 Mio. Les limites TXT restent inchangées. L'annulation ou une erreur laisse le TXT intact ; une réponse tardive après fermeture est ignorée.

Le dialogue natif accorde une capacité de lecture temporaire, révoquée après usage. Le JavaScript reçoit les octets sans chemin de fichier. La commande ATED 18 retourne une enveloppe ATRP version 1 : nombre de pages, longueur du texte UTF-8, nombre d'avertissements, texte puis pages ATDL précédées de leur longueur. Le décodage est strict ; cette commande ne publie aucun état TXT.

Le lot est développé avec trois agents GPT-6 Luna moyen selon la [coordination documentée](SUBAGENT-WORKFLOW.md). Les preuves de validation et les limites restantes sont enregistrées dans `_bmad-output/implementation-artifacts/rich-docx-preview-*`. La comparaison à Word, les performances sur la machine de référence et les essais manuels avec lecteur d'écran restent ouverts.

Validation finale : 24 contrôles, 760 tests Rust Windows et 111 assertions navigateur ; inventaire de 856 fichiers inchangé. Revue : 11 remarques triées, corrections fonctionnelles appliquées, aucun défaut bloquant restant. La CI distante est vérifiée séparément après publication.
