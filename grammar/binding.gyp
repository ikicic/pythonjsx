{
  "targets": [
    {
      "target_name": "tree_sitter_pythonjsx_binding",
      "include_dirs": [
        "<!(node -e \"require('nan')\")",
        "src"
      ],
      "sources": [
        "src/parser.c",
        "bindings/node/binding.cc"
      ],
      "cflags_c": [
        "-std=c11",
      ],
      "conditions": [
        ["OS=='win'", {
          "sources": [
            "src/scanner.cc"
          ]
        }, {
          "sources": [
            "src/scanner.c"
          ]
        }]
      ]
    }
  ]
}
