# US-202 — Document riche par défaut

`atelier_text_model::rich::default_rich_document()` retourne un `DefaultRichDocument` autonome avec l’arbre riche, un `StyleEngine`, les métadonnées de présentation et deux tables indexées par identifiant stable : `style_names` contient les noms français, `next_styles` les identifiants de style suivant. Les styles utilisent des identifiants `atelier.*` propres à Atelier; les choix de mise en forme sont ceux de ce profil et ne prétendent pas reproduire une version précise de Word.

Le document contient une section et un paragraphe vide référant au style Normal. Le moteur résout immédiatement ce paragraphe en Carlito 11 pt (22 demi-points), avec 8 pt après (160 twips). La page est A4 portrait (21000 × 29700 centièmes de millimètre), avec 2500 centièmes de millimètre de marge sur les quatre côtés et la langue `fr-FR`.

L’interligne exact demandé, 108/100, est conservé dans `presentation`. `ParaProps` ne représentant l’interligne automatique qu’en 240èmes entiers, le modèle de styles utilise 259/240 (arrondi au plus proche), soit environ 1,0792. Cette valeur approchée ne doit pas être décrite comme exactement 1,08. Le style Sans interligne utilise Auto 240 et 0 twip après.

```rust
use atelier_text_model::rich::default_rich_document;

let mut profile = default_rich_document();
let paragraph = profile.document.paragraph_ids()[0];
let resolved = profile.styles.resolve(
    &profile.document,
    paragraph,
    None,
    &Default::default(),
)?;
assert_eq!(resolved.character.size_half_points, Some(22));
# Ok::<(), atelier_text_model::rich::StyleError>(())
```

Chaque appel construit ses propres arbre, catalogue et moteur. Le modèle `Section` ne porte pas de géométrie typée et l’API `Style` ne contient pas de champ « style suivant »; ces données restent donc dans les métadonnées de la fabrique, sans attribut opaque artificiel ni modification des structures publiques existantes.

Cette fabrique rend le profil disponible aux appelants Rust. Elle ne raccorde pas le bouton Nouveau à l’éditeur riche; US-202 reste partielle jusqu’à cette intégration.
