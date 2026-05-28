/// Decode HTML entity references (both named and numeric) in a string.
///
/// `&amp;` → `&`, `&#60;` → `<`, `&#x3E;` → `>`, etc.
/// Unknown or malformed references are left unchanged.
pub fn decode_html_entities(s: &str) -> String {
    decode_html_entities_with_unknowns(s).0
}

/// Like [`decode_html_entities`] but also returns byte ranges (within `s`) of
/// every *entity-shaped* reference that didn't match any known name. An
/// entity-shaped reference is `&name;` where `name` starts with an ASCII
/// letter and contains only ASCII letters and digits — i.e. the shape of a
/// named HTML entity. Bare `&`, empty `&;`, and numeric references (`&#...;`,
/// `&#x...;`) never produce a warning here.
pub fn decode_html_entities_with_unknowns(s: &str) -> (String, Vec<(usize, usize)>) {
    let mut unknowns: Vec<(usize, usize)> = Vec::new();
    if !s.contains('&') {
        return (s.to_string(), unknowns);
    }
    let mut result = String::with_capacity(s.len());
    // `base` is the absolute byte offset (within the original `s`) of the
    // start of `remaining`, so we can translate local finds back to `s`.
    let mut base: usize = 0;
    let mut remaining: &str = s;
    while let Some(amp_pos) = remaining.find('&') {
        result.push_str(&remaining[..amp_pos]);
        let amp_abs = base + amp_pos;
        // Advance past the `&`; the loop will reprocess the name chars if we
        // end up leaving it as a literal.
        base += amp_pos + 1;
        remaining = &remaining[amp_pos + 1..];
        // Longest HTML5 named entity is ~33 chars; scan a bounded window.
        let limit = remaining.len().min(50);
        if let Some(semi_pos) = remaining[..limit].find(';') {
            let name = &remaining[..semi_pos];
            if let Some(decoded) = decode_entity(name) {
                result.push_str(decoded);
                base += semi_pos + 1;
                remaining = &remaining[semi_pos + 1..];
                continue;
            }
            if let Some(decoded) = decode_numeric_entity(name) {
                result.push_str(&decoded);
                base += semi_pos + 1;
                remaining = &remaining[semi_pos + 1..];
                continue;
            }
            if is_entity_shaped(name) {
                // Full `&name;` span in original string.
                unknowns.push((amp_abs, base + semi_pos + 1));
            }
        }
        // Leave the `&` as a literal. We deliberately don't advance past the
        // name — subsequent `find('&')` / final flush will handle it.
        result.push('&');
    }
    result.push_str(remaining);
    (result, unknowns)
}

/// Would `name` (the characters between `&` and `;`) look like a named HTML
/// entity? Used to decide whether an unknown reference is worth a warning.
fn is_entity_shaped(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() => {
            chars.all(|c| c.is_ascii_alphanumeric())
        }
        _ => false,
    }
}

fn decode_numeric_entity(name: &str) -> Option<String> {
    let rest = name.strip_prefix('#')?;
    let codepoint = if let Some(hex) = rest.strip_prefix('x').or_else(|| rest.strip_prefix('X')) {
        u32::from_str_radix(hex, 16).ok()?
    } else {
        rest.parse::<u32>().ok()?
    };
    char::from_u32(codepoint).map(|c| c.to_string())
}

