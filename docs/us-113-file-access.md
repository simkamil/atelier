# US-113 — Backend d’accès fichiers natif

## API de cette tranche

`lib.rs` enregistre `pub mod files;` et, sous `#[cfg(windows)]`, `pub mod file_dialog;`. La façade de dialogue est `file_dialog::StaFileDialogs::initialize_current_thread()`. Elle appartient au thread UI STA, n’est ni `Send` ni `Sync`, et est détruite sur ce même thread. Chaque appel `open`/`save` exige un `Option<&DialogOwner>` explicite. La création d’un `DialogOwner` depuis un HWND brut est `unsafe` et documente l’obligation de garder la fenêtre valide pendant le dialogue.

Les filtres sont créés avec `FileFilter::new(label, extensions)`. Les libellés et motifs UTF-16 restent en mémoire jusqu’au retour COM. Ouverture impose un système de fichiers, un dossier existant et un fichier existant. Enregistrement impose un dossier existant, confirme l’écrasement et indique une extension par défaut. `Ok(None)` signifie annulation; toute autre erreur est `FileError` avec `code`, `message_key`, HRESULT et/ou code Windows disponibles. Aucun chemin n’est inclus dans le message affichable.

Le dialogue retourne un `PathBuf` au code natif de confiance. Le coordinateur accorde ensuite l’accès avec `FileBroker::grant_read` ou `grant_write`, transmet le ticket opaque et garde le broker côté natif. JavaScript ne reçoit aucun chemin. Les capacités lecture/écriture sont des types distincts et se révoquent avec `revoke_read`/`revoke_write`. Les IDs sont globaux au processus et monotones, empêchant un ticket d’un broker d’adresser un grant d’un autre broker; l’épuisement échoue plutôt que de réutiliser un ID. Les écritures d’un même broker sont sérialisées pour garder publication et actualisation d’identité ordonnées.

`FileBroker::read` exige une limite par ticket, plafonnée à 1 Gio. Il refuse les chemins relatifs, les répertoires, les liens/reparse points, vérifie la taille issue du handle et lit au plus `limite + 1` octets afin de détecter la croissance après metadata. Les octets sont rendus tels quels, sans décodage UTF-8.

## Publication atomique et limites

Une sauvegarde crée son temporaire avec `create_new` dans le dossier de destination, écrit tous les octets, appelle `sync_all`, puis publie. Sous Windows, une destination existante passe par `ReplaceFileW`; une destination nouvelle passe par `MoveFileW` sur le même volume, qui ne remplace pas un fichier créé entre-temps. Sur les hôtes de test non Windows, le remplacement utilise `rename` et la nouvelle destination est publiée avec un lien dur sans remplacement. Chaque erreur garde son étape et supprime uniquement le temporaire dont cet appel a obtenu la création exclusive.

Le ticket enregistre l’état de la destination à l’autorisation. Si un fichier existant disparaît, change d’identité, ou est remplacé par un répertoire ou un reparse point avant publication, l’opération échoue et le temporaire est retiré. Pour une destination nouvelle, toute apparition concurrente provoque l’échec de `MoveFileW`; le fichier concurrent reste intact. Après réussite, le même ticket adopte l’identité du nouveau fichier, ce qui permet une autre sauvegarde autorisée. Les répertoires parents doivent déjà exister et les composants de chemin observés comme reparse points sont refusés. Pour une destination existante, la vérification d’identité juste avant `ReplaceFileW` est une vérification au mieux: elle ne constitue pas un compare-and-swap atomique et ne ferme pas la dernière fenêtre de course entre vérification et remplacement. Cette tranche ne prétend pas fournir une isolation contre un processus local hostile qui remplace simultanément un parent.

Les tests d’écriture utilisent des répertoires uniques créés sous `std::env::temp_dir()` et vérifient qu’ils restent sous cette racine. Aucun document utilisateur ni dialogue interactif n’est utilisé. Les cases COM sont vérifiées avec la déclaration SDK locale `C:\Program Files (x86)\Windows Kits\10\Include\10.0.22621.0\um\ShObjIdl_core.h`: `IFileDialog::Show` hérite de `IModalWindow` (slot 3), `SetFileTypes` slot 4, `GetOptions` slot 10, `GetResult` slot 20, `SetDefaultExtension` slot 22; `IShellItem::GetDisplayName` est slot 5. Les IID proviennent des UUID déclarés dans ce même header (`IFileOpenDialog`, `IFileSaveDialog`, `IShellItem`). Ces interfaces sont possédées par RAII; `CoInitializeEx`/`CoUninitialize` sont équilibrés.

