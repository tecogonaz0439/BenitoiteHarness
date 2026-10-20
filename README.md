# BenitoiteHarness

BenitoiteHarness is a prototype of a coding-agent harness for [Benitoite](https://github.com/tecogonaz0439/Benitoite), a statically typed functional scripting language. The executable is tentatively named `benitoiteh`.

The harness has two modes:

- **Restricted mode**: the agent may only write, check, and run Benitoite scripts and read files in the project. It has no shell tool and cannot start interpreters of other scripting languages (sh, Python, ...).
- **Normal mode**: the agent behaves like an ordinary coding-agent harness, with a shell tool and file-editing tools, inside an OS sandbox.

## Status

The prototype is implemented through P5-35, including the evaluation tools. The compaction evaluation (BH-014, P5-36) was deferred by the designer on 2026-10-21; some design decisions remain open. See the [design document (in Japanese)](docs/design/README.md), [implementation plan and task status](docs/plan/README.md), and [evaluation summaries](tools/skill-eval/results/).

The [documentation index](docs/README.md) also links to the shared development workflow. For security reports, see [SECURITY.md](SECURITY.md).

## License

Licensed under either of the [Apache License, Version 2.0](LICENSE-APACHE) or the [MIT License](LICENSE-MIT), at your option (`MIT OR Apache-2.0`).

Copyright (c) 2026 tecogonaz and BenitoiteHarness contributors
