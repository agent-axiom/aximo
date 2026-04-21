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
- realtime capacity exhausted

All of the above currently return a server event with `{"event":"error"}`.
