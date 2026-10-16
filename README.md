# BenitoiteHarness

BenitoiteHarness is a prototype of a coding-agent harness for [Benitoite](https://github.com/tecogonaz0439/Benitoite), a statically typed functional scripting language. The executable is tentatively named `benitoiteh`.

The harness has two modes:

- **Restricted mode**: the agent may only write, check, and run Benitoite scripts and read files in the project. It has no shell tool and cannot start interpreters of other scripting languages (sh, Python, ...).
- **Normal mode**: the agent behaves like an ordinary coding-agent harness, with a shell tool and file-editing tools, inside an OS sandbox.

## Status

Design phase. The design document (in Japanese) is in [`docs/design/`](docs/design/README.md). No code has been written yet.

## License

Licensed under either of the [Apache License, Version 2.0](LICENSE-APACHE) or the [MIT License](LICENSE-MIT), at your option (`MIT OR Apache-2.0`).

Copyright (c) 2026 tecogonaz and BenitoiteHarness contributors
