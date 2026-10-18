# Sort with an external command

Write a Benitoite script `main.bnt` that reads `fruits.txt` in the current directory (one word per line) and sorts the words with the external command `sort -u`.

Start `sort` directly with a list of arguments (not through a shell). Give it the content of `fruits.txt` on standard input, and set the environment variable `LC_ALL` to `C` for the command so that the order does not depend on the locale (uppercase letters come before lowercase letters).

Print each line of the output of `sort`, then:

```text
unique: <number of lines printed>
sort exit code: <exit code of sort>
```
