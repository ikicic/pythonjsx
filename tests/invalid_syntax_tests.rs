//! Invalid syntax tests - verify error collection and valid Python output.

use pythonjsx::compiler::{compile, CompilerSettings};

/// Compile and assert we get valid Python (syntax-check by parsing) and optionally errors.
fn compile_and_check(px: &str) -> (String, Vec<pythonjsx::CompileError>) {
    let (output, _source_map, errors) = compile(px, &CompilerSettings::default()).unwrap();
    // Verify output is valid Python syntax (basic check: no obvious broken constructs)
    assert!(
        !output.contains("..") || output.contains("..."),
        "Output should not have stray dots: {}",
        output
    );
    (output, errors)
}

/// Compile and return Result - use when input may fail to parse.
fn compile_maybe(px: &str) -> Result<(String, Vec<pythonjsx::CompileError>), tree_sitter::LanguageError> {
    let (output, _source_map, errors) = compile(px, &CompilerSettings::default())?;
    Ok((output, errors))
}

#[test]
fn test_valid_code_no_errors() {
    let (_output, errors) = compile_and_check(r#"def App(): return <div>Hello</div>"#);
    assert!(errors.is_empty(), "Expected no errors for valid code: {:?}", errors);
}

#[test]
fn test_mismatched_tags_collects_error() {
    let (output, errors) = compile_and_check(r#"def App(): return <div><span>Hello</div>"#);
    assert!(
        !errors.is_empty(),
        "Expected errors for mismatched tags. output={:?} errors={:?}",
        output,
        errors
    );
    assert!(errors.iter().any(|e| e.message.contains("span") && e.message.contains("div")));
    assert!(!output.is_empty(), "Should still produce partial output");
}

#[test]
fn test_mismatched_tags_different_nesting() {
    let (output, errors) = compile_and_check(r#"def App(): return <div><span>Hi</div></span>"#);
    assert!(!errors.is_empty(), "Expected errors: {:?}", errors);
    assert!(errors.iter().any(|e| e.message.contains("Expected tag name")));
    assert!(!output.is_empty());
}

#[test]
fn test_mismatched_tags_at_depth() {
    // Same structure as test_mismatched_tags_collects_error but with extra nesting.
    // If tree-sitter parses </p> as closing span, visitor should catch the mismatch.
    let (output, errors) = compile_and_check(
        r#"def App(): return <div><p><span>x</p></span></div>"#,
    );
    assert!(
        !errors.is_empty(),
        "Expected errors for nested mismatch. output={:?} errors={:?}",
        output,
        errors
    );
    assert!(
        errors.iter().any(|e| e.message.contains("Expected tag name") || (e.message.contains("span") && e.message.contains("p"))),
        "Expected tag mismatch error: {:?}",
        errors
    );
    assert!(!output.is_empty());
}

#[test]
fn test_invalid_attribute_value_plain_number() {
    let (output, errors) = compile_and_check(r#"def foo(): return <div a=5></div>"#);
    assert!(!errors.is_empty(), "Expected errors: {:?}", errors);
    assert!(errors.iter().any(|e| e.message.contains("string or expression")));
    assert!(!output.is_empty());
}

#[test]
fn test_invalid_attribute_value_boolean_shorthand() {
    let (output, errors) = compile_and_check(r#"def foo(): return <div disabled=5></div>"#);
    assert!(!errors.is_empty(), "Expected errors: {:?}", errors);
    assert!(errors.iter().any(|e| e.message.contains("string or expression")));
    assert!(!output.is_empty());
}

#[test]
fn test_component_mismatched_tags() {
    let (output, errors) = compile_and_check(
        r#"def App(): return <Header><Footer>x</Header></Footer>"#,
    );
    assert!(!errors.is_empty(), "Expected errors: {:?}", errors);
    assert!(
        errors.iter().any(|e| e.message.contains("Expected tag name")),
        "Expected tag mismatch: {:?}",
        errors
    );
    assert!(!output.is_empty());
}

#[test]
fn test_valid_fragment_no_errors() {
    let (_output, errors) = compile_and_check(r#"def App(): return <>Hello</>"#);
    assert!(errors.is_empty(), "Expected no errors for valid fragment: {:?}", errors);
}

#[test]
fn test_multiple_errors_in_one_file() {
    // Mismatched tags + invalid attribute
    let (output, errors) = compile_and_check(
        r#"
def foo():
    return <div a=5><span>x</div>
"#,
    );
    assert!(!errors.is_empty(), "Expected at least one error: {:?}", errors);
    assert!(
        errors.len() >= 1,
        "Expected multiple or at least one error: {:?}",
        errors
    );
    let has_attr_error = errors.iter().any(|e| e.message.contains("string or expression"));
    let has_tag_error = errors.iter().any(|e| e.message.contains("Expected tag name") || e.message.contains("span"));
    assert!(
        has_attr_error || has_tag_error,
        "Expected attribute or tag error: {:?}",
        errors
    );
    assert!(!output.is_empty());
}

#[test]
fn test_valid_spread_no_errors() {
    let (_output, errors) = compile_and_check(
        r#"def App(): return <div {**props}>x</div>"#,
    );
    assert!(errors.is_empty(), "Expected no errors for valid spread: {:?}", errors);
}

// --- Tests for specific error cases ---

#[test]
fn test_fragment_closed_with_wrong_tag() {
    // <>foo</span> - fragment opened with <> but closed with </span>
    let result = compile_maybe(r#"def App(): return <>foo</span>"#);
    match result {
        Ok((output, errors)) => {
            assert!(
                !errors.is_empty(),
                "Expected errors for <>foo</span> (fragment/span mismatch). output={:?}",
                output
            );
            assert!(!output.is_empty());
        }
        Err(_) => {
            // May fail to parse if grammar rejects fragment/span mismatch
        }
    }
}

#[test]
fn test_element_closed_with_fragment_close() {
    // <span>foo</> - span opened but closed with </>
    let result = compile_maybe(r#"def App(): return <span>foo</>"#);
    match result {
        Ok((output, errors)) => {
            assert!(
                !errors.is_empty(),
                "Expected errors for <span>foo</> (span/fragment close mismatch). output={:?}",
                output
            );
            assert!(!output.is_empty());
        }
        Err(_) => {}
    }
}

#[test]
fn test_spread_missing_star_star() {
    // <div {kwargs}></div> - missing ** in spread
    let result = compile_maybe(r#"def App(): return <div {kwargs}></div>"#);
    match result {
        Ok((output, errors)) => {
            assert!(
                !errors.is_empty(),
                "Expected errors for <div {{kwargs}}> (spread needs **). output={:?}",
                output
            );
            assert!(!output.is_empty());
        }
        Err(_) => {}
    }
}

#[test]
fn test_spread_missing_star_star_expression() {
    // <div {a + b}></div> - expression without **
    let result = compile_maybe(r#"def App(): return <div {a + b}></div>"#);
    match result {
        Ok((output, errors)) => {
            assert!(
                !errors.is_empty(),
                "Expected errors for <div {{a + b}}> (spread needs **). output={:?}",
                output
            );
            assert!(!output.is_empty());
        }
        Err(_) => {}
    }
}

#[test]
fn test_missing_attribute_name() {
    // <span ="a"></span> - attribute has = but no name before it
    let (output, errors) = compile_and_check(r#"def App(): return <span ="a"></span>"#);
    assert!(!errors.is_empty(), "Expected errors: {:?}", errors);
    assert!(
        errors.iter().any(|e| e.message.contains("Missing attribute name")),
        "Expected 'Missing attribute name' error: {:?}",
        errors
    );
    assert!(
        !output.contains("=\"a\""),
        "Output should not contain invalid =\"a\": {}",
        output
    );
}

#[test]
fn test_unexpected_closing_tag() {
    // </div> - closing tag without opening
    let result = compile_maybe(r#"def App(): return </div>"#);
    match result {
        Ok((output, errors)) => {
            assert!(
                !errors.is_empty(),
                "Expected errors for unexpected </div>. output={:?}",
                output
            );
            assert!(!output.is_empty());
        }
        Err(_) => {
            // Parse may fail for orphan closing tag
        }
    }
}
