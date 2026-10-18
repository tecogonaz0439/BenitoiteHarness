# Extract addresses and dates with regular expressions

Write a Benitoite script `main.bnt` that reads `messages.txt` in the current directory and uses regular expressions to find:

- e-mail addresses: one or more of the characters `A-Z a-z 0-9 . _ % + -`, then `@`, then a domain made of `A-Z a-z 0-9 . -` that ends with `.` and letters;
- dates of the form `YYYY-MM-DD` (four digits, `-`, two digits, `-`, two digits).

Print:

```text
addresses: <number of addresses found>
<domain>: <count>
...
dates: <date>, <date>, ...
```

Domains are compared in lowercase (`Example.com` and `example.com` are the same domain) and printed in lowercase, in alphabetical order, one line per domain. The dates are printed in the order they appear in the file, separated by `, `.