fn decode_entity(name: &str) -> Option<&'static str> {
    // Named references — HTML4 + common HTML5 additions.
    Some(match name {
        // XML / core
        "amp"    => "&",
        "lt"     => "<",
        "gt"     => ">",
        "quot"   => "\"",
        "apos"   => "'",

        // Latin-1 supplement (HTML 4)
        "nbsp"   => "\u{00A0}",
        "iexcl"  => "\u{00A1}",
        "cent"   => "\u{00A2}",
        "pound"  => "\u{00A3}",
        "curren" => "\u{00A4}",
        "yen"    => "\u{00A5}",
        "brvbar" => "\u{00A6}",
        "sect"   => "\u{00A7}",
        "uml"    => "\u{00A8}",
        "copy"   => "\u{00A9}",
        "ordf"   => "\u{00AA}",
        "laquo"  => "\u{00AB}",
        "not"    => "\u{00AC}",
        "shy"    => "\u{00AD}",
        "reg"    => "\u{00AE}",
        "macr"   => "\u{00AF}",
        "deg"    => "\u{00B0}",
        "plusmn" => "\u{00B1}",
        "sup2"   => "\u{00B2}",
        "sup3"   => "\u{00B3}",
        "acute"  => "\u{00B4}",
        "micro"  => "\u{00B5}",
        "para"   => "\u{00B6}",
        "middot" => "\u{00B7}",
        "cedil"  => "\u{00B8}",
        "sup1"   => "\u{00B9}",
        "ordm"   => "\u{00BA}",
        "raquo"  => "\u{00BB}",
        "frac14" => "\u{00BC}",
        "frac12" => "\u{00BD}",
        "frac34" => "\u{00BE}",
        "iquest" => "\u{00BF}",
        "Agrave" => "\u{00C0}",
        "Aacute" => "\u{00C1}",
        "Acirc"  => "\u{00C2}",
        "Atilde" => "\u{00C3}",
        "Auml"   => "\u{00C4}",
        "Aring"  => "\u{00C5}",
        "AElig"  => "\u{00C6}",
        "Ccedil" => "\u{00C7}",
        "Egrave" => "\u{00C8}",
        "Eacute" => "\u{00C9}",
        "Ecirc"  => "\u{00CA}",
        "Euml"   => "\u{00CB}",
        "Igrave" => "\u{00CC}",
        "Iacute" => "\u{00CD}",
        "Icirc"  => "\u{00CE}",
        "Iuml"   => "\u{00CF}",
        "ETH"    => "\u{00D0}",
        "Ntilde" => "\u{00D1}",
        "Ograve" => "\u{00D2}",
        "Oacute" => "\u{00D3}",
        "Ocirc"  => "\u{00D4}",
        "Otilde" => "\u{00D5}",
        "Ouml"   => "\u{00D6}",
        "times"  => "\u{00D7}",
        "Oslash" => "\u{00D8}",
        "Ugrave" => "\u{00D9}",
        "Uacute" => "\u{00DA}",
        "Ucirc"  => "\u{00DB}",
        "Uuml"   => "\u{00DC}",
        "Yacute" => "\u{00DD}",
        "THORN"  => "\u{00DE}",
        "szlig"  => "\u{00DF}",
        "agrave" => "\u{00E0}",
        "aacute" => "\u{00E1}",
        "acirc"  => "\u{00E2}",
        "atilde" => "\u{00E3}",
        "auml"   => "\u{00E4}",
        "aring"  => "\u{00E5}",
        "aelig"  => "\u{00E6}",
        "ccedil" => "\u{00E7}",
        "egrave" => "\u{00E8}",
        "eacute" => "\u{00E9}",
        "ecirc"  => "\u{00EA}",
        "euml"   => "\u{00EB}",
        "igrave" => "\u{00EC}",
        "iacute" => "\u{00ED}",
        "icirc"  => "\u{00EE}",
        "iuml"   => "\u{00EF}",
        "eth"    => "\u{00F0}",
        "ntilde" => "\u{00F1}",
        "ograve" => "\u{00F2}",
        "oacute" => "\u{00F3}",
        "ocirc"  => "\u{00F4}",
        "otilde" => "\u{00F5}",
        "ouml"   => "\u{00F6}",
        "divide" => "\u{00F7}",
        "oslash" => "\u{00F8}",
        "ugrave" => "\u{00F9}",
        "uacute" => "\u{00FA}",
        "ucirc"  => "\u{00FB}",
        "uuml"   => "\u{00FC}",
        "yacute" => "\u{00FD}",
        "thorn"  => "\u{00FE}",
        "yuml"   => "\u{00FF}",

        // Latin Extended-B / special (HTML 4)
        "fnof"     => "\u{0192}",
        "Alpha"    => "\u{0391}",
        "Beta"     => "\u{0392}",
        "Gamma"    => "\u{0393}",
        "Delta"    => "\u{0394}",
        "Epsilon"  => "\u{0395}",
        "Zeta"     => "\u{0396}",
        "Eta"      => "\u{0397}",
        "Theta"    => "\u{0398}",
        "Iota"     => "\u{0399}",
        "Kappa"    => "\u{039A}",
        "Lambda"   => "\u{039B}",
        "Mu"       => "\u{039C}",
        "Nu"       => "\u{039D}",
        "Xi"       => "\u{039E}",
        "Omicron"  => "\u{039F}",
        "Pi"       => "\u{03A0}",
        "Rho"      => "\u{03A1}",
        "Sigma"    => "\u{03A3}",
        "Tau"      => "\u{03A4}",
        "Upsilon"  => "\u{03A5}",
        "Phi"      => "\u{03A6}",
        "Chi"      => "\u{03A7}",
        "Psi"      => "\u{03A8}",
        "Omega"    => "\u{03A9}",
        "alpha"    => "\u{03B1}",
        "beta"     => "\u{03B2}",
        "gamma"    => "\u{03B3}",
        "delta"    => "\u{03B4}",
        "epsilon"  => "\u{03B5}",
        "zeta"     => "\u{03B6}",
        "eta"      => "\u{03B7}",
        "theta"    => "\u{03B8}",
        "iota"     => "\u{03B9}",
        "kappa"    => "\u{03BA}",
        "lambda"   => "\u{03BB}",
        "mu"       => "\u{03BC}",
        "nu"       => "\u{03BD}",
        "xi"       => "\u{03BE}",
        "omicron"  => "\u{03BF}",
        "pi"       => "\u{03C0}",
        "rho"      => "\u{03C1}",
        "sigmaf"   => "\u{03C2}",
        "sigma"    => "\u{03C3}",
        "tau"      => "\u{03C4}",
        "upsilon"  => "\u{03C5}",
        "phi"      => "\u{03C6}",
        "chi"      => "\u{03C7}",
        "psi"      => "\u{03C8}",
        "omega"    => "\u{03C9}",
        "thetasym" => "\u{03D1}",
        "upsih"    => "\u{03D2}",
        "piv"      => "\u{03D6}",

        // General punctuation (HTML 4)
        "bull"    => "\u{2022}",
        "hellip"  => "\u{2026}",
        "prime"   => "\u{2032}",
        "Prime"   => "\u{2033}",
        "oline"   => "\u{203E}",
        "frasl"   => "\u{2044}",

        // Letterlike symbols
        "weierp"  => "\u{2118}",
        "image"   => "\u{2111}",
        "real"    => "\u{211C}",
        "trade"   => "\u{2122}",
        "alefsym" => "\u{2135}",

        // Arrows (HTML 4)
        "larr"   => "\u{2190}",
        "uarr"   => "\u{2191}",
        "rarr"   => "\u{2192}",
        "darr"   => "\u{2193}",
        "harr"   => "\u{2194}",
        "crarr"  => "\u{21B5}",
        "lArr"   => "\u{21D0}",
        "uArr"   => "\u{21D1}",
        "rArr"   => "\u{21D2}",
        "dArr"   => "\u{21D3}",
        "hArr"   => "\u{21D4}",

        // Mathematical operators (HTML 4)
        "forall"  => "\u{2200}",
        "part"    => "\u{2202}",
        "exist"   => "\u{2203}",
        "empty"   => "\u{2205}",
        "nabla"   => "\u{2207}",
        "isin"    => "\u{2208}",
        "notin"   => "\u{2209}",
        "ni"      => "\u{220B}",
        "prod"    => "\u{220F}",
        "sum"     => "\u{2211}",
        "minus"   => "\u{2212}",
        "lowast"  => "\u{2217}",
        "radic"   => "\u{221A}",
        "prop"    => "\u{221D}",
        "infin"   => "\u{221E}",
        "ang"     => "\u{2220}",
        "and"     => "\u{2227}",
        "or"      => "\u{2228}",
        "cap"     => "\u{2229}",
        "cup"     => "\u{222A}",
        "int"     => "\u{222B}",
        "there4"  => "\u{2234}",
        "sim"     => "\u{223C}",
        "cong"    => "\u{2245}",
        "asymp"   => "\u{2248}",
        "ne"      => "\u{2260}",
        "equiv"   => "\u{2261}",
        "le"      => "\u{2264}",
        "ge"      => "\u{2265}",
        "sub"     => "\u{2282}",
        "sup"     => "\u{2283}",
        "nsub"    => "\u{2284}",
        "sube"    => "\u{2286}",
        "supe"    => "\u{2287}",
        "oplus"   => "\u{2295}",
        "otimes"  => "\u{2297}",
        "perp"    => "\u{22A5}",
        "sdot"    => "\u{22C5}",

        // Miscellaneous technical
        "lceil"  => "\u{2308}",
        "rceil"  => "\u{2309}",
        "lfloor" => "\u{230A}",
        "rfloor" => "\u{230B}",
        "lang"   => "\u{2329}",
        "rang"   => "\u{232A}",

        // Geometric shapes
        "loz"    => "\u{25CA}",

        // Miscellaneous symbols
        "spades" => "\u{2660}",
        "clubs"  => "\u{2663}",
        "hearts" => "\u{2665}",
        "diams"  => "\u{2666}",

        // Latin Extended-A / special (HTML 4)
        "OElig"  => "\u{0152}",
        "oelig"  => "\u{0153}",
        "Scaron" => "\u{0160}",
        "scaron" => "\u{0161}",
        "Yuml"   => "\u{0178}",
        "circ"   => "\u{02C6}",
        "tilde"  => "\u{02DC}",

        // General punctuation (HTML 4 / common HTML5)
        "ensp"   => "\u{2002}",
        "emsp"   => "\u{2003}",
        "thinsp" => "\u{2009}",
        "zwnj"   => "\u{200C}",
        "zwj"    => "\u{200D}",
        "lrm"    => "\u{200E}",
        "rlm"    => "\u{200F}",
        "ndash"  => "\u{2013}",
        "mdash"  => "\u{2014}",
        "lsquo"  => "\u{2018}",
        "rsquo"  => "\u{2019}",
        "sbquo"  => "\u{201A}",
        "ldquo"  => "\u{201C}",
        "rdquo"  => "\u{201D}",
        "bdquo"  => "\u{201E}",
        "dagger" => "\u{2020}",
        "Dagger" => "\u{2021}",
        "permil" => "\u{2030}",
        "lsaquo" => "\u{2039}",
        "rsaquo" => "\u{203A}",
        "euro"   => "\u{20AC}",

        _ => return None,
    })
}

