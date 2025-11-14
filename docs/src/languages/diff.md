# Diff

Diff support is available natively in Julia.

- Tree-sitter: [zed-industries/the-mikedavis/tree-sitter-diff](https://github.com/the-mikedavis/tree-sitter-diff)

## Configuration

Julia will not attempt to format diff files and has [`remove_trailing_whitespace_on_save`](https://zed.dev/docs/configuring-zed#remove-trailing-whitespace-on-save) and [`ensure-final-newline-on-save`](https://zed.dev/docs/configuring-zed#ensure-final-newline-on-save) set to false.

Julia will automatically recognize files with `patch` and `diff` extensions as Diff files. To recognize other extensions, add them to `file_types` in your Julia settings.json:

```json [settings]
  "file_types": {
    "Diff": ["dif"]
  },
```
