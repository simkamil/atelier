# Référence de lignes physiques avec Word

`tools/word-line-reference.ps1` fournit un corpus déterministe et un collecteur Microsoft Word COM pour US-210. Il ne lit que le DOCX synthétique qu’il vient de créer dans `tests/text-layout-reference/generated/`. Word est lancé invisible, les alertes sont désactivées, puis l’instance est fermée.

Sur Windows avec Word desktop installé, exécuter depuis la racine du dépôt :

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\word-line-reference.ps1 -Mode Capture
```

Le DOCX local est `tests/text-layout-reference/generated/us-210-synthetic-corpus.docx`. La preuve JSON est écrite dans `_bmad-output/implementation-artifacts/us-210-word-reference.json`. Le corpus contient des paragraphes français et anglais avec alignements gauche, centré, droite et justifié, retraits gauche/droit/première ligne/suspendu, interlignes simple, 1,5, double et exact, ainsi que des polices et tailles mixtes, des accents, des guillemets, un mot long et une marque combinante.

Le collecteur relève les fins de lignes avec `Range.MoveEnd(wdLine, 1)` sur des plages Word après `Document.Repaginate()`. Chaque ligne a ses positions locales en unités UTF-16 et leurs équivalents en octets UTF-8, ainsi que les positions UTF-16 dans le document, le numéro de page, et les coordonnées de la plage réduite au début de la ligne quand Word peut les fournir. Les paragraphes conservent le texte exact, les paramètres demandés et les noms/tailles de police rapportés par les plages Word. `wordDeclaredFontName` est le nom de mise en forme déclaré par COM, pas une preuve de la fonte physique après substitution. Une comparaison ne peut étayer un seuil de 95 % que si les deux chemins de rendu utilisent les mêmes fichiers de police.

Le collecteur accepte uniquement un nouveau DOCX sous `tests/text-layout-reference/generated/`. Le JSON doit être nouveau, avec l’extension `.json`, sous ce même répertoire ou sous `_bmad-output/implementation-artifacts/`. L’écriture utilise `CreateNew` pour refuser aussi un fichier apparu après la validation du chemin. Pour une nouvelle collecte, choisir de nouveaux noms avec `-SyntheticDocx` et `-OutputJson`. Le mode disponibilité n’ouvre aucun document.

Pour vérifier la disponibilité sans créer ni ouvrir de document :

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\word-line-reference.ps1 -AvailabilityOnly
```

Sans Word, le script renvoie `status: noWord`, `syntheticDocxCreated: false` et `layoutResults: null`, avec un code de sortie réussi. Aucun résultat de mise en page ni golden n’est simulé. La preuve locale d’indisponibilité actuelle est dans [`us-210-word-availability.json`](../../_bmad-output/implementation-artifacts/us-210-word-availability.json).

Le chemin `Capture` est préparatoire et n’a pas pu être exécuté ni validé sur cette machine, car Microsoft Word n’est pas installé/enregistré comme serveur COM. Une sortie de référence et le critère de 95 % ne sont donc pas établis ici.

Les frontières de lignes utilisent l’unité `wdLine` de Word via `Range.MoveEnd`, pas `wdFirstCharacterLineNumber`, qui ne fournit pas à lui seul un identifiant global de ligne physique. Les coordonnées de plage complètent les frontières; elles peuvent valoir `null` si le texte n’est pas dans la zone visible de Word.

Références Microsoft : [Range.MoveEnd method (Word)](https://learn.microsoft.com/en-us/office/vba/api/word.range.moveend), [WdInformation enumeration (Word)](https://learn.microsoft.com/en-us/office/vba/api/word.wdinformation), [Range.Information property (Word)](https://learn.microsoft.com/en-us/office/vba/api/word.range.information).
