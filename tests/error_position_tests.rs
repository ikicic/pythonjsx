//! Tests that verify compile errors report precise byte ranges and correct
//! (line, column) positions under a range of prefix/suffix wrappings.
//!
//! Strategy: each test case is written with Unicode guillemets « and » marking
//! the expected error span inside a JSX snippet. The helpers strip the markers,
//! wrap the clean snippet with many combinations of plain-Python prefix and
//! suffix text, and assert that:
//!   1. The compiler reports an error whose message contains a given substring.
//!   2. The error's byte range exactly matches the marked span (offset by the
//!      prefix length).
//!   3. `LineColumnMap` converts that byte range to (line, column) that matches
//!      an independent newline-counting computation, and that line_col→byte
//!      round-trips.

use pythonjsx::compiler::sourcemap::LineColumnMap;
use pythonjsx::compiler::{compile, CompilerSettings};
use pythonjsx::{CompileError, CompileErrorSeverity};

const START_MARKER: &str = "«";
const END_MARKER: &str = "»";

/// Strip « and » markers from `snippet` and return
/// (clean_text, marked_start_byte_in_clean, marked_end_byte_in_clean).
fn parse_markers(snippet: &str) -> (String, usize, usize) {
    let s_pos = snippet
        .find(START_MARKER)
        .unwrap_or_else(|| panic!("snippet missing « marker: {:?}", snippet));
    let after_s = &snippet[s_pos + START_MARKER.len()..];
    let e_pos_rel = after_s
        .find(END_MARKER)
        .unwrap_or_else(|| panic!("snippet missing » marker: {:?}", snippet));
    let middle = &after_s[..e_pos_rel];
    let suffix = &after_s[e_pos_rel + END_MARKER.len()..];

    let mut clean =
        String::with_capacity(snippet.len() - START_MARKER.len() - END_MARKER.len());
    clean.push_str(&snippet[..s_pos]);
    clean.push_str(middle);
    clean.push_str(suffix);

    (clean, s_pos, s_pos + middle.len())
}

/// Plain-Python prefixes (no JSX). Each ends with `\n` so the snippet starts
/// at column 0 after wrapping, keeping column math simple.
fn prefixes() -> &'static [&'static str] {
    &[
        "",
        "x = 1\n",
        "x = 1\ny = 2\n",
        "# comment line\n# another comment\n# third\n",
        "def helper():\n    return 42\n\n",
        "# café — leading multi-byte comment\n",
        "\n\n\n",
        "# one\n\n# blank line before and after\n\n",
        "import os\nimport sys\nimport json\nimport re\nimport math\n",
    ]
}

/// Plain-Python suffixes (no JSX).
fn suffixes() -> &'static [&'static str] {
    &["", "\n", "\nx = 1\n", "\n# trailing\n", "\n\n\n"]
}

/// Compile `full` source and return the errors, panicking if parse fails.
fn compile_errors(full: &str) -> Vec<CompileError> {
    let (_output, _sm, errors) =
        compile(full, &CompilerSettings::default()).expect("parse failed");
    errors
}

/// Count newlines in `s[..byte]` and compute column (distance to the last `\n`
/// before `byte`, or `byte` itself if none).
fn expected_line_col(full: &str, byte: usize) -> (usize, usize) {
    let up_to = &full[..byte];
    let line = up_to.matches('\n').count();
    let col = match up_to.rfind('\n') {
        Some(p) => byte - (p + 1),
        None => byte,
    };
    (line, col)
}

