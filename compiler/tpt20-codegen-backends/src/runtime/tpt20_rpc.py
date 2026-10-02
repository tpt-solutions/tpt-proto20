"""tpt20 minimal Python RPC runtime: asyncio client and server over HTTP/2.

Generated together with the message code; do not edit. Needs the `h2` package
(`pip install h2`). Speaks the protocol the Rust runtime speaks: cleartext
HTTP/2 (prior knowledge), POST /<service>/<method>, messages framed as
flags(1) | length(4, big endian) | payload, final status in the `grpc-status` /
`grpc-message` trailers, deadline in `grpc-timeout`.
"""
import asyncio
import base64
import struct
import time

import h2.config
import h2.connection
import h2.errors
import h2.events
import h2.exceptions

OK, CANCELLED, UNKNOWN, INVALID_ARGUMENT, DEADLINE_EXCEEDED, NOT_FOUND = 0, 1, 2, 3, 4, 5
ALREADY_EXISTS, PERMISSION_DENIED, RESOURCE_EXHAUSTED, FAILED_PRECONDITION = 6, 7, 8, 9
ABORTED, OUT_OF_RANGE, UNIMPLEMENTED, INTERNAL, UNAVAILABLE, DATA_LOSS = 10, 11, 12, 13, 14, 15
UNAUTHENTICATED = 16

DEFAULT_MAX_MESSAGE_BYTES = 4 * 1024 * 1024


class RpcError(Exception):
    """A failed call: a status code and message."""

    def __init__(self, code, message=""):
        super().__init__("tpt20 rpc: code %d: %s" % (code, message))
        self.code = code
        self.message = message


_RESERVED = {
    "grpc-status", "grpc-message", "grpc-timeout", "content-type", "te", "grpc-encoding",
    "grpc-accept-encoding", "traceparent", "tracestate", "user-agent", "accept-encoding",
    "content-length", "trailer",
}


class Metadata(dict):
    """Call metadata: lower-case keys to lists of values. Keys ending in
    `-bin` carry binary values (use set_bin/get_bin)."""

    def set(self, key, value):
        self[key.lower()] = [value]

    def get_first(self, key, default=None):
        values = dict.get(self, key.lower())
        return values[0] if values else default

    def set_bin(self, key, value):
        self.set(key, base64.b64encode(value).decode("ascii"))

    def get_bin(self, key):
        v = self.get_first(key)
        return None if v is None else base64.b64decode(v)


def _metadata_from(headers):
    md = Metadata()
    for k, v in headers:
        k = k.lower()
        if k in _RESERVED or k.startswith(":"):
            continue
        md.setdefault(k, []).append(v)
    return md


def percent_encode(s):
    out = []
    for b in s.encode("utf-8"):
        if 0x20 <= b <= 0x7E and b != 0x25:
            out.append(chr(b))
        else:
            out.append("%%%02X" % b)
    return "".join(out)


def percent_decode(s):
    raw = s.encode("utf-8")
    out = bytearray()
    i = 0
    while i < len(raw):
        if raw[i] == 0x25 and i + 2 < len(raw):
            try:
                out.append(int(raw[i + 1:i + 3], 16))
                i += 3
                continue
            except ValueError:
                pass
        out.append(raw[i])
        i += 1
    return out.decode("utf-8", "replace")


