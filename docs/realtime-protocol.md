# Realtime Protocol

The realtime API is exposed at `GET /v1/realtime` via WebSocket.

This endpoint supports native streaming when the configured realtime backend reports `supports_native_streaming=true` from `/v1/capabilities`. In that mode, Aximo creates a stateful backend streaming session and passes each chunk directly to it.

The bundled Parakeet/GigaAM ONNX adapters currently report `supports_native_streaming=false`, so they use bounded buffered realtime. In that fallback mode the endpoint accepts live chunks and emits partial/final events, but the model path is not a true incremental streaming decoder. Session memory and final latency are bounded by `max_realtime_session_bytes` and `max_realtime_session_duration_ms`.

## Client Messages

- `{"event":"start"}`: starts a realtime session and reserves capacity.
- binary frame: appends raw `pcm_s16le`, `16kHz`, mono audio bytes to the active session. Chunks must be aligned to 16-bit samples.
- `{"event":"stop"}`: finalizes the session and emits the final transcript.

## Server Messages

- `session_started`
- `partial`
- `final`
- `error`

`partial` is best-effort. Native streaming backends may emit a partial directly from their streaming session after a binary chunk. Bounded buffered backends decode partials from a bounded rolling recent window of the session audio and use latest-wins backpressure: when the realtime inference slot is saturated, the service keeps at most one fresher follow-up partial instead of replaying a backlog of stale partial work. This means partial updates are freshness-oriented and may not form a steady cadence. `final` remains strict; bounded buffered backends transcribe the full bounded session buffer, while native streaming backends finalize their stateful streaming session.
`error` carries machine-readable `code` and human-readable `reason`.

## Example Session

```json
{"event":"start"}
```

```json
{"event":"session_started","session_id":"session-1"}
```

binary audio chunk

```json
{"event":"partial","text":"..."}
```

```json
{"event":"stop"}
```

```json
{"event":"final","text":"..."}
```

```json
{"event":"error","code":"invalid_client_event","reason":"failed to parse client event"}
```

## Error Cases

- invalid JSON control frame
- binary frame before `start`
- `stop` before `start`
- repeated `start` while a session is already active on the same socket
- odd-length `pcm_s16le` binary chunk
- realtime capacity exhausted
- inference failure or inference timeout while producing `partial` or `final`

## Backpressure Semantics

- `partial` updates are lossy by design and optimized for freshness.
- For bounded buffered backends, if multiple eligible partials accumulate while one is already in flight, they are coalesced into one latest follow-up partial.
- `final` is never coalesced or dropped. Bounded buffered backends run it against the full session buffer within configured realtime session limits; native streaming backends call the backend session's `finish()` operation.
- Server events use a bounded per-socket queue controlled by `realtime_event_channel_capacity`; queue overflow increments `aximo_ws_queue_overflows_total`, attempts to enqueue a final `websocket_queue_overflow` error event, and then terminates the websocket session.
- Partial and final inference use separate timeout budgets. If a timeout fires, the scheduler permit is released and the client receives `inference_timeout`; the underlying blocking backend call may still return later because the server cannot safely kill the OS blocking thread.
- A per-engine execution gate is held inside the blocking task until the backend call actually returns. This keeps timeout semantics honest: client wait is bounded, but timed-out backend work still occupies model execution capacity until it exits.
- Runtime health is tracked separately for `realtime_partial:<engine>` and `realtime_final:<engine>`, so a flaky partial path does not automatically hide behind a successful short-audio request.

All of the above return a server event with `{"event":"error","code":"...","reason":"..."}`.
