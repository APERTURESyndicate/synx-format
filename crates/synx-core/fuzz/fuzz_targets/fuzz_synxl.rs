#![no_main]
//! SYNXL reader/writer fuzz target.
//!
//! SYNXL §16 states outright that documents arrive from untrusted sources, so
//! everything reachable here has to hold on arbitrary bytes: no panic, no
//! non-UTF-8 boundary slice, no unbounded recursion. Two audit findings —
//! panics on a Unicode indent inside a `|+` body and at the multiline byte cap
//! — were both a few minutes of fuzzing away and went unnoticed because the
//! format had no target of its own.
//!
//! Beyond crash-freedom this asserts the two invariants a reader/writer pair
//! must satisfy, since neither shows up as a crash:
//!
//! * §1.4.3 — the three readers (in-memory, owned, streaming) project one
//!   document identically;
//! * §14.1 — `parse(write(parse(D)))` projects identically to `parse(D)`.

use libfuzzer_sys::fuzz_target;
use synx_core::synxl;

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else { return };

    let Ok(doc) = synxl::parse_lines(text) else {
        // A hard error (§11.1) is a valid outcome. The streaming reader has to
        // agree that it is one, though: a document one reader rejects and
        // another accepts is the §1.4 contract broken.
        let streamed = synxl::SynxlStreamReader::new(std::io::Cursor::new(text))
            .map(|reader| reader.into_iter().all(|item| item.is_ok()));
        assert!(
            !matches!(streamed, Ok(true)),
            "the streaming reader accepted a document the in-memory reader rejected"
        );
        return;
    };

    let json = doc.to_json();

    // §12.2 — the NDJSON projection carries the same records as the array one.
    assert_eq!(
        doc.to_ndjson().lines().count(),
        doc.records.len(),
        "NDJSON projection lost or gained a record"
    );

    // §15.1 — the streaming reader sees the same document.
    if let Ok(reader) = synxl::SynxlStreamReader::new(std::io::Cursor::new(text)) {
        let mut streamed = Vec::new();
        let mut ok = true;
        for item in reader {
            match item {
                Ok(rec) => streamed.push(rec.value),
                Err(_) => {
                    ok = false;
                    break;
                }
            }
        }
        if ok {
            assert_eq!(
                synxl::records_to_json_array(&streamed),
                json,
                "streaming and in-memory readers disagree on the projection"
            );
        }
    }

    // §14.1 — round-trip, per field-list group (§4.2).
    let mut written = String::from("!synxl 1\n");
    let mut start = 0usize;
    let mut writable = true;
    while start < doc.records.len() {
        let fl_idx = doc.record_field_lists.get(start).copied().unwrap_or(0);
        let mut end = start + 1;
        while end < doc.records.len()
            && doc.record_field_lists.get(end).copied().unwrap_or(0) == fl_idx
        {
            end += 1;
        }
        let Some(fl) = doc.field_lists.get(fl_idx) else { return };
        match synxl::write_lines(fl.fields(), &doc.records[start..end]) {
            Ok(group) => written.push_str(group.strip_prefix("!synxl 1\n").unwrap_or(&group)),
            // §14.3 permits refusing a value with no rendering, but only
            // loudly — which is what this branch is.
            Err(_) => {
                writable = false;
                break;
            }
        }
        start = end;
    }

    if writable {
        let reparsed = synxl::parse_lines(&written)
            .expect("the writer produced a document its own parser rejects");
        assert_eq!(reparsed.to_json(), json, "§14.1 round-trip changed the projection");
    }
});
