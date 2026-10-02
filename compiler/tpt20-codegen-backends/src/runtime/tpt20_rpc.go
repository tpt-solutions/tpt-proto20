// tpt20 minimal Go RPC runtime: a client and a server for the tpt20 RPC
// protocol over HTTP/2 (Go 1.24+, cleartext h2c with prior knowledge).
//
// Generated together with the message code; do not edit. The protocol is the
// one the Rust runtime speaks: POST /<service>/<method>, messages framed as
// flags(1) | length(4, big endian) | payload, final status in the
// `grpc-status` / `grpc-message` trailers, deadline in `grpc-timeout`.

package PACKAGE

import (
	"context"
	"encoding/base64"
	"encoding/binary"
	"errors"
	"fmt"
	"io"
	"net"
	"net/http"
	"strconv"
	"strings"
	"sync"
	"time"
)

// Tpt20Code is an RPC status code (same numbering as gRPC).
type Tpt20Code int

const (
	Tpt20OK Tpt20Code = iota
	Tpt20Cancelled
	Tpt20Unknown
	Tpt20InvalidArgument
	Tpt20DeadlineExceeded
	Tpt20NotFound
	Tpt20AlreadyExists
	Tpt20PermissionDenied
	Tpt20ResourceExhausted
	Tpt20FailedPrecondition
	Tpt20Aborted
	Tpt20OutOfRange
	Tpt20Unimplemented
	Tpt20Internal
	Tpt20Unavailable
	Tpt20DataLoss
	Tpt20Unauthenticated
)

var tpt20CodeNames = [...]string{
	"OK", "CANCELLED", "UNKNOWN", "INVALID_ARGUMENT", "DEADLINE_EXCEEDED", "NOT_FOUND",
	"ALREADY_EXISTS", "PERMISSION_DENIED", "RESOURCE_EXHAUSTED", "FAILED_PRECONDITION",
	"ABORTED", "OUT_OF_RANGE", "UNIMPLEMENTED", "INTERNAL", "UNAVAILABLE", "DATA_LOSS",
	"UNAUTHENTICATED",
}

func (c Tpt20Code) String() string {
	if c >= 0 && int(c) < len(tpt20CodeNames) {
		return tpt20CodeNames[c]
	}
	return "UNKNOWN"
}

// Tpt20Status is the error type of a failed call.
type Tpt20Status struct {
	Code    Tpt20Code
	Message string
}

func (s *Tpt20Status) Error() string { return fmt.Sprintf("tpt20 rpc: %s: %s", s.Code, s.Message) }

// Tpt20Error builds a status error to return from a handler.
func Tpt20Error(code Tpt20Code, message string) error {
	return &Tpt20Status{Code: code, Message: message}
}

// Tpt20StatusOf converts any error into a status (context errors map to their
// RPC codes, other errors to UNKNOWN).
func Tpt20StatusOf(err error) *Tpt20Status {
	var s *Tpt20Status
	switch {
	case err == nil:
		return &Tpt20Status{Code: Tpt20OK}
	case errors.As(err, &s):
		return s
	case errors.Is(err, context.DeadlineExceeded):
		return &Tpt20Status{Code: Tpt20DeadlineExceeded, Message: "deadline exceeded"}
	case errors.Is(err, context.Canceled):
		return &Tpt20Status{Code: Tpt20Cancelled, Message: "call cancelled"}
	default:
		return &Tpt20Status{Code: Tpt20Unknown, Message: err.Error()}
	}
}

// Tpt20Metadata is call metadata; keys are lower case. Keys ending in "-bin"
// carry binary values (base64 on the wire; use SetBin/GetBin).
type Tpt20Metadata map[string][]string

// Set replaces the values of key.
func (m Tpt20Metadata) Set(key, value string) { m[strings.ToLower(key)] = []string{value} }

// Get returns the first value of key.
func (m Tpt20Metadata) Get(key string) string {
	if v := m[strings.ToLower(key)]; len(v) > 0 {
		return v[0]
	}
	return ""
}

// SetBin stores a binary value; key must end in "-bin".
func (m Tpt20Metadata) SetBin(key string, value []byte) {
	m.Set(key, base64.StdEncoding.EncodeToString(value))
}