## Validation et intégration restante

Les modules sont enregistrés dans `lib.rs`. La validation ciblée exécutée pour cette tranche comprend `cargo test -p atelier-desktop-win files::tests --lib` et `cargo test -p atelier-desktop-win file_dialog::tests --lib`; ces tests n’ouvrent aucune fenêtre. Le raccordement des tickets au pont natif/JavaScript et le transfert SharedBuffer (100 Mo/300 ms) ne sont pas validés ici. Les polices système, les dialogues manuels sous Windows, la traduction UI des `message_key` et les validations utilisateur de l’US-113 complète restent à réaliser.

Le lot ressources, fichiers et texte brut a été développé en parallèle avec trois
agents GPT-6 Luna moyen, puis corrigé après trois revues indépendantes.
La [validation finale](../_bmad-output/implementation-artifacts/embedded-foundations-validation.json)
réussit **37 contrôles, 351 tests workspace et 48 tests natifs release**,
sur sources et lock inchangés. Les diagnostics debug/release chargent les onze
ressources, vérifient le vrai Worker/WASM par écho Ping, HEAD/404/405 et six
catégories de refus CSP avec traces natives. La [copie isolée de l’exécutable](../_bmad-output/implementation-artifacts/embedded-foundations-portable.json)
fonctionne sans dossier web/data. Les [18 constats de revue](../_bmad-output/implementation-artifacts/embedded-foundations-review.md)
sont triés ; les corrections retenues sont appliquées, aucun nouveau constat
n’est différé. Les [échecs exploratoires](../_bmad-output/implementation-artifacts/embedded-foundations-exploration.json)
restent conservés.

US-112 reste ouverte pour la journalisation des violations CSP internes aux
Workers et les scénarios service-worker. US-113 reste ouverte pour le pont
fichiers JS, SharedBuffer, les polices/URL et les essais de dialogues natifs.
US-120 reste ouverte pour la mise en page et la connexion saisie/modèle/rendu,
ainsi que ses mesures de latence et démarrage. US-111 conserve l’échec strict
de handles ; ses contrôles fonctionnels de 100 cycles et callbacks passent.
Le backlog reste à **35 US entièrement validées sur 158**. Les tests musl sont
compilés localement ; leur exécution appartient à la CI Linux du commit publié.

## Validation du premier éditeur intégré — 2026-10-11

Trois agents GPT-6 Luna moyen ont développé en parallèle le moteur, l’interface
et le pont fichiers, puis corrigé le lot après trois revues indépendantes.
La [validation finale](../_bmad-output/implementation-artifacts/editor-integration-validation.json)
réussit **40 contrôles, 368 tests workspace et 55 tests natifs release**,
avec sources et lock inchangés. Le navigateur utilise le vrai Worker/WASM/Canvas
et un Host factice : saisie/sélection, pagination, composition, annulations,
sauvegarde obsolète, raccourcis de la vraie page et miroir textuel accessible.
L’ABI réelle vérifie aussi le rollback d’un candidat lorsque le registre de
128 Mio ne peut réserver sa réponse.

Les diagnostics WebView2 debug/release chargent les **17 ressources** et vérifient
saisie accentuée, pixels de glyphes, undo/redo, HTTP et six catégories CSP.
La [copie isolée](../_bmad-output/implementation-artifacts/editor-integration-portable.json)
fonctionne sans dossier web/data. Les tests de routage natif injectent des résultats
de dialogue contrôlés et passent par le broker réel ; ils n’ouvrent aucun dialogue.
La [revue](../_bmad-output/implementation-artifacts/editor-integration-review.md)
conserve les 12 constats séparément : cinq corrections retenues et trois fonctions
V1 reportées (presse-papiers, navigation par mot, colonne préférée), avec les rejets
motivés. Les limites de wrapping et de Tab sont également décrites dans le contrat UI.

Le premier parcours TXT est testable avec `cargo run -p atelier-desktop-win --release --locked --offline`. La tranche reste limitée à
128 Kio, latin gauche à droite et mise en page complète à chaque modification.
Les dialogues manuels, AZERTY/IME physique et mesures de 300 pages/démarrage restent
ouverts pour US-120. US-113 conserve SharedBuffer, polices/URL ; US-111 conserve
l’échec strict de handles. Les **35/158 US complètes** sont inchangées. Les tests
musl sont compilés localement ; leur exécution appartient à la CI Linux publiée.