/// Core assertion: compile `full` and verify:
///   * A diagnostic (`severity` matches `want_severity`) whose message
///     contains `msg_substr` exists.
///   * Its byte range equals `[want_start, want_end)`.
///   * LineColumnMap agrees with an independent newline-counting computation,
///     and line_col→byte round-trips for both endpoints.
fn check_compiled(
    full: &str,
    want_start: usize,
    want_end: usize,
    msg_substr: &str,
    want_severity: &CompileErrorSeverity,
) {
    let errors = compile_errors(full);

    let matched: Vec<&CompileError> = errors
        .iter()
        .filter(|e| &e.severity == want_severity && e.message.contains(msg_substr))
        .collect();
    assert!(
        !matched.is_empty(),
        "expected {:?} containing {:?}; got {:#?}\nsource:\n{}",
        want_severity,
        msg_substr,
        errors,
        full
    );

    let exact = matched
        .iter()
        .find(|e| e.range.start == want_start && e.range.end == want_end);
    assert!(
        exact.is_some(),
        "expected range [{}..{}] ({:?}) for error {:?}; got ranges: {:?}\nsource:\n{}",
        want_start,
        want_end,
        &full[want_start..want_end],
        msg_substr,
        matched
            .iter()
            .map(|e| {
                (
                    e.range.start,
                    e.range.end,
                    full[e.range.clone()].to_string(),
                    e.message.clone(),
                )
            })
            .collect::<Vec<_>>(),
        full
    );

    // Line/column verification.
    let lcm = LineColumnMap::new(full);
    let (sl, sc) = lcm.byte_to_line_col(want_start);
    let (el, ec) = lcm.byte_to_line_col(want_end);

    let (esl, esc) = expected_line_col(full, want_start);
    let (eel, eec) = expected_line_col(full, want_end);
    assert_eq!(
        (sl, sc),
        (esl, esc),
        "start byte_to_line_col mismatch at byte {}\nsource:\n{}",
        want_start,
        full
    );
    assert_eq!(
        (el, ec),
        (eel, eec),
        "end byte_to_line_col mismatch at byte {}\nsource:\n{}",
        want_end,
        full
    );

    assert_eq!(
        lcm.line_col_to_byte(sl, sc),
        want_start,
        "start round-trip failed"
    );
    assert_eq!(
        lcm.line_col_to_byte(el, ec),
        want_end,
        "end round-trip failed"
    );
}

/// Compile `snippet_with_markers` (containing « and »), wrap it in every
/// prefix/suffix combination, and assert the compiler reports a diagnostic
/// of `severity` matching `msg_substr` at the marked span.
fn assert_diag_at_marker(
    snippet_with_markers: &str,
    msg_substr: &str,
    severity: &CompileErrorSeverity,
) {
    let (clean, want_start_in_clean, want_end_in_clean) = parse_markers(snippet_with_markers);

    for prefix in prefixes() {
        for suffix in suffixes() {
            let mut full = String::with_capacity(prefix.len() + clean.len() + suffix.len());
            full.push_str(prefix);
            full.push_str(&clean);
            full.push_str(suffix);

            let want_start = prefix.len() + want_start_in_clean;
            let want_end = prefix.len() + want_end_in_clean;

            check_compiled(&full, want_start, want_end, msg_substr, severity);
        }
    }
}

/// Error variant of `assert_diag_at_marker`, for existing error tests.
fn assert_error_at_marker(snippet_with_markers: &str, msg_substr: &str) {
    assert_diag_at_marker(snippet_with_markers, msg_substr, &CompileErrorSeverity::Error);
}

/// Warning variant; also asserts no error-severity diagnostic is produced
/// for the same source, so warning cases stay strictly non-fatal.
fn assert_warning_at_marker(snippet_with_markers: &str, msg_substr: &str) {
    assert_diag_at_marker(snippet_with_markers, msg_substr, &CompileErrorSeverity::Warning);

    // Sanity: no error-severity diagnostics.
    let (clean, _, _) = parse_markers(snippet_with_markers);
    for prefix in prefixes() {
        for suffix in suffixes() {
            let mut full = String::with_capacity(prefix.len() + clean.len() + suffix.len());
            full.push_str(prefix);
            full.push_str(&clean);
            full.push_str(suffix);
            let errs = compile_errors(&full);
            assert!(
                errs.iter().all(|e| e.severity == CompileErrorSeverity::Warning),
                "warning-only case produced an error-severity diagnostic: {:#?}\nsource:\n{}",
                errs,
                full
            );
        }
    }
}

