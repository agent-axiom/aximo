# Realtime Protocol

The realtime API is exposed at `GET /v1/realtime` via WebSocket.

This endpoint currently implements bounded buffered realtime. It accepts live chunks and emits partial/final events, but the model path is not a true incremental streaming decoder. Session memory and final latency are bounded by `max_realtime_session_bytes` and `max_realtime_session_duration_ms`.

## Client Messages

- `{"event":"start"}`: starts a realtime session and reserves capacity.
- binary frame: appends raw `pcm_s16le`, `16kHz`, mono audio bytes to the active session.
- `{"event":"stop"}`: finalizes the session and emits the final transcript.

## Server Messages

- `session_started`
- `partial`
- `final`
- `error`

`partial` is best-effort and currently decoded from a bounded rolling recent window of the session audio. Partial updates use latest-wins backpressure: when the realtime inference slot is saturated, the service keeps at most one fresher follow-up partial instead of replaying a backlog of stale partial work. This means partial updates are freshness-oriented and may not form a steady cadence. `final` remains strict and always waits for the realtime inference slot to transcribe the full bounded session buffer.
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
- realtime capacity exhausted
- inference failure while producing `partial` or `final`

## Backpressure Semantics

- `partial` updates are lossy by design and optimized for freshness.
- If multiple eligible partials accumulate while one is already in flight, they are coalesced into one latest follow-up partial.
- `final` is never coalesced or dropped and still runs against the full session buffer within the configured realtime session limits.

All of the above return a server event with `{"event":"error","code":"...","reason":"..."}`.
