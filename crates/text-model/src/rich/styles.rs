// SPDX-License-Identifier: AGPL-3.0-or-later
//! Word-like style resolution for the rich document model (US-201).

use super::NodeId;
use super::attrs::{CharAttrs, Color, ParaProps};
use super::document::RichDocument;
use super::error::RichError;
use super::node::Node;
use super::ops::RichOp;
use atelier_model::{Transaction, TransactionError};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

/// The kind of a named style. `based_on` must point to a style of the same kind.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum StyleKind {
    #[default]
    Paragraph,
    Character,
    Table,
}

/// A font slot supplied by the document theme.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum ThemeFont {
    Major,
    Minor,
}

/// Theme color slots commonly used by Word documents.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum ThemeColor {
    Dark1,
    Light1,
    Dark2,
    Light2,
    Accent1,
    Accent2,
    Accent3,
    Accent4,
    Accent5,
    Accent6,
    Hyperlink,
    FollowedHyperlink,
}

/// Theme-aware character values. Concrete `CharAttrs` values take precedence
/// over the corresponding theme reference within the same style.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct StyleCharAttrs {
    pub values: CharAttrs,
    pub theme_font: Option<ThemeFont>,
    pub theme_color: Option<ThemeColor>,
    pub theme_highlight: Option<ThemeColor>,
}

/// A paragraph, character, or table style definition.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Style {
    pub id: String,
    pub kind: StyleKind,
    pub based_on: Option<String>,
    /// The reciprocal paragraph/character style link used by Word.
    pub linked_style: Option<String>,
    pub character: StyleCharAttrs,
    pub paragraph: ParaProps,
}

/// Document-level fallback formatting and optional default table style.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DocumentStyleDefaults {
    pub character: StyleCharAttrs,
    pub paragraph: ParaProps,
    pub table_style: Option<String>,
}

/// Resolved theme values. Missing referenced slots are reported as typed errors.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Theme {
    pub colors: BTreeMap<ThemeColor, Color>,
    pub fonts: BTreeMap<ThemeFont, String>,
}

/// Named style definitions. Mutate this through [`StyleEngine`] so caches stay coherent.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct StyleCatalog {
    styles: BTreeMap<String, Style>,
    defaults: DocumentStyleDefaults,
}

impl StyleCatalog {
    pub fn new(defaults: DocumentStyleDefaults) -> Self {
        Self {
            styles: BTreeMap::new(),
            defaults,
        }
    }

    pub fn style(&self, id: &str) -> Option<&Style> {
        self.styles.get(id)
    }

    pub fn defaults(&self) -> &DocumentStyleDefaults {
        &self.defaults
    }
}

/// A bounded, typed style-resolution failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StyleError {
    MissingStyle(String),
    InvalidStyleId,
    InvalidStyleAttribute(&'static str),
    WrongStyleKind { id: String, expected: StyleKind },
    StyleCycle(String),
    MissingThemeColor(ThemeColor),
    MissingThemeFont(ThemeFont),
    InvalidThemeFont(ThemeFont),
    Document(RichError),
}

impl From<RichError> for StyleError {
    fn from(value: RichError) -> Self {
        Self::Document(value)
    }
}

