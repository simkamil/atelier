# Références exhaustives Unicode 17.0.0

`checksums.tsv` contient trente empreintes FNV-1a-64 calculées par le parseur
Python indépendant de `check-ucd-oracle.py`, directement à partir des fichiers
UCD officiels figés. Les 29 champs de consultation sont sérialisés en UTF-8,
suivis de LF, pour chaque scalaire valide de U+0000 à U+10FFFF dans cet ordre.
Les surrogates sont exclus. La dernière empreinte porte sur les seize lignes
SpecialCasing conditionnelles, avec colonnes séparées par TAB et terminaison LF.
Les séquences utilisent l’hexadécimal majuscule sans zéro initial et la virgule ;
une valeur absente vaut `~`, une séquence vide reste vide.

Le test `crates/unicode/tests/ucd_reference.rs` recalcule ces empreintes depuis
les API publiques en CI, sans Python ni réseau. Le manifeste SHA-256 protège
le fichier de référence ; FNV sert à détecter les régressions de données,
sans prétendre être une empreinte cryptographique.

La régénération réservée aux agents exige l’archive officielle UCD 17.0.0 à
`target/prototype-oracle/UCD-17.0.0.zip`, dont le SHA-256 est vérifié contre le
manifeste UCD, puis `py -X utf8 _bmad-output/implementation-artifacts/check-ucd-oracle.py`.
Cet outil compare aussi chaque champ de chaque scalaire, sans s’appuyer seulement
sur les empreintes. Une modification du générateur ne justifie pas à elle seule
de changer ces références officielles.
