# Realtime Protocol

The realtime API is exposed at `GET /v1/realtime` via WebSocket.

## Client Messages

- `{"event":"start"}`: starts a realtime session and reserves capacity.
- binary frame: appends raw `pcm_s16le`, `16kHz`, mono audio bytes to the active session.
- `{"event":"stop"}`: finalizes the session and emits the final transcript.

## Server Messages

- `session_started`
- `partial`
- `final`
- `error`

`partial` is best-effort and currently decoded from a bounded rolling recent window of the session audio. Both `partial` and `final` wait for the realtime inference slot instead of being dropped when the engine is busy.
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

All of the above return a server event with `{"event":"error","code":"...","reason":"..."}`.