impl std::fmt::Display for StyleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingStyle(id) => write!(f, "style {id:?} does not exist"),
            Self::InvalidStyleId => f.write_str("style id must not be empty"),
            Self::InvalidStyleAttribute(message) => write!(f, "invalid style attribute: {message}"),
            Self::WrongStyleKind { id, expected } => {
                write!(f, "style {id:?} is not a {expected:?} style")
            }
            Self::StyleCycle(id) => write!(f, "style inheritance or link cycle at {id:?}"),
            Self::MissingThemeColor(slot) => write!(f, "theme color {slot:?} is not defined"),
            Self::MissingThemeFont(slot) => write!(f, "theme font {slot:?} is not defined"),
            Self::InvalidThemeFont(slot) => write!(f, "theme font {slot:?} is empty"),
            Self::Document(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for StyleError {}

/// Fully resolved properties for a paragraph/run pair.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ResolvedProperties {
    pub character: CharAttrs,
    pub paragraph: ParaProps,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct CacheKey {
    paragraph: NodeId,
    paragraph_style: Option<String>,
    character_style: Option<String>,
    table_style: Option<String>,
}

#[derive(Clone, Debug)]
struct CacheEntry {
    value: ResolvedProperties,
    dependencies: BTreeSet<String>,
}

#[derive(Clone, Copy, Debug, Default)]
struct ToggleState {
    bold: Option<bool>,
    italic: Option<bool>,
    strike: Option<bool>,
    defaults_true: [bool; 3],
}

impl ToggleState {
    fn from_defaults(values: &CharAttrs) -> Self {
        Self {
            bold: values.bold,
            italic: values.italic,
            strike: values.strike,
            defaults_true: [
                values.bold == Some(true),
                values.italic == Some(true),
                values.strike == Some(true),
            ],
        }
    }

    /// Word table styles reset toggle properties to their effective value.
    fn apply_table(&mut self, values: [Option<bool>; 3]) {
        let slots = [&mut self.bold, &mut self.italic, &mut self.strike];
        for (slot, value) in slots.into_iter().zip(values) {
            if let Some(value) = value {
                *slot = Some(value);
            }
        }
    }

    /// Character styles retain toggle semantics. Word's default-true case
    /// toggles for each effective false character-style level.
    fn apply_character(&mut self, values: [Option<bool>; 3]) {
        let slots = [&mut self.bold, &mut self.italic, &mut self.strike];
        for (index, (slot, value)) in slots.into_iter().zip(values).enumerate() {
            if let Some(value) = value {
                if self.defaults_true[index] && !value {
                    *slot = Some(!slot.unwrap_or(true));
                } else if !self.defaults_true[index] && value {
                    *slot = Some(!slot.unwrap_or(false));
                } else if slot.is_none() {
                    *slot = Some(false);
                }
            }
        }
    }

    /// Word paragraph styles reset toggle properties to their effective value.
    fn apply_paragraph(&mut self, values: [Option<bool>; 3]) {
        if let Some(value) = values[0] {
            self.bold = Some(value);
        }
        if let Some(value) = values[1] {
            self.italic = Some(value);
        }
        if let Some(value) = values[2] {
            self.strike = Some(value);
        }
    }

    fn write_to(self, attrs: &mut CharAttrs) {
        attrs.bold = self.bold;
        attrs.italic = self.italic;
        attrs.strike = self.strike;
    }
}

fn style_toggles(chain: &[&Style]) -> [Option<bool>; 3] {
    let mut values = [None, None, None];
    for style in chain {
        if style.character.values.bold.is_some() {
            values[0] = style.character.values.bold;
        }
        if style.character.values.italic.is_some() {
            values[1] = style.character.values.italic;
        }
        if style.character.values.strike.is_some() {
            values[2] = style.character.values.strike;
        }
    }
    values
}

/// Resolves style cascades and caches each paragraph/style base independently.
/// Direct character and paragraph formatting is overlaid for each call, so it
/// never leaks between runs that share a paragraph cache entry.
#[derive(Clone, Debug)]
pub struct StyleEngine {
    catalog: StyleCatalog,
    theme: Theme,
    cache: BTreeMap<CacheKey, CacheEntry>,
    order: VecDeque<CacheKey>,
    capacity: usize,
    hits: u64,
    misses: u64,
}

impl StyleEngine {
    pub fn new(catalog: StyleCatalog, theme: Theme, capacity: usize) -> Self {
        Self {
            catalog,
            theme,
            cache: BTreeMap::new(),
            order: VecDeque::new(),
            capacity,
            hits: 0,
            misses: 0,
        }
    }

    pub fn catalog(&self) -> &StyleCatalog {
        &self.catalog
    }

    /// Returns the opposite member of a paragraph/character style pair. A
    /// missing target or an invalid pair is ignored, matching WordprocessingML.
    pub fn linked_style_id(&self, id: &str) -> Option<&str> {
        let style = self.catalog.styles.get(id)?;
        let expected = match style.kind {
            StyleKind::Paragraph => StyleKind::Character,
            StyleKind::Character => StyleKind::Paragraph,
            StyleKind::Table => return None,
        };
        let linked = style.linked_style.as_deref()?;
        self.catalog
            .styles
            .get(linked)
            .filter(|candidate| candidate.kind == expected)
            .map(|_| linked)
    }
    pub fn cache_len(&self) -> usize {
        self.cache.len()
    }
    pub fn cache_hits(&self) -> u64 {
        self.hits
    }
    pub fn cache_misses(&self) -> u64 {
        self.misses
    }

    /// Adds or replaces a style after checking its local shape. References are
    /// validated during resolution, allowing styles to be loaded in any order.
    pub fn set_style(&mut self, style: Style) -> Result<(), StyleError> {
        if style.id.is_empty() {
            return Err(StyleError::InvalidStyleId);
        }
        if style.based_on.as_deref() == Some("") || style.linked_style.as_deref() == Some("") {
            return Err(StyleError::InvalidStyleId);
        }
        if style.character.values.font.as_deref() == Some("") {
            return Err(StyleError::InvalidStyleAttribute("empty font name"));
        }
        if style.character.values.size_half_points == Some(0) {
            return Err(StyleError::InvalidStyleAttribute("zero font size"));
        }
        let id = style.id.clone();
        self.catalog.styles.insert(id.clone(), style);
        self.invalidate_style(&id);
        Ok(())
    }

    pub fn remove_style(&mut self, id: &str) -> bool {
        let removed = self.catalog.styles.remove(id).is_some();
        if removed {
            self.invalidate_style(id);
        }
        removed
    }

    pub fn set_defaults(&mut self, defaults: DocumentStyleDefaults) {
        self.catalog.defaults = defaults;
        self.clear_cache();
    }

    pub fn set_theme(&mut self, theme: Theme) -> Result<(), StyleError> {
        if theme.fonts.values().any(String::is_empty) {
            let slot = theme
                .fonts
                .iter()
                .find(|(_, name)| name.is_empty())
                .map(|(slot, _)| *slot)
                .unwrap_or(ThemeFont::Major);
            return Err(StyleError::InvalidThemeFont(slot));
        }
        self.theme = theme;
        self.clear_cache();
        Ok(())
    }

    pub fn clear_cache(&mut self) {
        self.cache.clear();
        self.order.clear();
    }

    /// Resolves document defaults → containing table style → paragraph style
    /// inheritance → caller-selected run character style → direct formatting.
    /// Linked styles are pairs; callers choose the applicable partner and pass
    /// its character style ID when resolving a run.
    pub fn resolve(
        &mut self,
        document: &RichDocument,
        paragraph_id: NodeId,
        character_style: Option<&str>,
        direct_character: &CharAttrs,
    ) -> Result<ResolvedProperties, StyleError> {
        let paragraph = document.paragraph(paragraph_id)?;
        let paragraph_style = paragraph.props.style.as_deref();
        let table_style = match table_style_for_paragraph(document, paragraph_id) {
            Some(style) => style.or(self.catalog.defaults.table_style.as_deref()),
            None => None,
        }
        .map(str::to_owned);
        let key = CacheKey {
            paragraph: paragraph_id,
            paragraph_style: paragraph_style.map(str::to_owned),
            character_style: character_style.map(str::to_owned),
            table_style,
        };
        let base = if let Some(entry) = self.cache.get(&key) {
            self.hits = self.hits.saturating_add(1);
            entry.value.clone()
        } else {
            self.misses = self.misses.saturating_add(1);
            let (value, dependencies) =
                self.resolve_base(paragraph_style, character_style, key.table_style.as_deref())?;
            self.insert_cache(
                key,
                CacheEntry {
                    value: value.clone(),
                    dependencies,
                },
            );
            value
        };
        let mut resolved = base;
        let mut direct_para = paragraph.props.clone();
        direct_para.style = None;
        resolved.paragraph.overlay(&direct_para);
        resolved.character.overlay(direct_character);
        Ok(resolved)
    }

    /// Applies a rich operation transaction and invalidates only entries it
    /// could affect. Cache changes happen after successful application only.
    pub fn apply(
        &mut self,
        document: &mut RichDocument,
        transaction: &Transaction<RichOp>,
    ) -> Result<Transaction<RichOp>, TransactionError<RichError>> {
        let affected = affected_paragraphs(document, transaction);
        let inverse = document.apply(transaction)?;
        self.invalidate_paragraphs(&affected);
        Ok(inverse)
    }

    fn resolve_base(
        &self,
        paragraph_style: Option<&str>,
        character_style: Option<&str>,
        table_style: Option<&str>,
    ) -> Result<(ResolvedProperties, BTreeSet<String>), StyleError> {
        let mut output = ResolvedProperties::default();
        let mut dependencies = BTreeSet::new();
        let mut toggles = ToggleState::from_defaults(&self.catalog.defaults.character.values);
        self.apply_char(&mut output.character, &self.catalog.defaults.character)?;
        output.paragraph.overlay(&self.catalog.defaults.paragraph);
        if let Some(id) = table_style {
            let chain = self.chain(id, StyleKind::Table, &mut dependencies)?;
            for style in &chain {
                self.apply_char(&mut output.character, &style.character)?;
            }
            toggles.apply_table(style_toggles(&chain));
            for style in chain {
                output.paragraph.overlay(&style.paragraph);
            }
        }
        if let Some(id) = paragraph_style {
            let chain = self.chain(id, StyleKind::Paragraph, &mut dependencies)?;
            for style in &chain {
                self.apply_char(&mut output.character, &style.character)?;
                output.paragraph.overlay(&style.paragraph);
            }
            toggles.apply_paragraph(style_toggles(&chain));
        }
        if let Some(id) = character_style {
            let chain = self.chain(id, StyleKind::Character, &mut dependencies)?;
            for style in &chain {
                self.apply_char(&mut output.character, &style.character)?;
            }
            toggles.apply_character(style_toggles(&chain));
        }
        toggles.write_to(&mut output.character);
        output.paragraph.style = None;
        Ok((output, dependencies))
    }

    fn chain(
        &self,
        id: &str,
        kind: StyleKind,
        dependencies: &mut BTreeSet<String>,
    ) -> Result<Vec<&Style>, StyleError> {
        let mut chain = Vec::new();
        let mut seen = BTreeSet::new();
        let mut next = Some(id);
        while let Some(current) = next {
            if !seen.insert(current.to_owned()) {
                return Err(StyleError::StyleCycle(current.to_owned()));
            }
            let style = self
                .catalog
                .styles
                .get(current)
                .ok_or_else(|| StyleError::MissingStyle(current.to_owned()))?;
            if style.kind != kind {
                return Err(StyleError::WrongStyleKind {
                    id: current.to_owned(),
                    expected: kind,
                });
            }
            dependencies.insert(current.to_owned());
            chain.push(style);
            next = style.based_on.as_deref();
            if chain.len() > self.catalog.styles.len() {
                return Err(StyleError::StyleCycle(current.to_owned()));
            }
        }
        chain.reverse();
        Ok(chain)
    }

    fn apply_char(
        &self,
        target: &mut CharAttrs,
        source: &StyleCharAttrs,
    ) -> Result<(), StyleError> {
        let mut values = source.values.clone();
        values.bold = None;
        values.italic = None;
        values.strike = None;
        target.overlay(&values);
        if source.values.font.is_none() {
            if let Some(slot) = source.theme_font {
                let font = self
                    .theme
                    .fonts
                    .get(&slot)
                    .ok_or(StyleError::MissingThemeFont(slot))?;
                if font.is_empty() {
                    return Err(StyleError::InvalidThemeFont(slot));
                }
                target.font = Some(font.clone());
            }
        }
        if source.values.color.is_none() {
            if let Some(slot) = source.theme_color {
                target.color = Some(
                    *self
                        .theme
                        .colors
                        .get(&slot)
                        .ok_or(StyleError::MissingThemeColor(slot))?,
                );
            }
        }
        if source.values.highlight.is_none() {
            if let Some(slot) = source.theme_highlight {
                target.highlight = Some(
                    *self
                        .theme
                        .colors
                        .get(&slot)
                        .ok_or(StyleError::MissingThemeColor(slot))?,
                );
            }
        }
        Ok(())
    }

    fn insert_cache(&mut self, key: CacheKey, entry: CacheEntry) {
        if self.capacity == 0 {
            return;
        }
        if let std::collections::btree_map::Entry::Occupied(mut existing) =
            self.cache.entry(key.clone())
        {
            existing.insert(entry);
            return;
        }
        while self.cache.len() >= self.capacity {
            let Some(oldest) = self.order.pop_front() else {
                break;
            };
            self.cache.remove(&oldest);
        }
        self.order.push_back(key.clone());
        self.cache.insert(key, entry);
    }

    fn invalidate_style(&mut self, id: &str) {
        let keys: Vec<_> = self
            .cache
            .iter()
            .filter(|(_, entry)| entry.dependencies.contains(id))
            .map(|(key, _)| key.clone())
            .collect();
        self.remove_keys(&keys);
    }

    fn invalidate_paragraphs(&mut self, ids: &BTreeSet<NodeId>) {
        let keys: Vec<_> = self
            .cache
            .keys()
            .filter(|key| ids.contains(&key.paragraph))
            .cloned()
            .collect();
        self.remove_keys(&keys);
    }

    fn remove_keys(&mut self, keys: &[CacheKey]) {
        for key in keys {
            self.cache.remove(key);
        }
        if !keys.is_empty() {
            self.order.retain(|key| !keys.contains(key));
        }
    }
}

fn table_style_for_paragraph(document: &RichDocument, target: NodeId) -> Option<Option<&str>> {
    fn walk<'a>(
        document: &'a RichDocument,
        id: NodeId,
        inherited: Option<&'a str>,
        in_table: bool,
        target: NodeId,
    ) -> Option<Option<&'a str>> {
        let node = document.node(id)?;
        let (current, in_table) = match node {
            Node::Table(table) => (table.style.as_deref().or(inherited), true),
            _ => (inherited, in_table),
        };
        if id == target {
            return in_table.then_some(current);
        }
        for child in node.children() {
            if let Some(found) = walk(document, child, current, in_table, target) {
                return Some(found);
            }
        }
        None
    }
    for section in document.section_ids() {
        if let Some(found) = walk(document, section, None, false, target) {
            return Some(found);
        }
    }
    None
}

