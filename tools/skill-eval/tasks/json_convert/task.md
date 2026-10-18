# Convert CSV to JSON

Write a Benitoite script `main.bnt` that reads `members.csv` in the current directory. It has the header `name,team,joined` (`joined` is a year).

1. Group the member names by team and write `teams.json`: one JSON object whose keys are the team names and whose values are arrays of the member names of that team, sorted alphabetically. Write it compactly (no spaces), with the keys in alphabetical order, followed by a newline. For example: `{"a":["X","Y"],"b":["Z"]}`.
2. Print:

```text
teams: <number of teams>, members: <number of members>
oldest member: <name> (<joined>)
```

The oldest member is the one with the smallest `joined` year.
