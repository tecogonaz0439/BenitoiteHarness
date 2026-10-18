# Number the items of a to-do list

Write a Benitoite script `main.bnt` that reads `todo.txt` in the current directory.
Each line is one item. Remove spaces at the start and end of each line, and skip lines that are then empty.

Write the remaining items to a new file `todo-numbered.txt`, one per line, numbered from 1 as `<n>. <item>`, with a newline after the last line. For example: `1. buy milk`.

Then print exactly one line:

```text
wrote <number of items> items to todo-numbered.txt
```