// GetBin returns a binary value.
func (m Tpt20Metadata) GetBin(key string) ([]byte, bool) {
	v := m.Get(key)
	if v == "" {
		return nil, false
	}
	b, err := base64.StdEncoding.DecodeString(v)
	return b, err == nil
}

var tpt20Reserved = map[string]bool{
	"grpc-status": true, "grpc-message": true, "grpc-timeout": true, "content-type": true,
	"te": true, "grpc-encoding": true, "grpc-accept-encoding": true, "traceparent": true,
	"tracestate": true, "user-agent": true, "accept-encoding": true, "content-length": true,
	"trailer": true,
}

func rtMetadataFrom(h http.Header) Tpt20Metadata {
	md := Tpt20Metadata{}
	for k, v := range h {
		k = strings.ToLower(k)
		if tpt20Reserved[k] || strings.HasPrefix(k, ":") {
			continue
		}
		md[k] = append([]string(nil), v...)
	}
	return md
}

func rtPercentEncode(s string) string {
	var b strings.Builder
	for _, c := range []byte(s) {
		if c >= 0x20 && c <= 0x7e && c != '%' {
			b.WriteByte(c)
		} else {
			fmt.Fprintf(&b, "%%%02X", c)
		}
	}
	return b.String()
}

func rtPercentDecode(s string) string {
	var out []byte
	for i := 0; i < len(s); i++ {
		if s[i] == '%' && i+2 < len(s) {
			if v, err := strconv.ParseUint(s[i+1:i+3], 16, 8); err == nil {
				out = append(out, byte(v))
				i += 2
				continue
			}
		}
		out = append(out, s[i])
	}
	return string(out)
}

func rtEncodeTimeout(d time.Duration) string {
	if d < 0 {
		d = 0
	}
	const max = 99_999_999
	n := int64(d)
	for _, u := range []struct {
		unit byte
		per  int64
	}{{'n', 1}, {'u', 1000}, {'m', 1_000_000}, {'S', 1_000_000_000}, {'M', 60_000_000_000}} {
		v := (n + u.per - 1) / u.per
		if v <= max {
			return fmt.Sprintf("%d%c", v, u.unit)
		}
	}
	return fmt.Sprintf("%dH", min((n+3_600_000_000_000-1)/3_600_000_000_000, max))
}

func rtDecodeTimeout(s string) (time.Duration, bool) {
	s = strings.TrimSpace(s)
	if len(s) < 2 || len(s) > 9 {
		return 0, false
	}
	n, err := strconv.ParseInt(s[:len(s)-1], 10, 64)
	if err != nil || n < 0 {
		return 0, false
	}
	switch s[len(s)-1] {
	case 'n':
		return time.Duration(n), true
	case 'u':
		return time.Duration(n) * time.Microsecond, true
	case 'm':
		return time.Duration(n) * time.Millisecond, true
	case 'S':
		return time.Duration(n) * time.Second, true
	case 'M':
		return time.Duration(n) * time.Minute, true
	case 'H':
		return time.Duration(n) * time.Hour, true
	}
	return 0, false
}

func rtFrame(payload []byte) []byte {
	out := make([]byte, 5+len(payload))
	binary.BigEndian.PutUint32(out[1:5], uint32(len(payload)))
	copy(out[5:], payload)
	return out
}

// rtReadFrame reads one message; io.EOF means a clean end of the stream.
func rtReadFrame(r io.Reader, max int) ([]byte, error) {
	var head [5]byte
	if _, err := io.ReadFull(r, head[:]); err != nil {
		if err == io.EOF {
			return nil, io.EOF
		}
		return nil, err
	}
	if head[0]&1 != 0 {
		return nil, Tpt20Error(Tpt20Unimplemented, "compressed messages are not supported by this runtime")
	}
	if head[0]&0xfe != 0 {
		return nil, Tpt20Error(Tpt20Internal, "reserved frame flags set")
	}
	n := int(binary.BigEndian.Uint32(head[1:5]))
	if n > max {
		return nil, Tpt20Error(Tpt20ResourceExhausted, "message exceeds the size limit")
	}
	buf := make([]byte, n)
	if _, err := io.ReadFull(r, buf); err != nil {
		if err == io.EOF {
			err = io.ErrUnexpectedEOF
		}
		return nil, err
	}
	return buf, nil
}

