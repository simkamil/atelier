// SPDX-License-Identifier: AGPL-3.0-or-later
//! Tracked changes and comments of a package (US-218): detection, and the
//! "accept everything, drop the comments" copy.
//!
//! The scan is a tag tokenizer over the raw XML of each Word part, not a
//! re-serialisation: the accepted copy is the source bytes minus the removed
//! ranges, so prefixes, `mc:Ignorable` and every other byte are untouched. A
//! part that cannot be accepted faithfully (a deleted paragraph mark, a
//! deleted or merged cell, an encoding other than UTF-8) is reported as
//! `blocked` and the copy is refused, never approximated.

use super::{W_NS, W_STRICT_NS};
use crate::{OpcError, Package, PartUri};

/// What a package holds in tracked changes and comments, with counts.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RevisionReport {
    /// `w:ins` (content, paragraph mark, row, cell).
    pub insertions: usize,
    /// `w:del`.
    pub deletions: usize,
    /// `w:moveFrom` and `w:moveTo`.
    pub moves: usize,
    /// `w:rPrChange`, `w:pPrChange`, `w:sectPrChange`, `w:tblPrChange`...
    pub property_changes: usize,
    /// Comment anchors and comment parts.
    pub comments: usize,
    /// Rows deleted by a tracked change (removed by the accepted copy).
    pub rows_removed: usize,
    /// Changes the accepted copy cannot reproduce (deleted paragraph marks,
    /// deleted or merged cells, parts that are not UTF-8).
    pub blocked: usize,
}

impl RevisionReport {
    /// A tracked change of any kind.
    pub fn has_revisions(&self) -> bool {
        self.insertions + self.deletions + self.moves + self.property_changes + self.blocked > 0
    }

    pub fn has_comments(&self) -> bool {
        self.comments > 0
    }

    fn add(&mut self, other: &Self) {
        self.insertions += other.insertions;
        self.deletions += other.deletions;
        self.moves += other.moves;
        self.property_changes += other.property_changes;
        self.comments += other.comments;
        self.rows_removed += other.rows_removed;
        self.blocked += other.blocked;
    }
}

/// Why an accepted copy could not be made.
#[derive(Debug)]
pub enum AcceptError {
    /// The package layer refused the result.
    Package(OpcError),
    /// Some change cannot be accepted faithfully.
    Unsupported(RevisionReport),
    /// A change was left after the pass (a safety net, never expected).
    Incomplete,
}

impl From<OpcError> for AcceptError {
    fn from(error: OpcError) -> Self {
        Self::Package(error)
    }
}

/// An accepted copy of a package.
#[derive(Debug)]
pub struct AcceptedRevisions {
    pub package: Package,
    /// What was accepted or removed.
    pub report: RevisionReport,
}

const PROPERTY_CHANGES: &[&str] = &[
    "rPrChange",
    "pPrChange",
    "sectPrChange",
    "tblPrChange",
    "tblPrExChange",
    "tblGridChange",
    "tcPrChange",
    "trPrChange",
    "numberingChange",
];
const RANGE_MARKS: &[&str] = &[
    "moveFromRangeStart",
    "moveFromRangeEnd",
    "moveToRangeStart",
    "moveToRangeEnd",
];

struct Open {
    local: String,
    is_w: bool,
    start: usize,
    bindings: usize,
    /// Drop the whole element, children included, at its end tag.
    drop_subtree: bool,
    /// Drop only the start and the end tag.
    unwrap: bool,
    /// The row holds a tracked deletion: drop it at its end tag.
    remove_row: bool,
}

/// Position of `pat` in `xml` from `from`.
fn find(xml: &[u8], from: usize, pat: &[u8]) -> Option<usize> {
    xml.get(from..)?
        .windows(pat.len())
        .position(|w| w == pat)
        .map(|p| p + from)
}

/// Namespace declarations of a start tag's attributes, pushed to `scope`.
fn read_bindings(mut tail: &str, scope: &mut Vec<(String, String)>) {
    while let Some(eq) = tail.find('=') {
        let key = tail[..eq].trim();
        let after = tail[eq + 1..].trim_start();
        let Some(q) = after.chars().next().filter(|c| *c == '"' || *c == '\'') else {
            break;
        };
        let Some(close) = after[1..].find(q) else {
            break;
        };
        let value = &after[1..1 + close];
        if let Some(prefix) = key.strip_prefix("xmlns:") {
            scope.push((prefix.to_owned(), value.to_owned()));
        } else if key == "xmlns" {
            scope.push((String::new(), value.to_owned()));
        }
        tail = after[1 + close + 1..].trim_start();
    }
}

