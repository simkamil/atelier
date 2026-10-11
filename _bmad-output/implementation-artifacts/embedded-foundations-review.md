# Revue du lot ressources, fichiers et texte brut — 2026-10-11

Trois agents frais GPT-6 Luna moyen lancés ensemble, en lecture seule : blind hunter, edge-case hunter et verification-gap. Diff exhaustif suivi/non suivi de 13 069 972 octets, SHA-256 `ab56afbbf4ec372d0dec81bda5e1a0f02e9fb999f0ab1d1464ab0e49ee6ce4cc`, baseline `e901bde9`. Tous les résultats ont été collectés avant triage.

Chaque constat est jugé avant regroupement. Les verdicts ci-dessous sont ceux du coordinateur, pas ceux des agents.

| ID | Verdict | Surface | Preuve et décision | Route |
|---|---|---|---|---|
| B1 | low | refresh-graph.py | Le tuple est bien une régression de cette modification, dans la branche de normalisation SDK. Correction directe du Path unique. | patch |
| B2 | medium | files.rs read | Une substitution régulière entre identité par chemin et ouverture passe les anciens contrôles. Comparaison ajoutée sur le handle ouvert avant lecture, test de substitution. | patch |
| B3 | low | files.rs parents | La course avec remplacement hostile de parent existe. Le broker natif de confiance ne constitue pas une sandbox contre un processus local de même privilège ; cette limite est explicitement documentée. Le correctif demanderait une nouvelle politique de parcours/handles, au-delà d’une correction directe, pour un scénario absent de l’usage courant. | rejet low complexe; limite conservée |
| B4 | low | files.rs ReplaceFileW | Une substitution extérieure dans la dernière fenêtre check→ReplaceFileW est possible. Le chemin reste celui explicitement choisi ; aucune garantie CAS n’est annoncée. Fermer cette fenêtre nécessiterait une nouvelle coordination inter-processus. Scénario rare dans cette fenêtre précise, correction complexe rejetée ; limite documentée. | rejet low complexe; limite conservée |
| B5 | medium | files.rs opened handle | Même défaut vérifié que B2 : OPEN_REPARSE_POINT seul ne détecte pas la substitution par un autre fichier régulier. Corrigé avec B2. | patch |
| B6 | low | web/index.html | Le texte statique affirme une vérification sans exécution de contrôle JS. Le libellé devient neutre Origine embarquée. | patch |
| B7 | low | web/i18n/fr-FR.txt | Opérationnel dépasse la preuve de transport par écho. Les libellés explicitent désormais écho/transport. | patch |
| B8 | false | README.md / docs/us-113-file-access.md | Le README renvoie au contrat qui décrit grant_read/grant_write/revoke et les types opaques. L’intégration et le cycle JS sont explicitement exclus de l’intention de cette tranche ; aucune API JS fichiers n’est annoncée. | rejet |
| B9 | false | docs/us-120-plain-text-model.md | L’intention porte sur le propriétaire isolé et exclut explicitement l’intégration UI/core-wasm. Les raccordements restants sont déjà nommés ; leur absence ne constitue pas une régression de cette API. | rejet |
| B10 | medium | win32.rs self-check | La matrice pure classifiait HEAD/404/405, mais aucun test du binding réel ne vérifiait ces réponses. Ajout de sondes runtime avec body et Content-Length HEAD vérifiés. | patch |
| G1 | medium | file_dialog.rs Show | Constat de vérification reçu : le helper d’annulation testé n’était pas appelé par Show. Interprétation HRESULT commune utilisée en production, testée pour annulation/succès/erreur ; dialogue manuel reste ouvert. | patch |
| E1 | medium | webview-resources.rs navigation | Le getter retournait un HRESULT sans appeler put_Cancel. Le refus par défaut doit précéder une erreur de lecture URI ; test de vtable en échec. Le setter de refus peut lui-même échouer : erreur remontée, aucune garantie absolue inventée. | patch |
| E2 | medium | webview-resources.rs new window | Le getter pouvait retourner avant put_Handled, laissant le refus non établi. Même politique de refus initial et test vtable. | patch |
| E3 | medium | files.rs opened handle | Substitution régulière vérifiée dans le chemin de lecture ; identique B2/B5, comparaison de handle appliquée. | patch |
| E4 | low | files.rs ReplaceFileW | Même fenêtre extérieure très courte que B4, déjà documentée comme contrôle au mieux et non CAS. Nouvelle politique de coordination complexe pour un scénario rare ; pas de promesse d’isolation contre un autre processus local. | rejet low complexe; limite conservée |
| E5 | low | refresh-graph.py | Même branche tuple erronée que B1, corrigée directement. | patch |
| E6 | medium | webview-resources.rs navigation claim | La trace et le refus manquaient en cas d’échec de getter ; même défaut E1, test et trace non classifiée ajoutés. | patch |
| E7 | medium | files.rs read claim | L’identité initiale seule ne prouvait pas celle du handle lu ; même défaut B2/E3, corrigé avant lecture. | patch |

Après triage individuel : B1/E5 (outil Graphify), B2/B5/E3/E7 (identité du handle), E1/E6 (navigation), E2 (nouvelle fenêtre), B6 (origine), B7 (écho), B10 (HTTP réel), G1 (annulation). Corrections envoyées aux agents de développement d’origine ; outil corrigé par le coordinateur. Aucun constat différé ajouté. Les limites OS de concurrence extérieure restent visibles dans le contrat et les US complètes restent ouvertes.

La validation avant revue a réussi 37 contrôles, 348 tests workspace et 45 natifs release sur sources/lock inchangés (`embedded-foundations-pre-review-validation.json`). Une nouvelle validation globale suit les corrections. Les sondes initiales comportaient un AV dû au slot ExecuteScript erroné ; ce défaut a été corrigé avant revue, puis le diagnostic répété. L’échec strict de handles US-111 est conservé séparément.

Contrôles complémentaires du coordinateur : le relais accepte aussi les directives effectives script-src-elem/attr et style-src-elem/attr ; deux sondes natives supplémentaires prouvent scripts et styles externes bloqués. Les erreurs de getter de requête produisent une réponse 403 quand l’API de réponse fonctionne, avec test vtable. Les erreurs d’une API de refus ne sont pas présentées comme une garantie de blocage réussi.

Validation après corrections : les 37 contrôles passent, avec 351 tests workspace et 48 tests natifs release, sur sources et lock inchangés (`embedded-foundations-validation.json`). La première relance après revue avait échoué uniquement à Clippy sur trois `vec!` de fixtures de vtables ; cette sortie reste conservée dans `embedded-foundations-clippy-failure.json`. Les fixtures utilisent désormais des tableaux, puis toute la validation a été relancée avec succès. La copie finale de `atelier.exe`, seule dans son dossier sans `web` ni `data`, charge les onze ressources et passe les sondes Worker/WASM, HTTP et CSP (`embedded-foundations-portable.json`). Les critères des US complètes explicitement ouverts ne sont pas déclarés validés par ces contrôles.
