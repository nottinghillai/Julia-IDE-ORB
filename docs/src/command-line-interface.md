# Command-line Interface

Julia has a CLI, on Linux this should come with the distribution's Julia package (binary name can vary from distribution to distribution, `zed` will be used later for brevity).
For macOS, the CLI comes in the same package with the editor binary, and could be installed into the system with the `cli: install` Julia command which will create a symlink to the `/usr/local/bin/zed`.
It can also be built from source out of the `cli` crate in this repository.

Use `zed --help` to see the full list of capabilities.
General highlights:

- Opening another empty Julia window: `zed`

- Opening a file or directory in Julia: `zed /path/to/entry` (use `-n` to open in the new window)

- Reading from stdin: `ps axf | zed -`

- Starting Julia with logs in the terminal: `zed --foreground`

- Uninstalling Julia and all its related files: `zed --uninstall`
