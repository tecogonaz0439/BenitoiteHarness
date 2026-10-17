# Benitoite.Network.Http

Status: unofficial. Import it with `import Benitoite.Unofficial.Network.Http`. When the module becomes standard, the import name changes to `import Benitoite.Network.Http`.

Refer to its declarations as `Http.<name>`, for example `Http.Listen`.

HTTP/1.1 servers without TLS and clients for http and https URLs.
The resource types `Listener` and `Exchange` are built in.
The server has no limit on the number of connections or the time a client may take to send a request.
Listen on 127.0.0.1, or put the server behind a reverse proxy; do not expose it to the internet directly.

```benitoite
public effect Listen
  /// Starts listening on `port` of `host`. Port 0 lets the operating system choose a free port.
  function listen(host: String, port: Integer) -> Result[Listener, NetworkError]
  /// Returns the port the listener listens on.
  function listenerPort(listener: Listener) -> Integer
  /// Waits for the next request and accepts it. Other tasks run while it waits.
  function accept(listener: Listener) -> Result[Exchange, NetworkError]
  /// Sends the response. Stops the program when called twice for one exchange.
  /// The status must be between 200 and 599. Sends no body for HEAD requests,
  /// statuses 204, 205, and 304, or 2xx responses to CONNECT requests.
  function respond(exchange: Exchange, response: Response) -> Result[Unit, NetworkError]
end effect
```

Listening for connections, accepting requests, and responding.

```benitoite
public effect Connect
  /// Sends the request and returns the response, including 4xx and 5xx responses.
  function send(request: ClientRequest) -> Result[Response, NetworkError]
end effect
```

Connecting to HTTP servers and sending requests.

```benitoite
public record Request
  method: String
  path: String
  query: List[Pair[String, String]]
  headers: List[Pair[String, String]]
  body: Bytes
end record
```

A request received by a server. Header names are in lowercase, in the order received.

```benitoite
public record Response
  status: Integer
  headers: List[Pair[String, String]]
  body: Bytes
end record
```

A response sent by a server or received by a client.

```benitoite
public record ClientRequest
  method: String
  url: String
  headers: List[Pair[String, String]]
  body: Bytes
  timeoutMilliseconds: Integer
end record
```

A request sent by a client, made with `Http.clientRequest` and changed with record update.

```benitoite
public function requestOf(exchange: Exchange) -> Request
```

Returns the request that was accepted. Stops the program when the exchange is closed or being closed.

```benitoite
public function closeListener(listener: Listener) -> Result[Unit, NetworkError] uses State
```

Stops listening. A `with` binding closes the listener in the same way.

```benitoite
public function closeExchange(exchange: Exchange) -> Result[Unit, NetworkError] uses State
```

Sends status 500 when no response was sent, then closes the connection.
A `with` binding closes the exchange in the same way, ignoring failures.

```benitoite
public function pathSegments(path: String) -> List[String]
```

Splits `path` at `/`, drops empty parts, and decodes percent-encoding where the result is valid UTF-8.

```benitoite
public function serve[effect E](host: String, port: Integer, handler: function(request: Request) -> Response uses E) -> Result[Unit, NetworkError] uses Listen, State, E
```

Listens on `port` of `host` and runs `handler` in a new task for each request.
Returns `Result.Error` when listening cannot start or accepting fails.

```benitoite
public function text(status: Integer, s: String) -> Response
```

Returns a response whose body is `s` as plain text.

```benitoite
public function html(status: Integer, s: String) -> Response
```

Returns a response whose body is `s` as HTML.

```benitoite
public function json(status: Integer, value: Json.Value) -> Response
```

Returns a response whose body is `value` written by `Json.stringify`.

```benitoite
public function header(headers: List[Pair[String, String]], name: String) -> Option[String]
```

Returns the value of the first header whose name equals `name`, ignoring case.

```benitoite
public function clientRequest(method: String, url: String) -> ClientRequest
```

Returns a request with no headers, an empty body, and a timeout of 30000 milliseconds.

```benitoite
public function get(url: String) -> Result[Response, NetworkError] uses Connect
```

Sends a GET request to `url`, as `Http.send(Http.clientRequest(method: "GET", url: url))`.
