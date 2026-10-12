// SPDX-License-Identifier: AGPL-3.0-or-later
//! Ordered XML tree (mixed content) built from the bounded streaming reader,
//! with ODF namespace constants and a small serializer for opaque fragments.

use atelier_xml::{Event, Limits, Reader};

pub(crate) const NS_OFFICE: &str = "urn:oasis:names:tc:opendocument:xmlns:office:1.0";
pub(crate) const NS_STYLE: &str = "urn:oasis:names:tc:opendocument:xmlns:style:1.0";
pub(crate) const NS_TEXT: &str = "urn:oasis:names:tc:opendocument:xmlns:text:1.0";
pub(crate) const NS_TABLE: &str = "urn:oasis:names:tc:opendocument:xmlns:table:1.0";
pub(crate) const NS_DRAW: &str = "urn:oasis:names:tc:opendocument:xmlns:drawing:1.0";
pub(crate) const NS_FO: &str = "urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0";
pub(crate) const NS_SVG: &str = "urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0";
pub(crate) const NS_META: &str = "urn:oasis:names:tc:opendocument:xmlns:meta:1.0";
pub(crate) const NS_MANIFEST: &str = "urn:oasis:names:tc:opendocument:xmlns:manifest:1.0";
pub(crate) const NS_XLINK: &str = "http://www.w3.org/1999/xlink";
pub(crate) const NS_DC: &str = "http://purl.org/dc/elements/1.1/";
pub(crate) const NS_XML: &str = "http://www.w3.org/XML/1998/namespace";

/// One attribute with its expanded name.
#[derive(Clone, Debug)]
pub(crate) struct Attr {
    pub ns: String,
    pub local: String,
    pub prefix: Option<String>,
    pub value: String,
}

#[derive(Clone, Debug)]
pub(crate) enum Child {
    Text(String),
    Elem(XNode),
}

#[derive(Clone, Debug)]
pub(crate) struct XNode {
    pub ns: String,
    pub local: String,
    pub prefix: Option<String>,
    pub attrs: Vec<Attr>,
    pub children: Vec<Child>,
}

impl XNode {
    pub fn is(&self, ns: &str, local: &str) -> bool {
        self.local == local && self.ns == ns
    }
    pub fn attr(&self, ns: &str, local: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|a| a.local == local && a.ns == ns)
            .map(|a| a.value.as_str())
    }
    pub fn elements(&self) -> impl Iterator<Item = &XNode> {
        self.children.iter().filter_map(|c| match c {
            Child::Elem(e) => Some(e),
            Child::Text(_) => None,
        })
    }
    pub fn find(&self, ns: &str, local: &str) -> Option<&XNode> {
        self.elements().find(|e| e.is(ns, local))
    }
    /// Concatenated character data of the whole subtree, in document order.
    pub fn text_content(&self) -> String {
        let mut out = String::new();
        self.collect_text(&mut out);
        out
    }
    fn collect_text(&self, out: &mut String) {
        for c in &self.children {
            match c {
                Child::Text(t) => out.push_str(t),
                Child::Elem(e) => e.collect_text(out),
            }
        }
    }
    /// `prefix:local` for diagnostics.
    pub fn qualified(&self) -> String {
        match &self.prefix {
            Some(p) => format!("{p}:{}", self.local),
            None => self.local.clone(),
        }
    }
}

/// Parse a whole part into a tree. Errors carry the reader message.
pub(crate) fn parse(bytes: &[u8], limits: &Limits) -> Result<XNode, String> {
    let mut reader = Reader::with_limits(bytes, limits.clone());
    let mut stack: Vec<XNode> = Vec::new();
    let mut root: Option<XNode> = None;
    loop {
        let event = reader.next_event().map_err(|e| e.to_string())?;
        let Some(event) = event else { break };
        match event {
            Event::StartElement {
                name,
                attributes,
                empty,
            } => {
                let node = XNode {
                    ns: name.namespace.clone().unwrap_or_default(),
                    local: name.local.clone(),
                    prefix: name.prefix.clone(),
                    attrs: attributes
                        .into_iter()
                        .map(|a| Attr {
                            ns: a.name.namespace.unwrap_or_default(),
                            local: a.name.local,
                            prefix: a.name.prefix,
                            value: a.value,
                        })
                        .collect(),
                    children: Vec::new(),
                };
                if empty {
                    attach(node, &mut stack, &mut root)?;
                } else {
                    stack.push(node);
                }
            }
            Event::EndElement { .. } => {
                let node = stack.pop().ok_or("unbalanced end element")?;
                attach(node, &mut stack, &mut root)?;
            }
            Event::Text(text) => {
                if let Some(parent) = stack.last_mut() {
                    match parent.children.last_mut() {
                        Some(Child::Text(previous)) => previous.push_str(&text),
                        _ => parent.children.push(Child::Text(text)),
                    }
                }
            }
            Event::Comment(_) | Event::ProcessingInstruction { .. } => {}
        }
    }
    if !stack.is_empty() {
        return Err("document ends inside an element".into());
    }
    root.ok_or_else(|| "document has no root element".into())
}

