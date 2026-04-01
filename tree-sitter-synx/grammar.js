/// tree-sitter grammar for SYNX — The Active Data Format
/// https://github.com/APERTURESyndicate/synx-format

module.exports = grammar({
  name: "synx",

  extras: $ => [/\r/],

  rules: {
    document: $ => repeat(choice(
      $.shebang_mode,
      $.directive,
      $.comment,
      $.pair,
      $.list_item,
      $.blank_line,
    )),

    blank_line: _ => /[ \t]*\n/,

    // #!mode:active | #!mode:static (must appear before generic # comments)
    shebang_mode: _ => seq(/[ \t]*/, "#!mode:", /[^\n]*/, /\n/),

    // Any line starting with ! after indentation: !active, !llm, !include path, !tool, …
    directive: _ => seq(/[ \t]*/, /![^\n]+/, /\n/),

    // Comments: # ... or // ...  (lines starting with #!mode: are shebang_mode — listed first in document)
    comment: _ => seq(
      /[ \t]*/,
      choice(
        seq("#", /[^\n]*/),
        seq("//", /[^\n]*/),
      ),
      /\n/,
    ),

    // Key-value pair: key value  (first space separates)
    pair: $ => seq(
      /[ \t]*/,
      $.key,
      optional(seq(
        / /,
        $.value,
      )),
      /\n/,
    ),

    // Keys may have type casts, markers, and constraints:
    //   key_name(type)[constraints]:marker:arg VALUE
    key: $ => seq(
      $.key_name,
      optional($.type_cast),
      optional($.constraints),
      repeat($.marker),
    ),

    key_name: _ => /[a-zA-Z_][a-zA-Z0-9_.]*/,

    // (int), (float), (bool), (string)
    type_cast: _ => seq("(", /[a-z]+/, ")"),

    // [min:1, max:100, required]
    constraints: _ => seq("[", /[^\]\n]+/, "]"),

    // :marker (chained markers are multiple marker nodes in repeat($.marker) on key)
    marker: _ => seq(":", /[a-zA-Z_][a-zA-Z0-9_]*/),

    // List item: - value
    list_item: $ => seq(
      /[ \t]*/,
      "- ",
      $.value,
      /\n/,
    ),

    // Values: everything after the first space until newline
    value: _ => /[^\n]+/,
  },
});
