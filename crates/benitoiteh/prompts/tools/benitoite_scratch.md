Write Benitoite code to a new temporary script in the session temporary directory, check it, and run it in the sandbox.
benitoiteh runs it without asking when its effects stay within the automatic set (no effects, Console.Write, Log, State, Process.Exit, and any the user added); otherwise the user confirms them as for benitoite_run. You do not choose the allowed effects.
Log is automatic by default. File.Write and other sink effects still need their own permission.
The script's root is the session temporary directory, so `import` cannot load project modules. The result shows the script's path; read it and write it into the project to keep it.
Pass script arguments in `args` and standard input in `stdin` (empty otherwise). The time limit is 120 seconds.
