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

`partial` is best-effort and currently decoded from a bounded rolling recent window of the session audio. If the realtime inference slot is busy, a partial update may be skipped. `final` is decoded from the full buffered session audio after `stop` and returns `error` if the realtime inference slot cannot be acquired.

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

## Error Cases

- invalid JSON control frame
- binary frame before `start`
- `stop` before `start`
- repeated `start` while a session is already active on the same socket
- realtime capacity exhausted
- final decode cannot acquire a realtime inference slot

All of the above currently return a server event with `{"event":"error"}`.
