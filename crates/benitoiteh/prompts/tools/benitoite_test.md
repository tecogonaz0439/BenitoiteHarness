Run Benitoite tests in .bnt files or directories in the sandbox after benitoiteh inspects their effects and asks the user to confirm them.
Check them with benitoite_check with `tests` set to true first. You do not choose the allowed effects: benitoiteh passes the union of the selected tests' effects, including Assert.Check.
A directory runs every .bnt file under it except package.bnt and nested packages.
Documentation examples (`--doc`) are not included.
