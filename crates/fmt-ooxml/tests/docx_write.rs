// SPDX-License-Identifier: AGPL-3.0-or-later
//! US-307: the DOCX writer. Round trip over the corpus and the documents
//! built by the other DOCX tests.
mod support;

use support::*;

#[test]
fn corpus_and_built_documents_round_trip_without_difference() {
    let mut docs = corpus();
    docs.extend(built_documents());
    let mut total = RoundTrip::default();
    for (name, bytes) in &docs {
        let rt = round_trip(name, bytes);
        total.parts_compared += rt.parts_compared;
        total.parts_identical += rt.parts_identical;
        total.differences.extend(rt.differences);
    }
    println!(
        "{} documents, {} interpreted parts compared, {} identical",
        docs.len(),
        total.parts_compared,
        total.parts_identical
    );
    assert!(total.differences.is_empty(), "{:#?}", total.differences);
}
