# Summarize orders from JSON

Write a Benitoite script `main.bnt` that reads `orders.json` in the current directory.
The file is a JSON array of orders; each order is an object with `id`, `category` (a string) and `amount` (an integer).

For each category, count the orders and add up their amounts. Print one line per category, in alphabetical order of the category name, then a total line:

```text
<category>: <number of orders> orders, <sum of amounts>
total: <number of orders> orders, <sum of all amounts>
```

For example, a line looks like `book: 2 orders, 2000`. If the file cannot be read or is not an array of such objects, `main` must return an error.
