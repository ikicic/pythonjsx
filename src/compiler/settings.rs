//! Compiler settings and configuration.

use std::collections::HashSet;

fn builtin_html_tags() -> HashSet<&'static str> {
    const TAGS: &[&str] = &[
        "a", "abbr", "address", "area", "article", "aside", "audio",
        "b", "base", "bdi", "bdo", "blockquote", "body", "br", "button",
        "canvas", "caption", "cite", "code", "col", "colgroup",
        "data", "datalist", "dd", "del", "details", "dfn", "dialog",
        "div", "dl", "dt", "em", "embed",
        "fieldset", "figcaption", "figure", "footer", "form",
        "h1", "h2", "h3", "h4", "h5", "h6", "head", "header", "hr", "html",
        "i", "iframe", "img", "input", "ins", "kbd",
        "label", "legend", "li", "link",
        "main", "map", "mark", "meta", "meter",
        "nav", "noscript",
        "object", "ol", "optgroup", "option", "output",
        "p", "param", "picture", "pre", "progress",
        "q", "rp", "rt", "ruby",
        "s", "samp", "script", "section", "select", "small", "source", "span",
        "strong", "style", "sub", "summary", "sup",
        "table", "tbody", "td", "template", "textarea", "tfoot", "th", "thead",
        "time", "title", "tr", "track",
        "u", "ul", "var", "video", "wbr",
    ];
    TAGS.iter().copied().collect()
}

/// HTML5 void elements (no content, no end tag).
/// See <https://html.spec.whatwg.org/multipage/syntax.html#void-elements>.
fn void_html_tags() -> HashSet<&'static str> {
    const TAGS: &[&str] = &[
        "area", "base", "br", "col", "embed", "hr", "img", "input",
        "link", "meta", "source", "track", "wbr",
    ];
    TAGS.iter().copied().collect()
}

#[derive(Clone)]
pub struct CompilerSettings {
    pub builtin_html_tags: HashSet<&'static str>,
    pub void_html_tags: HashSet<&'static str>,
    pub fragment_function: String,
    pub import_alias: String,
}

impl Default for CompilerSettings {
    fn default() -> Self {
        Self {
            builtin_html_tags: builtin_html_tags(),
            void_html_tags: void_html_tags(),
            fragment_function: "fragment".to_string(),
            import_alias: "pjr".to_string(),
        }
    }
}

impl CompilerSettings {
    pub fn is_builtin_tag(&self, tag: &str) -> bool {
        self.builtin_html_tags.contains(tag)
    }

    pub fn is_void_tag(&self, tag: &str) -> bool {
        self.void_html_tags.contains(tag)
    }
}
