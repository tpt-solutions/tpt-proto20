# Examples

- [`hello-rpc`](hello-rpc) — schema → generated Rust → a `Greeter` service
  served over HTTP/2 and called with the generated client (unary +
  server-streaming, metadata, deadline, error status), plus the JSON, wire and
  text-format views of a message.

  ```sh
  cargo run -p hello-rpc
  ```

  The same generation step from the command line:
  `tpt20 gen rust --in examples/hello-rpc/greeter.tpt --out /tmp/generated`.