/// Assert that compiling `source` produces no diagnostics of any severity,
/// under all prefix/suffix wrappings. Used for negative cases that should
/// stay silent (bare `&`, `&;`, numeric refs, etc.).
fn assert_no_diagnostic(source: &str) {
    for prefix in prefixes() {
        for suffix in suffixes() {
            let mut full = String::with_capacity(prefix.len() + source.len() + suffix.len());
            full.push_str(prefix);
            full.push_str(source);
            full.push_str(suffix);
            let errs = compile_errors(&full);
            assert!(
                errs.is_empty(),
                "expected no diagnostics but got {:#?}\nsource:\n{}",
                errs,
                full
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Precise-range cases that already work today.
// ---------------------------------------------------------------------------

#[test]
fn pos_mismatched_closing_tag() {
    assert_error_at_marker(
        "x = <div>foo</«span»>",
        "Expected tag name",
    );
}

#[test]
fn pos_mismatched_closing_tag_component() {
    assert_error_at_marker(
        "x = <Header>foo</«Footer»>",
        "Expected tag name",
    );
}

#[test]
fn pos_spread_missing_star_star() {
    assert_error_at_marker(
        "x = <div «{kwargs}»></div>",
        "Spread attribute requires",
    );
}

#[test]
fn pos_spread_missing_star_star_expression() {
    assert_error_at_marker(
        "x = <div «{a + b}»></div>",
        "Spread attribute requires",
    );
}

#[test]
fn pos_orphan_closing_tag() {
    assert_error_at_marker(
        "«</div>»",
        "Unexpected closing tag",
    );
}

#[test]
fn pos_invalid_attribute_value() {
    assert_error_at_marker(
        "x = <div «a=5»></div>",
        "Attribute value must be a string or expression",
    );
}

#[test]
fn pos_missing_attribute_name() {
    assert_error_at_marker(
        "x = <div «=\"a\"»></div>",
        "Missing attribute name",
    );
}

#[test]
fn pos_multi_spread_disallowed() {
    assert_error_at_marker(
        "x = <div «{**a, **b}»></div>",
        "multiple spreads",
    );
}

#[test]
fn pos_duplicate_attribute_kv() {
    assert_error_at_marker(
        "x = <div a=\"1\" «a»=\"2\"></div>",
        "Duplicate attribute 'a'",
    );
}

#[test]
fn pos_duplicate_attribute_boolean() {
    assert_error_at_marker(
        "x = <div foo «foo»></div>",
        "Duplicate attribute 'foo'",
    );
}

#[test]
fn pos_duplicate_attribute_mixed() {
    assert_error_at_marker(
        "x = <div a=\"1\" «a»></div>",
        "Duplicate attribute 'a'",
    );
}

// ---------------------------------------------------------------------------
// Narrowed diagnostics recovered from wide ERROR nodes.
// ---------------------------------------------------------------------------

#[test]
fn pos_mismatched_tag_at_depth_inner() {
    // Inner `<span>...</p>` mismatch should land precisely on `p`.
    assert_error_at_marker(
        "x = <div><span>foo</«p»></span></div>",
        "Expected tag name 'span', got 'p'",
    );
}

#[test]
fn pos_mismatched_tag_at_depth_no_double_emission() {
    // Regression: two code paths can emit "Expected tag name 'span', got 'p'"
    // — the ERROR-branch nested scan and the per-element tail check inside
    // `visit_jsx_element`. They must be mutually exclusive, so a nested
    // mismatch should produce exactly one diagnostic with that message, not
    // two. Check under every prefix/suffix wrapping so tree-sitter's
    // recovery shape can vary.
    let clean = "x = <div><span>foo</p></span></div>";
    for prefix in prefixes() {
        for suffix in suffixes() {
            let mut full = String::with_capacity(prefix.len() + clean.len() + suffix.len());
            full.push_str(prefix);
            full.push_str(clean);
            full.push_str(suffix);
            let errs = compile_errors(&full);
            let span_p_count = errs
                .iter()
                .filter(|e| e.message.contains("Expected tag name 'span', got 'p'"))
                .count();
            assert_eq!(
                span_p_count, 1,
                "expected exactly one span/p mismatch diagnostic, got {} in {:#?}\nsource:\n{}",
                span_p_count, errs, full
            );
        }
    }
}

#[test]
fn pos_fragment_closed_with_element() {
    assert_error_at_marker(
        "x = <>foo«</span>»",
        "Fragment must close with </>",
    );
}

#[test]
fn pos_element_closed_with_fragment() {
    assert_error_at_marker(
        "x = <span>foo«</>»",
        "Expected closing tag '</span>', got '</>'",
    );
}

#[test]
fn pos_malformed_opening_tag() {
    assert_error_at_marker(
        "x = «<divfoo»</div>",
        "Malformed opening tag",
    );
}

#[test]
fn pos_unclosed_element() {
    assert_error_at_marker(
        "x = «<div>»foo",
        "Unclosed element '<div>'",
    );
}

#[test]
fn pos_stray_gt_in_opening_tag() {
    // `<div>>foo</div>` — the extra `>` leaks into the opening tag as an
    // ERROR child of the jsx_element.
    assert_error_at_marker(
        "x = <div«>»>foo</div>",
        "Unexpected token",
    );
}

#[test]
fn pos_stray_gt_in_children() {
    // `<div>foo>bar</div>` — `>bar` absorbed as ERROR between children.
    assert_error_at_marker(
        "x = <div>foo«>bar»</div>",
        "Unexpected token",
    );
}

#[test]
fn pos_unclosed_inner_opening_tag() {
    // `<div>foo<bar</div>` — `<bar` never closes; tree-sitter synthesises
    // an empty `/>` which we detect via `is_missing()`.
    assert_error_at_marker(
        "x = <div>foo«<bar»</div>",
        "Unclosed or malformed opening tag '<bar'",
    );
}

// ---------------------------------------------------------------------------
// Unknown HTML entity warnings (severity=Warning, not Error).
// ---------------------------------------------------------------------------

#[test]
fn pos_warn_unknown_entity_in_jsx_text() {
    assert_warning_at_marker(
        "x = <div>bla «&asdf;» bla</div>",
        "Unknown named HTML entity &asdf;",
    );
}

#[test]
fn pos_warn_unknown_entity_in_attribute() {
    assert_warning_at_marker(
        "x = <div title=\"foo «&nsbp;» bar\"></div>",
        "Unknown named HTML entity &nsbp;",
    );
}

#[test]
fn pos_warn_unknown_entity_at_start_of_text() {
    assert_warning_at_marker(
        "x = <div>«&amps;» rest</div>",
        "Unknown named HTML entity &amps;",
    );
}

#[test]
fn pos_warn_known_entities_no_warning() {
    // Both `&amp;` and `&lt;` are known; no diagnostic at all.
    assert_no_diagnostic("x = <div>a &amp; b &lt; c</div>");
}

#[test]
fn pos_warn_non_entity_shapes_silent() {
    // Bare `&`, empty `&;`, digit-start, no-semicolon, numeric refs, etc.
    // must not warn.
    for s in [
        "x = <div>a & b</div>",
        "x = <div>a &; b</div>",
        "x = <div>a &123; b</div>",
        "x = <div>a &#60; b</div>",
        "x = <div>a &#x3E; b</div>",
        "x = <div>a &abc_def; b</div>",
    ] {
        assert_no_diagnostic(s);
    }
}

#[test]
fn pos_warn_multiple_unknowns_each_reported() {
    // Both should be independently flagged. We assert on the first one; the
    // shared check_compiled already tolerates extra diagnostics.
    assert_warning_at_marker(
        "x = <div>«&foo;» and &bar; done</div>",
        "Unknown named HTML entity &foo;",
    );
    assert_warning_at_marker(
        "x = <div>&foo; and «&bar;» done</div>",
        "Unknown named HTML entity &bar;",
    );
}