const tpt20DefaultMax = 4 * 1024 * 1024

// ---- client ----------------------------------------------------------------

// Tpt20Channel is a client connection to one server (h2c, multiplexed).
type Tpt20Channel struct {
	base            string
	hc              *http.Client
	MaxMessageBytes int
}

// Tpt20Dial creates a channel for "host:port" (connections are made lazily).
func Tpt20Dial(addr string) *Tpt20Channel {
	t := &http.Transport{}
	t.Protocols = new(http.Protocols)
	t.Protocols.SetUnencryptedHTTP2(true)
	return &Tpt20Channel{base: "http://" + addr, hc: &http.Client{Transport: t}, MaxMessageBytes: tpt20DefaultMax}
}

// Tpt20Call is one client call in flight.
type Tpt20Call struct {
	ch     *Tpt20Channel
	pw     *io.PipeWriter
	cancel context.CancelFunc
	ctx    context.Context
	ready  chan struct{}
	resp   *http.Response
	err    error
	done   bool
	sendMu sync.Mutex
}

// Start begins a call; send messages with Send and read with Recv.
func (c *Tpt20Channel) Start(ctx context.Context, method string, md Tpt20Metadata) (*Tpt20Call, error) {
	pr, pw := io.Pipe()
	cctx, cancel := context.WithCancel(ctx)
	req, err := http.NewRequestWithContext(cctx, http.MethodPost, c.base+"/"+method, pr)
	if err != nil {
		cancel()
		return nil, err
	}
	req.ContentLength = -1
	req.Header.Set("content-type", "application/tpt20")
	req.Header.Set("te", "trailers")
	if dl, ok := ctx.Deadline(); ok {
		req.Header.Set("grpc-timeout", rtEncodeTimeout(time.Until(dl)))
	}
	for k, vs := range md {
		for _, v := range vs {
			req.Header.Add(k, v)
		}
	}
	call := &Tpt20Call{ch: c, pw: pw, cancel: cancel, ctx: ctx, ready: make(chan struct{})}
	go func() {
		call.resp, call.err = c.hc.Do(req)
		close(call.ready)
	}()
	return call, nil
}

// Send writes one request message.
func (c *Tpt20Call) Send(msg []byte) error {
	c.sendMu.Lock()
	defer c.sendMu.Unlock()
	if _, err := c.pw.Write(rtFrame(msg)); err != nil {
		// The server ended the call; its status explains why.
		if _, rerr := c.Recv(); rerr != nil && rerr != io.EOF {
			return rerr
		}
		return err
	}
	return nil
}

// CloseSend half-closes the request stream.
func (c *Tpt20Call) CloseSend() error { return c.pw.Close() }

func (c *Tpt20Call) status() error {
	get := func(k string) string {
		if v := c.resp.Trailer.Get(k); v != "" {
			return v
		}
		return c.resp.Header.Get(k)
	}
	raw := get("grpc-status")
	if raw == "" {
		if c.ctx.Err() != nil {
			return Tpt20StatusOf(c.ctx.Err())
		}
		return Tpt20Error(Tpt20Unavailable, "connection ended without trailers")
	}
	code, err := strconv.Atoi(raw)
	if err != nil {
		return Tpt20Error(Tpt20Internal, "malformed grpc-status")
	}
	if code == 0 {
		return io.EOF
	}
	return Tpt20Error(Tpt20Code(code), rtPercentDecode(get("grpc-message")))
}

// Recv returns the next response message. It returns io.EOF after the last
// message when the call ended with status OK, and a *Tpt20Status otherwise.
func (c *Tpt20Call) Recv() ([]byte, error) {
	<-c.ready
	if c.err != nil {
		if c.ctx.Err() != nil {
			return nil, Tpt20StatusOf(c.ctx.Err())
		}
		return nil, Tpt20Error(Tpt20Unavailable, c.err.Error())
	}
	if c.done {
		return nil, c.status()
	}
	msg, err := rtReadFrame(c.resp.Body, c.ch.MaxMessageBytes)
	if err == io.EOF {
		c.done = true
		c.cancel()
		return nil, c.status()
	}
	if err != nil {
		if c.ctx.Err() != nil {
			return nil, Tpt20StatusOf(c.ctx.Err())
		}
		var st *Tpt20Status
		if errors.As(err, &st) {
			return nil, err
		}
		return nil, Tpt20Error(Tpt20Unavailable, err.Error())
	}
	return msg, nil
}