def encode_timeout(seconds):
    nanos = max(0, int(seconds * 1e9))
    for unit, per in (("n", 1), ("u", 1000), ("m", 1_000_000), ("S", 10**9), ("M", 60 * 10**9)):
        n = -(-nanos // per)
        if n <= 99_999_999:
            return "%d%s" % (n, unit)
    return "%dH" % min(-(-nanos // (3600 * 10**9)), 99_999_999)


def decode_timeout(s):
    s = s.strip()
    if len(s) < 2 or len(s) > 9 or not s[:-1].isdigit():
        return None
    per = {"n": 1e-9, "u": 1e-6, "m": 1e-3, "S": 1.0, "M": 60.0, "H": 3600.0}.get(s[-1])
    return None if per is None else int(s[:-1]) * per


def frame(payload):
    return b"\x00" + struct.pack(">I", len(payload)) + payload


class _FrameParser:
    """Reassembles length-prefixed messages from DATA chunks."""

    def __init__(self, max_bytes):
        self.buf = bytearray()
        self.max = max_bytes

    def feed(self, data):
        self.buf += data
        out = []
        while len(self.buf) >= 5:
            if self.buf[0] & 1:
                raise RpcError(UNIMPLEMENTED, "compressed messages are not supported by this runtime")
            if self.buf[0] & 0xFE:
                raise RpcError(INTERNAL, "reserved frame flags set")
            n = struct.unpack(">I", self.buf[1:5])[0]
            if n > self.max:
                raise RpcError(RESOURCE_EXHAUSTED, "message exceeds the size limit")
            if len(self.buf) < 5 + n:
                break
            out.append(bytes(self.buf[5:5 + n]))
            del self.buf[:5 + n]
        return out

    def at_boundary(self):
        return not self.buf


class _Stream:
    """Per-stream receive state shared by the client and server sides."""

    def __init__(self, max_bytes):
        self.events = asyncio.Queue()
        self.parser = _FrameParser(max_bytes)
        self.window = asyncio.Event()
        self.reset = asyncio.Event()  # set when the peer resets the stream


class _Conn:
    """An HTTP/2 connection driven by asyncio streams."""

    def __init__(self, reader, writer, client_side, on_request=None):
        config = h2.config.H2Configuration(client_side=client_side, header_encoding="utf-8")
        self.h2 = h2.connection.H2Connection(config=config)
        self.reader = reader
        self.writer = writer
        self.streams = {}
        self.on_request = on_request
        self.closed = False
        self.max_bytes = DEFAULT_MAX_MESSAGE_BYTES

    def flush(self):
        data = self.h2.data_to_send()
        if data and not self.writer.is_closing():
            self.writer.write(data)

    async def start(self):
        self.h2.initiate_connection()
        # Larger windows so big messages do not stall on window updates.
        self.h2.increment_flow_control_window(8 * 1024 * 1024 - 65535)
        self.flush()
        await self.writer.drain()

    async def run(self):
        try:
            while True:
                data = await self.reader.read(65536)
                if not data:
                    break
                try:
                    events = self.h2.receive_data(data)
                except h2.exceptions.ProtocolError:
                    self.flush()
                    break
                for ev in events:
                    self._dispatch(ev)
                self.flush()
                await self.writer.drain()
        except (ConnectionError, asyncio.IncompleteReadError):
            pass
        finally:
            self.closed = True
            for st in self.streams.values():
                st.events.put_nowait(("reset", UNAVAILABLE))
                st.window.set()
                st.reset.set()
            try:
                self.writer.close()
            except Exception:
                pass

    def _dispatch(self, ev):
        if isinstance(ev, h2.events.RequestReceived):
            st = _Stream(self.max_bytes)
            self.streams[ev.stream_id] = st
            if self.on_request:
                self.on_request(ev.stream_id, st, ev.headers)
        elif isinstance(ev, h2.events.ResponseReceived):
            st = self.streams.get(ev.stream_id)
            if st:
                st.events.put_nowait(("headers", ev.headers))
        elif isinstance(ev, h2.events.DataReceived):
            st = self.streams.get(ev.stream_id)
            self.h2.acknowledge_received_data(ev.flow_controlled_length, ev.stream_id)
            if st:
                try:
                    for msg in st.parser.feed(ev.data):
                        st.events.put_nowait(("message", msg))
                except RpcError as e:
                    st.events.put_nowait(("error", e))
        elif isinstance(ev, h2.events.TrailersReceived):
            st = self.streams.get(ev.stream_id)
            if st:
                st.events.put_nowait(("trailers", ev.headers))
        elif isinstance(ev, h2.events.StreamEnded):
            st = self.streams.get(ev.stream_id)
            if st:
                st.events.put_nowait(("end", None))
        elif isinstance(ev, h2.events.StreamReset):
            st = self.streams.get(ev.stream_id)
            if st:
                st.events.put_nowait(("reset", ev.error_code))
                st.window.set()
                st.reset.set()
        elif isinstance(ev, (h2.events.WindowUpdated, h2.events.RemoteSettingsChanged)):
            for sid, st in list(self.streams.items()):
                st.window.set()
        elif isinstance(ev, h2.events.ConnectionTerminated):
            self.closed = True
            for st in self.streams.values():
                st.events.put_nowait(("reset", UNAVAILABLE))
                st.window.set()
                st.reset.set()

    async def send_data(self, stream_id, data, end_stream=False):
        st = self.streams[stream_id]
        view = memoryview(data)
        while True:
            if self.closed:
                raise RpcError(UNAVAILABLE, "connection closed")
            try:
                window = self.h2.local_flow_control_window(stream_id)
            except h2.exceptions.StreamClosedError:
                raise RpcError(CANCELLED, "stream closed")
            n = min(len(view), window, self.h2.max_outbound_frame_size)
            if n > 0 or len(view) == 0:
                try:
                    last = n == len(view)
                    self.h2.send_data(stream_id, bytes(view[:n]), end_stream=end_stream and last)
                except (h2.exceptions.StreamClosedError, h2.exceptions.ProtocolError):
                    raise RpcError(CANCELLED, "stream closed")
                view = view[n:]
                self.flush()
                await self.writer.drain()
                if not len(view):
                    return
            else:
                st.window.clear()
                await st.window.wait()

    def reset(self, stream_id, code=h2.errors.ErrorCodes.CANCEL):
        try:
            self.h2.reset_stream(stream_id, error_code=code)
            self.flush()
        except Exception:
            pass
        self.streams.pop(stream_id, None)


# ---- client ----------------------------------------------------------------

class Channel:
    """A client connection to one server (h2c, multiplexed)."""

    def __init__(self, host, port, max_message_bytes=DEFAULT_MAX_MESSAGE_BYTES):
        self.host, self.port = host, port
        self.max_message_bytes = max_message_bytes
        self._conn = None
        self._lock = asyncio.Lock()

    async def _connection(self):
        async with self._lock:
            if self._conn is None or self._conn.closed:
                reader, writer = await asyncio.open_connection(self.host, self.port)
                conn = _Conn(reader, writer, True)
                conn.max_bytes = self.max_message_bytes
                await conn.start()
                conn.task = asyncio.get_running_loop().create_task(conn.run())
                self._conn = conn
            return self._conn

    async def start(self, method, metadata=None, timeout=None):
        """Opens a call; use `send`, `close_send` and `recv` on the result."""
        try:
            conn = await self._connection()
        except OSError as e:
            raise RpcError(UNAVAILABLE, str(e))
        headers = [
            (":method", "POST"), (":scheme", "http"), (":authority", "%s:%d" % (self.host, self.port)),
            (":path", "/" + method), ("content-type", "application/tpt20"), ("te", "trailers"),
        ]
        if timeout is not None:
            headers.append(("grpc-timeout", encode_timeout(timeout)))
        for k, values in (metadata or {}).items():
            for v in values if isinstance(values, (list, tuple)) else [values]:
                headers.append((k.lower(), v))
        stream_id = conn.h2.get_next_available_stream_id()
        conn.streams[stream_id] = _Stream(self.max_message_bytes)
        conn.h2.send_headers(stream_id, headers, end_stream=False)
        conn.flush()
        return Call(conn, stream_id, timeout)

    async def unary(self, method, request, metadata=None, timeout=None):
        call = await self.start(method, metadata, timeout)
        try:
            await call.send(request)
            await call.close_send()
            msg = await call.recv()
            if msg is None:
                raise RpcError(INTERNAL, "call succeeded without a response message")
            if await call.recv() is not None:
                raise RpcError(INTERNAL, "unary call returned several messages")
            return msg
        finally:
            call.close()

    async def close(self):
        if self._conn is not None:
            self._conn.writer.close()


class Call:
    """One client call in flight."""

    def __init__(self, conn, stream_id, timeout):
        self.conn = conn
        self.id = stream_id
        self.deadline = None if timeout is None else time.monotonic() + timeout
        self.status = None  # set once trailers arrive
        self.done = False
        self.send_closed = False

    async def send(self, msg):
        await self._guard(self.conn.send_data(self.id, frame(msg)))

    async def close_send(self):
        if not self.send_closed:
            self.send_closed = True
            await self._guard(self.conn.send_data(self.id, b"", end_stream=True))

    def close(self):
        """Abandons the call (cancels it on the server if still running)."""
        if not self.done:
            self.done = True
            self.conn.reset(self.id)
        self.conn.streams.pop(self.id, None)

    async def _guard(self, aw):
        remaining = None if self.deadline is None else self.deadline - time.monotonic()
        if remaining is not None and remaining <= 0:
            self.close()
            raise RpcError(DEADLINE_EXCEEDED, "deadline exceeded")
        try:
            return await asyncio.wait_for(aw, remaining)
        except asyncio.TimeoutError:
            self.close()
            raise RpcError(DEADLINE_EXCEEDED, "deadline exceeded")
        except asyncio.CancelledError:
            self.close()
            raise

    async def recv(self):
        """Next response message, or None after the final OK status."""
        if self.done:
            return None
        st = self.conn.streams.get(self.id)
        if st is None:
            raise RpcError(UNAVAILABLE, "connection closed")
        while True:
            kind, value = await self._guard(st.events.get())
            if kind == "message":
                return value
            if kind == "headers":
                got = dict(value)
                if "grpc-status" in got:  # trailers-only response
                    self.status = got
            elif kind == "trailers":
                self.status = dict(value)
            elif kind == "error":
                self.close()
                raise value
            elif kind == "reset":
                self.done = True
                raise RpcError(CANCELLED if value == 8 else UNAVAILABLE, "stream reset by server")
            elif kind == "end":
                self.done = True
                self.conn.streams.pop(self.id, None)
                status = self.status or {}
                if "grpc-status" not in status:
                    raise RpcError(UNAVAILABLE, "connection ended without trailers")
                code = int(status["grpc-status"])
                if code == OK:
                    return None
                raise RpcError(code, percent_decode(status.get("grpc-message", "")))


# ---- server ----------------------------------------------------------------

class ServerContext:
    """What a handler knows about its call."""

    def __init__(self, metadata, deadline):
        self.metadata = metadata
        self.deadline = deadline

    def time_remaining(self):
        return None if self.deadline is None else max(0.0, self.deadline - time.monotonic())


class ServerCall:
    """Message transport of one incoming call (used by generated code)."""

    def __init__(self, conn, stream_id, stream, metadata):
        self.conn, self.id, self.stream = conn, stream_id, stream
        self.metadata = metadata
        self.ended = False

    async def recv(self):
        """Next request message; None once the client half-closed."""
        if self.ended:
            return None
        while True:
            kind, value = await self.stream.events.get()
            if kind == "message":
                return value
            if kind == "end":
                self.ended = True
                return None
            if kind == "error":
                raise value
            if kind == "reset":
                self.ended = True
                raise RpcError(CANCELLED, "client went away")

    async def send(self, msg):
        await self.conn.send_data(self.id, frame(msg))


class Server:
    """Serves registered services over h2c."""

    def __init__(self, max_message_bytes=DEFAULT_MAX_MESSAGE_BYTES):
        self.services = {}
        self.max_message_bytes = max_message_bytes

    def register(self, name, handler):
        """`handler(method, call, ctx)` is an async function; it returns when
        the call is complete (raise RpcError for a failure status)."""
        self.services[name] = handler

    async def serve(self, host="127.0.0.1", port=0):
        """Starts listening; returns the asyncio server (use `.sockets`)."""
        return await asyncio.start_server(self._accept, host, port)

    async def _accept(self, reader, writer):
        conn = _Conn(reader, writer, False)
        conn.max_bytes = self.max_message_bytes
        conn.on_request = lambda sid, st, headers: asyncio.get_running_loop().create_task(
            self._handle(conn, sid, st, headers)
        )
        await conn.start()
        await conn.run()

    async def _handle(self, conn, sid, stream, headers):
        h = dict(headers)
        ct = h.get("content-type", "")
        ct = ct if ct.startswith("application/grpc") else "application/tpt20"
        conn.h2.send_headers(sid, [(":status", "200"), ("content-type", ct), ("grpc-accept-encoding", "identity")])
        conn.flush()
        timeout = decode_timeout(h.get("grpc-timeout", ""))
        deadline = None if timeout is None else time.monotonic() + timeout
        ctx = ServerContext(_metadata_from(headers), deadline)
        call = ServerCall(conn, sid, stream, ctx.metadata)
        service, _, method = h.get(":path", "").lstrip("/").partition("/")
        code, message = OK, ""
        handler = self.services.get(service)

        async def run():
            if handler is None or not method:
                raise RpcError(UNIMPLEMENTED, "unknown service `%s`" % service)
            await handler(method, call, ctx)

        reset = asyncio.get_running_loop().create_task(stream.reset.wait())
        try:
            work = asyncio.get_running_loop().create_task(run())
            done, _ = await asyncio.wait({work, reset}, timeout=timeout, return_when=asyncio.FIRST_COMPLETED)
            if work in done:
                work.result()
            else:
                work.cancel()
                if reset in done:
                    return  # client reset the stream: nothing to answer
                code, message = DEADLINE_EXCEEDED, "deadline exceeded"
        except RpcError as e:
            code, message = e.code, e.message
        except asyncio.CancelledError:
            raise
        except Exception:
            code, message = INTERNAL, "handler failed"
        finally:
            reset.cancel()
        trailers = [("grpc-status", str(code))]
        if message:
            trailers.append(("grpc-message", percent_encode(message)))
        try:
            conn.h2.send_headers(sid, trailers, end_stream=True)
            conn.flush()
        except Exception:
            pass
        conn.streams.pop(sid, None)


async def aiter_of(items):
    """Iterates a sync or async iterable asynchronously."""
    if hasattr(items, "__aiter__"):
        async for x in items:
            yield x
    else:
        for x in items:
            yield x
