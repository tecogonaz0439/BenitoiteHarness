# Revenue per region from CSV

Write a Benitoite script `main.bnt` that reads `sales.csv` in the current directory.
It is a CSV file with the header `region,item,quantity,unit_price`. Some fields are quoted and contain commas or quotes.

For each region, count the rows and compute the revenue (the sum of `quantity * unit_price`).

1. Write `summary.csv` with the header `region,orders,revenue` and one row per region, in alphabetical order of the region. End every row, including the last, with a newline.
2. Print one line per region in the same order, then the total:

```text
<region>: <rows> orders, revenue <revenue>
total revenue: <sum of all revenue>
```

If a quantity or a price is not an integer, `main` must return an error.
