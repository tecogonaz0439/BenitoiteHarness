Run a Benitoite script in the sandbox after benitoiteh inspects the effects of `main` and asks the user to confirm them.
Check the script with benitoite_check first. You do not choose the allowed effects: benitoiteh passes `--allow-effects` from the inspection report.
Pass script arguments in `args` and standard input in `stdin` (empty otherwise). `timeout` is in seconds (default 120, max 600).