// Close abandons the call (cancels it on the server).
func (c *Tpt20Call) Close() { c.cancel() }

// Unary performs a call with one request and one response message.
func (c *Tpt20Channel) Unary(ctx context.Context, method string, md Tpt20Metadata, req []byte) ([]byte, error) {
	call, err := c.Start(ctx, method, md)
	if err != nil {
		return nil, err
	}
	defer call.Close()
	go func() {
		_ = call.Send(req)
		_ = call.CloseSend()
	}()
	msg, err := call.Recv()
	if err == io.EOF {
		return nil, Tpt20Error(Tpt20Internal, "call succeeded without a response message")
	}
	if err != nil {
		return nil, err
	}
	if _, err := call.Recv(); err != io.EOF {
		if err == nil {
			return nil, Tpt20Error(Tpt20Internal, "unary call returned several messages")
		}
		return nil, err
	}
	return msg, nil
}

// Tpt20Stream is a typed client call for streaming methods.
type Tpt20Stream[Q, R any] struct {
	call *Tpt20Call
	enc  func(Q) []byte
	dec  func([]byte) (R, error)
}

// Tpt20NewStream wraps a started call with its message codecs.
func Tpt20NewStream[Q, R any](call *Tpt20Call, enc func(Q) []byte, dec func([]byte) (R, error)) *Tpt20Stream[Q, R] {
	return &Tpt20Stream[Q, R]{call: call, enc: enc, dec: dec}
}

// Send writes one request message.
func (s *Tpt20Stream[Q, R]) Send(v Q) error { return s.call.Send(s.enc(v)) }

// CloseSend half-closes the request stream.
func (s *Tpt20Stream[Q, R]) CloseSend() error { return s.call.CloseSend() }

// Recv returns the next response; io.EOF after the final OK status.
func (s *Tpt20Stream[Q, R]) Recv() (R, error) {
	var zero R
	b, err := s.call.Recv()
	if err != nil {
		return zero, err
	}
	v, err := s.dec(b)
	if err != nil {
		s.call.Close()
		return zero, Tpt20Error(Tpt20Internal, "invalid response message: "+err.Error())
	}
	return v, nil
}

// CloseAndRecv finishes a client-streaming call and returns its response.
func (s *Tpt20Stream[Q, R]) CloseAndRecv() (R, error) {
	var zero R
	_ = s.call.CloseSend()
	v, err := s.Recv()
	if err == io.EOF {
		return zero, Tpt20Error(Tpt20Internal, "call succeeded without a response message")
	}
	if err != nil {
		return zero, err
	}
	if _, err := s.call.Recv(); err != io.EOF {
		if err == nil {
			return zero, Tpt20Error(Tpt20Internal, "unary call returned several messages")
		}
		return zero, err
	}
	s.call.Close()
	return v, nil
}

// Close abandons the call.
func (s *Tpt20Stream[Q, R]) Close() { s.call.Close() }

// ---- server ----------------------------------------------------------------

// Tpt20Service is implemented by generated service registrations.
type Tpt20Service interface {
	Tpt20ServiceName() string
	Tpt20Handle(ctx context.Context, method string, call *Tpt20ServerCall) error
}

// Tpt20Server serves registered services over h2c.
type Tpt20Server struct {
	mu              sync.RWMutex
	services        map[string]Tpt20Service
	MaxMessageBytes int
}

// NewTpt20Server creates an empty server.
func NewTpt20Server() *Tpt20Server {
	return &Tpt20Server{services: map[string]Tpt20Service{}, MaxMessageBytes: tpt20DefaultMax}
}

// Register adds a service.
func (s *Tpt20Server) Register(svc Tpt20Service) {
	s.mu.Lock()
	s.services[svc.Tpt20ServiceName()] = svc
	s.mu.Unlock()
}