/// Scans one XML part. Returns the ranges to remove (sorted, merged) and the
/// counts.
fn scan_part(xml: &[u8]) -> (Vec<(usize, usize)>, RevisionReport) {
    let mut report = RevisionReport::default();
    let mut cuts: Vec<(usize, usize)> = Vec::new();
    if xml.starts_with(&[0xff, 0xfe]) || xml.starts_with(&[0xfe, 0xff]) || xml.contains(&0) {
        report.blocked += 1;
        return (cuts, report);
    }
    let n = xml.len();
    let mut at = 0;
    let mut stack: Vec<Open> = Vec::new();
    let mut scope: Vec<(String, String)> = Vec::new();
    while at < n {
        if xml[at] != b'<' {
            at += 1;
            continue;
        }
        let rest = &xml[at..];
        let skip = if rest.starts_with(b"<!--") {
            Some((4, &b"-->"[..]))
        } else if rest.starts_with(b"<![CDATA[") {
            Some((9, &b"]]>"[..]))
        } else if rest.starts_with(b"<?") {
            Some((2, &b"?>"[..]))
        } else {
            None
        };
        if let Some((head, tail)) = skip {
            match find(xml, at + head, tail) {
                Some(end) => at = end + tail.len(),
                None => break,
            }
            continue;
        }
        if rest.starts_with(b"<!") {
            // A DTD is not part of WordprocessingML: not interpreted.
            report.blocked += 1;
            return (Vec::new(), report);
        }
        // Tag end, outside quotes.
        let mut j = at + 1;
        let mut quote = 0u8;
        while j < n {
            let c = xml[j];
            if quote != 0 {
                if c == quote {
                    quote = 0;
                }
            } else if c == b'"' || c == b'\'' {
                quote = c;
            } else if c == b'>' {
                break;
            }
            j += 1;
        }
        if j >= n {
            break;
        }
        let tag_end = j + 1;
        if rest.starts_with(b"</") {
            if let Some(open) = stack.pop() {
                if open.drop_subtree || open.remove_row {
                    cuts.push((open.start, tag_end));
                } else if open.unwrap {
                    cuts.push((at, tag_end));
                }
                scope.truncate(open.bindings);
            }
            at = tag_end;
            continue;
        }
        let empty = xml[tag_end - 2] == b'/';
        let inner = &xml[at + 1..tag_end - 1 - usize::from(empty)];
        let text = String::from_utf8_lossy(inner);
        let qname = text.split_ascii_whitespace().next().unwrap_or("");
        let bindings = scope.len();
        read_bindings(text[qname.len()..].trim_start(), &mut scope);
        let (prefix, local) = qname.split_once(':').unwrap_or(("", qname));
        let is_w = scope
            .iter()
            .rev()
            .find(|(p, _)| p == prefix)
            .is_some_and(|(_, uri)| uri == W_NS || uri == W_STRICT_NS);
        let parent_local = stack
            .last()
            .filter(|o| o.is_w)
            .map_or("", |o| o.local.as_str())
            .to_owned();
        let inside_dropped = stack.iter().any(|o| o.drop_subtree);
        let mut open = Open {
            local: local.to_owned(),
            is_w,
            start: at,
            bindings,
            drop_subtree: false,
            unwrap: false,
            remove_row: false,
        };
        if is_w && !inside_dropped {
            let marker = matches!(parent_local.as_str(), "rPr" | "trPr");
            match local {
                "ins" | "moveTo" => {
                    if local == "ins" {
                        report.insertions += 1;
                    } else {
                        report.moves += 1;
                    }
                    if marker {
                        open.drop_subtree = true;
                    } else {
                        open.unwrap = true;
                    }
                }
                "del" | "moveFrom" => {
                    if local == "del" {
                        report.deletions += 1;
                    } else {
                        report.moves += 1;
                    }
                    if parent_local == "rPr" {
                        // A deleted paragraph mark merges two paragraphs.
                        report.blocked += 1;
                    } else if parent_local == "trPr" {
                        report.rows_removed += 1;
                        if let Some(row) =
                            stack.iter_mut().rev().find(|o| o.is_w && o.local == "tr")
                        {
                            row.remove_row = true;
                        }
                    }
                    open.drop_subtree = true;
                }
                "cellDel" | "cellMerge" => {
                    report.blocked += 1;
                    report.deletions += 1;
                }
                "cellIns" => {
                    report.insertions += 1;
                    open.drop_subtree = true;
                }
                l if PROPERTY_CHANGES.contains(&l) => {
                    report.property_changes += 1;
                    open.drop_subtree = true;
                }
                l if RANGE_MARKS.contains(&l) => open.drop_subtree = true,
                "commentRangeStart" | "commentRangeEnd" | "commentReference" => {
                    report.comments += 1;
                    open.drop_subtree = true;
                }
                _ => {}
            }
        }
        if empty {
            // An empty element is dropped whole; an empty wrapper just goes.
            if open.drop_subtree || open.unwrap {
                cuts.push((at, tag_end));
            }
            scope.truncate(bindings);
        } else {
            if open.unwrap {
                cuts.push((at, tag_end));
            }
            stack.push(open);
        }
        at = tag_end;
    }
    if !stack.is_empty() {
        // Not well formed: the package layer would have refused it.
        report.blocked += 1;
    }
    cuts.sort_unstable();
    let mut merged: Vec<(usize, usize)> = Vec::with_capacity(cuts.len());
    for (start, end) in cuts {
        if start == end {
            continue;
        }
        match merged.last_mut() {
            Some(last) if start <= last.1 => last.1 = last.1.max(end),
            _ => merged.push((start, end)),
        }
    }
    (merged, report)
}