fn affected_paragraphs(
    document: &RichDocument,
    transaction: &Transaction<RichOp>,
) -> BTreeSet<NodeId> {
    let mut ids = BTreeSet::new();
    for op in transaction.operations() {
        match op {
            RichOp::SetCharAttrs { para, .. } | RichOp::SetParaProps { para, .. } => {
                ids.insert(*para);
            }
            RichOp::SetTableStyle { table, .. } => collect_paragraphs(document, *table, &mut ids),
            RichOp::InsertBlock { subtree, .. } | RichOp::AttachHeaderFooter { subtree, .. } => {
                for (id, node) in &subtree.nodes {
                    if matches!(node, Node::Paragraph(_)) {
                        ids.insert(*id);
                    }
                }
            }
            RichOp::RemoveBlock { parent, position } => {
                if let Ok(blocks) = document.block_ids(*parent) {
                    if let Some(root) = blocks.get(*position) {
                        collect_paragraphs(document, *root, &mut ids);
                    }
                }
            }
            RichOp::DetachHeaderFooter {
                section,
                kind,
                variant,
            } => {
                if let Some(Node::Section(value)) = document.node(*section) {
                    let list = match kind {
                        super::node::HfKind::Header => &value.headers,
                        super::node::HfKind::Footer => &value.footers,
                    };
                    if let Some((_, root)) = list.iter().find(|(v, _)| v == variant) {
                        collect_paragraphs(document, *root, &mut ids);
                    }
                }
            }
            RichOp::RestoreSubtree { subtree } => {
                collect_paragraphs(document, subtree.root, &mut ids);
                for (id, node) in &subtree.nodes {
                    if matches!(node, Node::Paragraph(_)) {
                        ids.insert(*id);
                    }
                }
            }
            RichOp::SplitParagraph {
                parent, position, ..
            } => {
                if let Ok(blocks) = document.block_ids(*parent) {
                    if let Some(id) = blocks.get(*position) {
                        ids.insert(*id);
                    }
                }
            }
            RichOp::MergeParagraphs { parent, position }
            | RichOp::CollapsePair {
                parent, position, ..
            } => {
                if let Ok(blocks) = document.block_ids(*parent) {
                    ids.extend(blocks.get(*position));
                    ids.extend(blocks.get(position.saturating_add(1)));
                }
            }
            RichOp::ExpandPair { first, second, .. } => {
                for branch in [first, second] {
                    for (id, node) in &branch.nodes {
                        if matches!(node, Node::Paragraph(_)) {
                            ids.insert(*id);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    ids
}

fn collect_paragraphs(document: &RichDocument, root: NodeId, out: &mut BTreeSet<NodeId>) {
    let mut stack = vec![root];
    while let Some(id) = stack.pop() {
        match document.node(id) {
            Some(Node::Paragraph(_)) => {
                out.insert(id);
            }
            Some(node) => stack.extend(node.children()),
            None => {}
        }
    }
}
