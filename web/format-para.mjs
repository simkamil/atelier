// SPDX-License-Identifier: AGPL-3.0-or-later
// Mise en forme des paragraphes (US-243) : coquille vide posée par la tranche de préparation. US-243 écrit ICI (et dans
// format-para.css, tests/rich-editor-format-para.mjs, rich_editor/format_para*.rs) et nulle part ailleurs côté web :
// ni editor-shell.mjs, ni app.mjs, ni rich-editor.mjs, ni index.html ne changent.
//
// Entrée : createParaFormat({ document, i18n, controller, announce })
//   document   : le document DOM (création des éléments)
//   i18n       : `i18n.t(key, values)` (catalogue) ; les clés de la story vont dans le bloc « # US-243 » de web/i18n/fr-FR.txt
//   controller : createFormatController (web/format-controller.mjs) ; `controller.subscribe(({ state, readOnly,
//                plainText, busy }) => …)` donne l'état du paragraphe (UN FormatState par changement), `controller.run(client =>
//                client.applyParaFormat({ edits }))` exécute une commande, relit l'état et rend le focus au document ;
//                `canFormat(snapshot)` (même fichier) dit si une commande peut partir
//   announce   : `announce(texte, isError)` écrit dans la région d'état de l'éditeur
// Sortie : { groups, setBlocked, keyBindings, dispose }
//   groups({ command, future }) -> tableau d'entrées du groupe « Mise en forme » de l'onglet Accueil, après celles des caractères.
//     command(id, labelKey, keyTip, element?) -> entrée active ; `future(id, labelKey, keyTip)` -> bouton désactivé.
//     Entrée personnalisée (listes d'interligne, retraits, bordures) : créer l'élément (un `button`, ou un conteneur
//     `div` portant `<select>`, `<input>`…) et le passer en `element` ; le ruban lui ajoute la classe `ribbon-command`
//     et un `data-ribbon-command-id`, mais ne lui donne NI libellé NI `aria-label` : c'est au module de nommer ses
//     contrôles (aria-label, `title`), de gérer `disabled` et de rendre le focus au document par `controller.run`. Le
//     ruban prend le premier descendant focalisable pour la navigation au clavier et les astuces. Un élément ne peut
//     figurer qu'une fois. Astuces (keyTip, un caractère) déjà prises dans l'onglet Accueil : U R E C H (historique,
//     édition) et P (module Caractères) ; ici G est la seule utilisée aujourd'hui.
//   setBlocked(bool) : appelé par app.mjs à chaque changement d'état de l'application (aucun document, occupé) ; en plus de
//     `snapshot.busy`, désactiver tous les contrôles quand il vaut vrai.
//   keyBindings() -> [{ key | code, ctrl, shift, alt, run(event) }] ; enregistrés par app.mjs auprès de
//     createFormatKeyRouter (format-controller.mjs). Prévus : Ctrl+L, Ctrl+E, Ctrl+R, Ctrl+J, Ctrl+M…
//     Conflits : aucun avec les raccourcis existants (liste RESERVED_SHORTCUTS du contrôleur : Ctrl+N/O/S, Ctrl+F/H,
//     Ctrl+A/Z/Y, Ctrl+Maj+Espace, Ctrl+Home/End, Ctrl+flèches). Ctrl+H (remplacer) et Ctrl+F (rechercher) restent à la
//     recherche : ne pas les réutiliser. Une combinaison déjà prise lève une erreur à la création du routeur.
//   dispose() : retire les écouteurs et le DOM créé.

export function createParaFormat({ document, i18n, controller, announce } = {}) {
  void document; void i18n; void controller; void announce;
  let blocked = false;
  return Object.freeze({
    groups({ future }) { return [future("paragraph", "ribbon-paragraph", "G")]; },
    setBlocked(value) { blocked = Boolean(value); void blocked; },
    keyBindings() { return []; },
    dispose() {},
  });
}