fn is_comment_part(content_type: &str) -> bool {
    let lower = content_type.to_ascii_lowercase();
    lower.contains("comments") || lower.ends_with("wordprocessingml.people+xml")
}

fn is_word_xml(content_type: &str, uri: &str) -> bool {
    uri.ends_with(".xml")
        && (content_type
            .starts_with("application/vnd.openxmlformats-officedocument.wordprocessingml.")
            || content_type.starts_with("application/vnd.ms-word."))
        && content_type.ends_with("+xml")
}

/// Tracked changes and comments of the package, over every Word part.
pub fn scan_revisions(package: &Package) -> RevisionReport {
    let mut report = RevisionReport::default();
    for part in package.parts().values() {
        if is_comment_part(&part.content_type) {
            report.comments += 1;
        } else if is_word_xml(&part.content_type, part.uri.as_str()) {
            report.add(&scan_part(&part.bytes).1);
        }
    }
    report
}

/// A copy of the package where every tracked change is accepted and the
/// comments are gone: insertions and moves-to keep their content, deletions
/// and moves-from vanish, property changes keep the current properties, rows
/// with a tracked deletion are removed, comment anchors and the comment parts
/// are removed. Refuses what it cannot reproduce faithfully
/// ([`RevisionReport::blocked`]).
pub fn accept_all_revisions(package: &Package) -> Result<AcceptedRevisions, AcceptError> {
    let mut copy = package.clone();
    let mut report = RevisionReport::default();
    let mut doomed: Vec<PartUri> = Vec::new();
    let mut edits: Vec<(PartUri, Vec<u8>)> = Vec::new();
    for part in package.parts().values() {
        if is_comment_part(&part.content_type) {
            report.comments += 1;
            doomed.push(part.uri.clone());
        } else if is_word_xml(&part.content_type, part.uri.as_str()) {
            let (cuts, found) = scan_part(&part.bytes);
            report.add(&found);
            if !cuts.is_empty() {
                let mut out = Vec::with_capacity(part.bytes.len());
                let mut from = 0;
                for (start, end) in cuts {
                    out.extend_from_slice(&part.bytes[from..start]);
                    from = end;
                }
                out.extend_from_slice(&part.bytes[from..]);
                edits.push((part.uri.clone(), out));
            }
        }
    }
    if report.blocked > 0 {
        return Err(AcceptError::Unsupported(report));
    }
    for (uri, bytes) in edits {
        copy.replace_part_bytes(&uri, bytes);
    }
    for uri in &doomed {
        copy.remove_part(uri);
    }
    let left = scan_revisions(&copy);
    if left.has_revisions() || left.has_comments() {
        return Err(AcceptError::Incomplete);
    }
    Ok(AcceptedRevisions {
        package: copy,
        report,
    })
}
