# Local voice input for terminals and Claude Code

**Status: integration assessment, checked 2026-10-07.** Aximo supplies local
transcription over HTTP and WebSocket. This repository does not yet ship a
terminal microphone client or a Claude Code plugin. Start with an existing WAV
and manual paste; the in-session integration below is a proposed next step.

## What works today

- Run Aximo on loopback using the [installation guide](installation.md), then
  send a whole audio file to `POST /v1/transcriptions`. The request body is raw
  audio bytes, not multipart form data. Read the final JSON `text` field.
- Check `GET /v1/capabilities` for the installed model's actual language support.
  The bundled adapters report English for Parakeet and Russian for GigaAM.
  Set `default_offline_engine = "gigaam"` and restart for Russian transcription;
  adding `?engine=gigaam` cannot switch a Parakeet-configured instance.
- Default short-audio limits include 60 seconds of audio and a 120-second
  inference timeout. Keep recordings shorter than the configured limit. See the
  [API reference](api-reference.md) and [configuration](configuration.md).
- WebSocket partials are available, but the bundled adapters use bounded
  buffered offline decoding, not a true incremental decoder. For a first
  dictation client, one final HTTP result is simpler and avoids unstable partial
  text in a prompt. See the [realtime protocol](realtime-protocol.md).

Claude Code already has `/voice`, including Russian support, but it streams
audio to Anthropic for transcription. It requires Claude.ai authentication and
a local microphone; SSH and cloud sessions are unsupported. The official voice
documentation does not describe a custom STT endpoint setting, so Aximo is not a
documented drop-in backend for `/voice`. Hold mode normally waits for Enter;
tap mode can submit automatically. See [Claude Code voice
dictation](https://code.claude.com/docs/en/voice-dictation).

## Try an existing recording

Prerequisites: Aximo is already running on `127.0.0.1:8080`, `curl` and `jq` are
installed, and `sample.wav` is an existing recording within the configured
limits. Run this in Bash. It uploads that file only when you run it; it does not
start a microphone or invoke Claude.

```bash
if transcript=$(
  set -o pipefail
  curl --fail --silent --show-error --noproxy '*' \
    --connect-timeout 3 --max-time 130 \
    -H 'Content-Type: audio/wav' \
    --data-binary @sample.wav \
    http://127.0.0.1:8080/v1/transcriptions |
    jq -er '
      .text | select(type == "string") | select(test("\\S")) |
      select(test("[\u0000-\u0008\u000b-\u001f\u007f-\u009f]") | not)
    '
); then
  printf '%s\n' "$transcript"
else
  unset transcript
  printf '%s\n' 'Transcription failed or returned empty/unsafe text.' >&2
fi
```

Review the displayed text, copy it into Claude Code's prompt, edit it, and press
Enter yourself. Do not paste unreviewed speech into an executable shell prompt.
After reviewing, you can alternatively start an interactive session with
`claude "$transcript"`; this submits an initial prompt immediately, so run it
only intentionally. Prefer manual paste if the text begins with `-`, which may
be parsed as a CLI option. Never use `eval`, `sh -c`, or unquoted transcript
expansion. Command-line arguments can also be visible to other local processes.
See the [Claude Code CLI reference](https://code.claude.com/docs/en/cli-reference).

## Proposed in-session `/aximo-dictate` mod

The current official [mods
overview](https://code.claude.com/docs/en/plugins/mods/overview) documents custom
commands in the terminal on Claude Code v2.1.287 or later. A mod is a plausible
integration surface, but this design has not been implemented or tested here.

1. Register `/aximo-dictate` with explicit Start, Stop, and Cancel controls and a
   visible recording indicator. Request OS microphone permission only after the
   user starts recording; allow only one recording per session.
2. Launch a trusted, platform-specific recorder helper through `$.process.run`
   or `$.process.spawn`, passing an argument array rather than shell text. Have
   the helper capture bounded mono WAV audio, POST its bytes to the fixed
   loopback endpoint, and return structured JSON. Use finite capture and HTTP
   timeouts; `process.run` defaults to 30 seconds, so configure its supported
   timeout for the full recording/transcription budget. See the [mods
   API](https://code.claude.com/docs/en/plugins/mods/api).
3. Validate the helper exit status, HTTP status, JSON shape, and a nonempty,
   bounded final `text` string without terminal control sequences. Insert it as
   editable draft text through `$.prompt.fill`, preserving existing user text.
   Do not overwrite a prompt edited while transcription was pending.
4. Let the user review and press Enter. Never call `prompt.submit`, generate
   synthetic keystrokes, or execute the transcript as a command. Cancel must
   stop capture, discard late results, and leave the prompt unchanged.

The [mods reference](https://code.claude.com/docs/en/plugins/mods/reference)
lists `prompt.read`/`prompt.fill` and process APIs; its audio namespace offers
`play` and `speak`, with no documented microphone recording method. That is why
the proposed design needs a recorder helper. Pin a tested Claude Code version
and use the generated `.claude-plugin/types/` declarations as the authority for
exact signatures. Validate the plugin and test cancel, timeout, microphone
denial, malformed/empty results, and concurrent prompt edits before use. See
[Create a mod](https://code.claude.com/docs/en/plugins/mods/create).

## Privacy and operating boundaries

Keep the service and helper on the microphone's machine and bind to loopback;
do not expose an unauthenticated transcription port to the network. A remote
shell has no automatic access to your laptop microphone. See
[deployment security](deployment-security.md) for non-local deployment.

Local transcription keeps audio out of a cloud STT service when this localhost
flow is used. It does **not** mean audio never touches disk: the current
[runtime adapter](../crates/aximo-inference/src/runtime.rs) materializes a
temporary WAV, and a recorder may also create files. Use restrictive temporary
file permissions and cleanup on success, cancellation, and errors; avoid audio
or transcript logging. Submitted Claude prompts still go to the configured
model provider. Install only reviewed mods and helpers: mods run with your user
permissions, not inside Claude Code's Bash sandbox.