// Serve accepts connections on l until it fails.
func (s *Tpt20Server) Serve(l net.Listener) error {
	srv := &http.Server{Handler: s}
	srv.Protocols = new(http.Protocols)
	srv.Protocols.SetUnencryptedHTTP2(true)
	return srv.Serve(l)
}

// Tpt20ServerCall is one incoming call as seen by a handler.
type Tpt20ServerCall struct {
	// Metadata is the request metadata.
	Metadata Tpt20Metadata
	r        io.Reader
	w        http.ResponseWriter
	fl       http.Flusher
	max      int
	mu       sync.Mutex
}

// Recv returns the next request message; io.EOF once the client finished.
func (c *Tpt20ServerCall) Recv() ([]byte, error) { return rtReadFrame(c.r, c.max) }

// Send writes one response message.
func (c *Tpt20ServerCall) Send(msg []byte) error {
	c.mu.Lock()
	defer c.mu.Unlock()
	if _, err := c.w.Write(rtFrame(msg)); err != nil {
		return Tpt20Error(Tpt20Cancelled, "client went away")
	}
	c.fl.Flush()
	return nil
}

// ServeHTTP implements http.Handler.
func (s *Tpt20Server) ServeHTTP(w http.ResponseWriter, r *http.Request) {
	fl, _ := w.(http.Flusher)
	path := strings.TrimPrefix(r.URL.Path, "/")
	service, method, _ := strings.Cut(path, "/")
	ct := "application/tpt20"
	if c := r.Header.Get("content-type"); strings.HasPrefix(c, "application/grpc") {
		ct = c
	}
	w.Header().Set("content-type", ct)
	w.Header().Set("grpc-accept-encoding", "identity")
	w.WriteHeader(http.StatusOK)
	fl.Flush()

	ctx := r.Context()
	if t, ok := rtDecodeTimeout(r.Header.Get("grpc-timeout")); ok {
		var cancel context.CancelFunc
		ctx, cancel = context.WithTimeout(ctx, t)
		defer cancel()
	}
	finish := func(err error) {
		st := Tpt20StatusOf(err)
		w.Header().Set(http.TrailerPrefix+"grpc-status", strconv.Itoa(int(st.Code)))
		if st.Message != "" {
			w.Header().Set(http.TrailerPrefix+"grpc-message", rtPercentEncode(st.Message))
		}
	}
	s.mu.RLock()
	svc := s.services[service]
	s.mu.RUnlock()
	if svc == nil || method == "" {
		finish(Tpt20Error(Tpt20Unimplemented, "unknown service `"+service+"`"))
		return
	}
	call := &Tpt20ServerCall{Metadata: rtMetadataFrom(r.Header), r: r.Body, w: w, fl: fl, max: s.MaxMessageBytes}
	var err error
	func() {
		defer func() {
			if p := recover(); p != nil {
				err = Tpt20Error(Tpt20Internal, "handler panicked")
			}
		}()
		err = svc.Tpt20Handle(ctx, method, call)
	}()
	if err == nil && ctx.Err() != nil {
		err = ctx.Err()
	}
	finish(err)
}

// Tpt20ServerStream is the typed stream handed to streaming handlers.
type Tpt20ServerStream[Q, R any] struct {
	call *Tpt20ServerCall
	dec  func([]byte) (Q, error)
	enc  func(R) []byte
}

// Tpt20NewServerStream wraps a server call with its message codecs.
func Tpt20NewServerStream[Q, R any](call *Tpt20ServerCall, dec func([]byte) (Q, error), enc func(R) []byte) *Tpt20ServerStream[Q, R] {
	return &Tpt20ServerStream[Q, R]{call: call, dec: dec, enc: enc}
}

// Recv returns the next request; io.EOF after the client half-closed.
func (s *Tpt20ServerStream[Q, R]) Recv() (Q, error) {
	var zero Q
	b, err := s.call.Recv()
	if err != nil {
		return zero, err
	}
	v, err := s.dec(b)
	if err != nil {
		return zero, Tpt20Error(Tpt20InvalidArgument, "invalid request message: "+err.Error())
	}
	return v, nil
}

// Send writes one response message.
func (s *Tpt20ServerStream[Q, R]) Send(v R) error { return s.call.Send(s.enc(v)) }
