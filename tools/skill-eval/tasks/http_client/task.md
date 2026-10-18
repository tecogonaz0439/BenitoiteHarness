# Read an inventory from an HTTP server

An HTTP server is running on this machine. Its base URL (such as `http://127.0.0.1:54321`, without a trailing `/`) is in the environment variable `SKILL_EVAL_HTTP_BASE`. Read it with `Process.environmentVariable`; do not hard-code a port. If the variable is not set, `main` must return an error.

The server answers:

- `GET /status`: the text `ok`.
- `GET /inventory`: a JSON array of objects with `name` (string), `stock` (integer) and `price` (integer).
- `POST /report`: send a JSON array as the request body; the server answers with status 201 and a short text.

Write a Benitoite script `main.bnt` that:

1. Prints `status: <body of GET /status>`.
2. Reads the inventory and prints `low stock: <name> (<stock>), <name> (<stock>), ...` for the items whose stock is less than 5, in the order of the inventory.
3. Prints `inventory value: <sum of stock * price over all items>`.
4. Sends the names of the low-stock items as a JSON array of strings (for example `["a","b"]`) to `POST /report`, and prints `report: <status code> <response body>`.