/// Collapse JSX text whitespace using the same rules as Babel/React:
///
/// 1. Replace tabs with spaces.
/// 2. Split on newlines.
/// 3. Trim trailing whitespace from every line except the last.
///    Trim leading whitespace from every line except the first.
/// 4. Drop lines that are empty after step 3.
/// 5. Join surviving lines with a single space.
/// 6. Return `None` if the result is empty (caller should drop the node).
pub fn collapse_jsx_whitespace(s: &str) -> Option<String> {
    let s = s.replace('\t', " ");
    let lines: Vec<&str> = s.split('\n').collect();
    let n = lines.len();
    let mut parts: Vec<String> = Vec::new();

    for (i, line) in lines.iter().enumerate() {
        // Trim leading on every line except the first;
        // trim trailing on every line except the last.
        // When there is only one line (i == 0 == n-1), neither trim applies.
        let line = if i > 0 { line.trim_start_matches(' ') } else { line };
        let line = if i < n - 1 { line.trim_end_matches(' ') } else { line };
        if !line.is_empty() {
            parts.push(line.to_string());
        }
    }

    if parts.is_empty() {
        None
    } else {
        Some(parts.join(" "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_no_entities() {
        assert_eq!(decode_html_entities("hello world"), "hello world");
    }

    #[test]
    fn test_amp() {
        assert_eq!(decode_html_entities("a &amp; b"), "a & b");
    }

    #[test]
    fn test_core_entities() {
        assert_eq!(decode_html_entities("&lt;&gt;&quot;&apos;"), "<>\"'");
    }

    #[test]
    fn test_numeric_decimal() {
        assert_eq!(decode_html_entities("&#60;"), "<");
        assert_eq!(decode_html_entities("&#34;"), "\"");
    }

    #[test]
    fn test_numeric_hex() {
        assert_eq!(decode_html_entities("&#x3C;"), "<");
        assert_eq!(decode_html_entities("&#X3E;"), ">");
    }

    #[test]
    fn test_url_ampersand() {
        assert_eq!(decode_html_entities("/q?a=1&amp;b=2"), "/q?a=1&b=2");
    }

    #[test]
    fn test_unknown_entity_left_unchanged() {
        assert_eq!(decode_html_entities("&notanentity;"), "&notanentity;");
    }

    #[test]
    fn test_bare_ampersand_left_unchanged() {
        assert_eq!(decode_html_entities("a & b"), "a & b");
    }

    // --- collapse_jsx_whitespace ---

    #[test]
    fn test_collapse_inline_preserves_trailing_space() {
        assert_eq!(collapse_jsx_whitespace("Hello, "), Some("Hello, ".into()));
    }

    #[test]
    fn test_collapse_inline_preserves_leading_space() {
        assert_eq!(collapse_jsx_whitespace(" world"), Some(" world".into()));
    }

    #[test]
    fn test_collapse_whitespace_only_between_elements() {
        assert_eq!(collapse_jsx_whitespace("\n  "), None);
        assert_eq!(collapse_jsx_whitespace("\n    \n"), None);
    }

    #[test]
    fn test_collapse_multiline_joins_with_space() {
        assert_eq!(
            collapse_jsx_whitespace("\n  Hello\n  World\n"),
            Some("Hello World".into()),
        );
    }

    #[test]
    fn test_collapse_single_line_no_trim() {
        // Single line = both first and last: neither leading nor trailing is trimmed.
        assert_eq!(collapse_jsx_whitespace("  text  "), Some("  text  ".into()));
    }

    #[test]
    fn test_collapse_tabs_replaced() {
        assert_eq!(collapse_jsx_whitespace("A\tB"), Some("A B".into()));
    }

    #[test]
    fn test_collapse_empty_string() {
        assert_eq!(collapse_jsx_whitespace(""), None);
    }

    #[test]
    fn test_common_typography() {
        assert_eq!(decode_html_entities("&mdash;"), "\u{2014}");
        assert_eq!(decode_html_entities("&ndash;"), "\u{2013}");
        assert_eq!(decode_html_entities("&hellip;"), "\u{2026}");
        assert_eq!(decode_html_entities("&ldquo;&rdquo;"), "\u{201C}\u{201D}");
    }

    // --- decode_html_entities_with_unknowns ---

    #[test]
    fn test_unknowns_none_for_known_entities() {
        let (out, unk) = decode_html_entities_with_unknowns("a &amp; b &lt; c");
        assert_eq!(out, "a & b < c");
        assert!(unk.is_empty());
    }

    #[test]
    fn test_unknowns_single_unknown_named_entity() {
        let (out, unk) = decode_html_entities_with_unknowns("x &asdf; y");
        // Unknown entity is left as-is in the output.
        assert_eq!(out, "x &asdf; y");
        assert_eq!(unk, vec![(2, 8)]);
        assert_eq!(&"x &asdf; y"[2..8], "&asdf;");
    }

    #[test]
    fn test_unknowns_mixed_known_and_unknown() {
        let src = "&amp; &foo; &lt; &bar;";
        let (out, unk) = decode_html_entities_with_unknowns(src);
        assert_eq!(out, "& &foo; < &bar;");
        assert_eq!(unk.len(), 2);
        assert_eq!(&src[unk[0].0..unk[0].1], "&foo;");
        assert_eq!(&src[unk[1].0..unk[1].1], "&bar;");
    }

    #[test]
    fn test_unknowns_skip_non_entity_shaped() {
        // Bare `&`, empty `&;`, digit-start, underscore inside, no semicolon:
        // none of these should produce a warning.
        for s in [
            "a & b",          // bare &
            "a &; b",         // empty name
            "a &123; b",      // starts with digit
            "a &abc_def; b",  // underscore is not [A-Za-z0-9]
            "a &abc b",       // no closing semicolon within window
            "a &#xyz; b",     // numeric-looking malformed
        ] {
            let (_, unk) = decode_html_entities_with_unknowns(s);
            assert!(unk.is_empty(), "expected no unknowns for {:?}, got {:?}", s, unk);
        }
    }

    #[test]
    fn test_unknowns_back_to_back() {
        let src = "&foo;&bar;";
        let (_out, unk) = decode_html_entities_with_unknowns(src);
        assert_eq!(unk, vec![(0, 5), (5, 10)]);
    }

    #[test]
    fn test_unknowns_with_multibyte_prefix() {
        // Multi-byte UTF-8 before the entity must not shift the byte offset.
        let src = "café &asdf; q";
        let (_out, unk) = decode_html_entities_with_unknowns(src);
        assert_eq!(unk.len(), 1);
        assert_eq!(&src[unk[0].0..unk[0].1], "&asdf;");
    }

    #[test]
    fn test_unknowns_numeric_entities_are_not_flagged() {
        // Valid numeric refs decode and don't appear in unknowns.
        let (out, unk) = decode_html_entities_with_unknowns("&#60; &#x3E;");
        assert_eq!(out, "< >");
        assert!(unk.is_empty());
    }
}
