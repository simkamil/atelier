# Sous-arbres XML opaques (US-032)

`Reader::capture_opaque()` capture le dernier événement `StartElement` retourné par ce lecteur et consomme son contenu jusqu'à la fermeture correspondante. La capture est à usage unique et doit être appelée avant `next_event()` : aucune capture n'est possible depuis un événement construit par l'appelant ou depuis un start périmé. Un start vide produit immédiatement un sous-arbre et ne consomme pas l'événement suivant. Le lecteur conserve temporairement le scope namespace d'un start vide jusqu'à la capture ou au prochain appel à `next_event()`.

`Writer::write_opaque(&OpaqueSubtree)` réémet le sous-arbre dans un autre contexte. `OpaqueSubtree` n'expose pas ses champs et ne peut donc pas servir à injecter des événements ou du texte XML fabriqués. Les noms, attributs et constructions XML passent par la validation du lecteur; le writer rééchappe les valeurs et écrit des déclarations namespace locales. Les préfixes enregistrés sont maintenus, y compris les mappings hérités, masqués et réinitialisés. Le scope complet du start racine est conservé afin de préserver les préfixes employés uniquement dans des valeurs d'attribut ou de texte de forme QName. Si la cible a un namespace par défaut alors que la capture n'en avait pas, le writer émet `xmlns=""` à la racine.

Ici, « brut » signifie une fidélité sémantique : noms développés, préfixes effectifs, valeurs et ordre des événements, commentaires et instructions de traitement. Cela ne promet pas les octets d'origine, l'encodage, les espaces entre balises, les entités lexicales ni la forme vide/pleine. Les normalisations documentées dans [US-030/031](us-030-031-xml.md) restent applicables. Les limites du Reader gouvernent la capture; un dépassement ou une erreur de syntaxe/encodage invalide la capture en cours et ne retourne aucun sous-arbre partiel. Les DTD restent ignorées, sans expansion ni accès externe.

Exemple :

```rust
use atelier_xml::{Reader, Writer};

let mut reader = Reader::new(b"<r xmlns:p='urn:kind' xmlns:xsi='urn:xsi'><x xsi:type='p:Widget'/></r>".as_slice());
reader.next_event()?; // r
reader.next_event()?; // x
let opaque = reader.capture_opaque()?; // x et son scope effectif
let mut writer = Writer::new(Vec::new())?;
writer.write_opaque(&opaque)?;
let xml = writer.finish()?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Validation — 2026-10-10

US développée, revue et validée. Le [suivi du lot](EDITING-PROGRESS.md) relie les tests, les comparaisons indépendantes, les correctifs de revue et leurs limites.