fn attach(node: XNode, stack: &mut [XNode], root: &mut Option<XNode>) -> Result<(), String> {
    if let Some(parent) = stack.last_mut() {
        parent.children.push(Child::Elem(node));
    } else if root.replace(node).is_some() {
        return Err("multiple root elements".into());
    }
    Ok(())
}

fn escape(out: &mut String, text: &str, attribute: bool) {
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' if attribute => out.push_str("&quot;"),
            '\n' if attribute => out.push_str("&#10;"),
            '\t' if attribute => out.push_str("&#9;"),
            '\r' if attribute => out.push_str("&#13;"),
            c => out.push(c),
        }
    }
}

/// Serialize a subtree as a self-contained XML fragment: every namespace used
/// by the subtree is declared on its root element.
pub(crate) fn serialize(node: &XNode) -> Vec<u8> {
    let mut decls: Vec<(String, String)> = Vec::new();
    collect_decls(node, &mut decls);
    let mut out = String::new();
    write_node(node, &decls, true, &mut out);
    out.into_bytes()
}

fn collect_decls(node: &XNode, decls: &mut Vec<(String, String)>) {
    let mut note = |prefix: &Option<String>, ns: &str| {
        if ns.is_empty() || ns == NS_XML {
            return;
        }
        let p = prefix.clone().unwrap_or_default();
        if !decls.iter().any(|(q, _)| *q == p) {
            decls.push((p, ns.to_owned()));
        }
    };
    note(&node.prefix, &node.ns);
    for a in &node.attrs {
        if a.prefix.is_some() {
            note(&a.prefix, &a.ns);
        }
    }
    for e in node.elements() {
        collect_decls(e, decls);
    }
}

fn write_node(node: &XNode, decls: &[(String, String)], root: bool, out: &mut String) {
    let name = node.qualified();
    out.push('<');
    out.push_str(&name);
    if root {
        for (prefix, ns) in decls {
            if prefix.is_empty() {
                out.push_str(" xmlns=\"");
            } else {
                out.push_str(" xmlns:");
                out.push_str(prefix);
                out.push_str("=\"");
            }
            escape(out, ns, true);
            out.push('"');
        }
    }
    for a in &node.attrs {
        out.push(' ');
        if let Some(p) = &a.prefix {
            out.push_str(p);
            out.push(':');
        }
        out.push_str(&a.local);
        out.push_str("=\"");
        escape(out, &a.value, true);
        out.push('"');
    }
    if node.children.is_empty() {
        out.push_str("/>");
        return;
    }
    out.push('>');
    for c in &node.children {
        match c {
            Child::Text(t) => escape(out, t, false),
            Child::Elem(e) => write_node(e, decls, false, out),
        }
    }
    out.push_str("</");
    out.push_str(&name);
    out.push('>');
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mixed_content_is_ordered_and_serialization_reparses() {
        let xml = br#"<a:r xmlns:a="urn:x" a:k="v&amp;1">one<a:b/>two &lt;3</a:r>"#;
        let node = parse(xml, &Limits::default()).unwrap();
        assert_eq!(node.children.len(), 3);
        assert_eq!(node.text_content(), "onetwo <3");
        let again = parse(&serialize(&node), &Limits::default()).unwrap();
        assert_eq!(again.text_content(), "onetwo <3");
        assert_eq!(again.attr("urn:x", "k"), Some("v&1"));
    }
}
