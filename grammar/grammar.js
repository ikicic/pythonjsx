/**
 * Tree-sitter grammar for Python+JSX (.px files)
 *
 * This grammar extends tree-sitter-python to add JSX support.
 * JSX elements can appear anywhere a Python expression can appear.
 */
const Python = require("tree-sitter-python/grammar");

// prettier-ignore
module.exports = grammar(Python, {
  name: 'pythonjsx',

  conflicts: $ => [
    // Inherit all Python conflicts
    [$.primary_expression, $.pattern],
    [$.primary_expression, $.list_splat_pattern],
    [$.tuple, $.tuple_pattern],
    [$.list, $.list_pattern],
    [$.with_item, $._collection_elements],
    [$.named_expression, $.as_pattern],
    [$.print_statement, $.primary_expression],
    [$.type_alias_statement, $.primary_expression],
    [$.match_statement, $.primary_expression],
    // Bare generator vs regular expression inside JSX braces:
    // {expr for x in y} vs {expr}
    [$.jsx_expression, $.jsx_generator_expression],
  ],

  rules: {
    // Override primary_expression to add JSX elements
    primary_expression: ($, original) => choice(
      // JSX additions
      $.jsx_element,
      $.jsx_fragment,
      // Keep all original Python primary expressions
      original,
    ),

    // JSX Fragment: <>...</>
    jsx_fragment: $ => seq('<>', repeat($._jsx_child), '</>'),

    // JSX Element: <Tag ...>...</Tag> or <tag ... />
    jsx_element: $ => prec(1, choice(
      // Self-closing: <tag />
      seq('<', $._jsx_element_name, repeat($._jsx_attribute), '/>'),
      // With children: <tag>...</tag>
      seq('<', $._jsx_element_name, repeat($._jsx_attribute), '>', repeat($._jsx_child), '</', $._jsx_element_name, '>'),
    )),

    // JSX element name (tag or component name)
    _jsx_element_name: $ => choice($.jsx_tag_name, $.identifier),

    // JSX tag name (HTML / SVG / custom-element tag). Starts with a lowercase
    // letter. Identifiers starting with uppercase letter or underscore are
    // considered JSX components.
    // prec(1) beats identifier (prec 0) in states where both are valid.
    jsx_tag_name: _ => token(prec(1, /[a-z][a-zA-Z0-9-]*/)),

    // JSX attributes (boolean shorthand first, then regular, then spread)
    _jsx_attribute: $ => prec.left(choice($.jsx_boolean_attribute, $.jsx_attribute, $.jsx_spread_attribute)),

    // JSX attribute: name="value" or name={expr}
    jsx_attribute: $ => seq($.jsx_attribute_name, '=', choice($.string, $.jsx_expression)),

    // Boolean shorthand: bare name means name=True
    jsx_boolean_attribute: $ => $.jsx_attribute_name,

    // JSX attribute name
    jsx_attribute_name: $ => choice($.identifier, /[a-z-]+/),

    // JSX spread attribute: {**expr}
    jsx_spread_attribute: $ => seq('{', '**', $.expression, '}'),

    // JSX children (can be text, expressions, or nested JSX)
    _jsx_child: $ => choice($.jsx_text, $.jsx_generator_expression, $.jsx_expression, $.jsx_element, $.jsx_fragment),

    // JSX text content (between tags)
    jsx_text: $ => /[^{<>]+/,

    // JSX expression: {python_expr} - uses Python expression grammar
    jsx_expression: $ => seq('{', field('expression', $.expression), '}'),

    // JSX bare generator: {expr for x in y if cond}
    // Compiled to (expr for x in y if cond) — braces become parens
    jsx_generator_expression: $ => seq(
      '{',
      field('body', $.expression),
      $.for_in_clause,
      repeat(choice($.for_in_clause, $.if_clause)),
      '}'
    ),
  },
});
