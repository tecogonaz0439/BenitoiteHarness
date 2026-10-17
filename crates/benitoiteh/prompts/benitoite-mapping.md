Use benitoite_check for `benitoite check`, benitoite_check with `tests` set to true for `benitoite test --check`, benitoite_run for `benitoite run`, and benitoite_test for `benitoite test` in the Skill below.
Read `references/<name>.md` with benitoite_reference, passing `<name>` as `name` (for example, `idioms` or `stdlib/List`).
Use benitoite_explain to look up a diagnostic code.
Skip the "Check the version first" section, including `skill list` and `skill show`: benitoiteh has already checked the processor at startup, and benitoite_reference reads the Skill documents.
benitoite_run and benitoite_test obtain the inspection report and pass `--allow-effects` themselves; you do not read the report or choose the allowed effects.
benitoiteh handles the "Before running" confirmation in its UI; you do not need to ask the user for confirmation yourself. You may explain what the effects mean in your response.
