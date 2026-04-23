# Client Examples

This page shows practical client-side integrations with the `aximo` API.

Assumptions used below:

- Base URL: `http://127.0.0.1:8080`
- Short-audio requests send a whole file to `POST /v1/transcriptions`
- Realtime examples stream a pre-existing `.pcm` file to `GET /v1/realtime`
- Realtime audio must be raw `pcm_s16le`, `16 kHz`, mono

If your source audio is WAV/MP3/M4A, use `POST /v1/transcriptions`. If you want realtime streaming, convert the input into raw PCM first.

## TypeScript

### Short Audio

This example uploads a local WAV file using modern Node.js with built-in `fetch`.

```ts
import { readFile } from "node:fs/promises";

const baseUrl = "http://127.0.0.1:8080";
const audio = await readFile("./sample.wav");

const response = await fetch(`${baseUrl}/v1/transcriptions`, {
  method: "POST",
  headers: {
    "Content-Type": "audio/wav",
  },
  body: audio,
});

if (!response.ok) {
  throw new Error(`Transcription failed: ${response.status} ${await response.text()}`);
}

const payload = await response.json();
console.log(payload);
```

### Realtime

This example uses the `ws` package and streams a `.pcm` file in 100 ms chunks.

Install:

```bash
npm install ws
```

```ts
import { readFile } from "node:fs/promises";
import WebSocket from "ws";

const baseUrl = "ws://127.0.0.1:8080/v1/realtime";
const pcm = await readFile("./sample.pcm");
const chunkSize = 3200; // 100 ms of 16 kHz mono s16le audio

const socket = new WebSocket(baseUrl);

socket.on("message", (data) => {
  console.log("server:", data.toString());
});

socket.on("open", async () => {
  socket.send(JSON.stringify({ event: "start" }));

  for (let offset = 0; offset < pcm.length; offset += chunkSize) {
    socket.send(pcm.subarray(offset, offset + chunkSize));
    await new Promise((resolve) => setTimeout(resolve, 100));
  }

  socket.send(JSON.stringify({ event: "stop" }));
});

socket.on("error", (error) => {
  console.error("websocket error:", error);
});
```

## Python

### Short Audio

Install:

```bash
pip install httpx
```

```python
from pathlib import Path

import httpx

base_url = "http://127.0.0.1:8080"
audio_bytes = Path("sample.wav").read_bytes()

response = httpx.post(
    f"{base_url}/v1/transcriptions",
    content=audio_bytes,
    headers={"content-type": "audio/wav"},
    timeout=120,
)
response.raise_for_status()

print(response.json())
```

### Realtime

Install:

```bash
pip install websockets
```

```python
import asyncio
from pathlib import Path

import websockets

WS_URL = "ws://127.0.0.1:8080/v1/realtime"
CHUNK_SIZE = 3200  # 100 ms of pcm_s16le, 16 kHz, mono


async def main() -> None:
    pcm = Path("sample.pcm").read_bytes()

    async with websockets.connect(WS_URL) as ws:
        await ws.send('{"event":"start"}')

        async def reader() -> None:
            try:
                async for message in ws:
                    print("server:", message)
            except websockets.ConnectionClosed:
                pass

        reader_task = asyncio.create_task(reader())

        for offset in range(0, len(pcm), CHUNK_SIZE):
            await ws.send(pcm[offset : offset + CHUNK_SIZE])
            await asyncio.sleep(0.1)

        await ws.send('{"event":"stop"}')
        await asyncio.sleep(1)
        reader_task.cancel()


asyncio.run(main())
```

## Rust

### Short Audio

`Cargo.toml`:

```toml
[dependencies]
reqwest = { version = "0.12", features = ["json", "rustls-tls"] }
tokio = { version = "1", features = ["macros", "rt-multi-thread", "fs"] }
```

```rust
use tokio::fs;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let audio = fs::read("sample.wav").await?;

    let client = reqwest::Client::new();
    let response = client
        .post("http://127.0.0.1:8080/v1/transcriptions")
        .header(reqwest::header::CONTENT_TYPE, "audio/wav")
        .body(audio)
        .send()
        .await?;

    let response = response.error_for_status()?;
    let body = response.text().await?;
    println!("{body}");

    Ok(())
}
```

### Realtime

`Cargo.toml`:

```toml
[dependencies]
futures-util = "0.3"
tokio = { version = "1", features = ["macros", "rt-multi-thread", "fs", "time"] }
tokio-tungstenite = "0.29"
```

```rust
use futures_util::{SinkExt, StreamExt};
use tokio::{fs, time::{sleep, Duration}};
use tokio_tungstenite::{connect_async, tungstenite::Message};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let pcm = fs::read("sample.pcm").await?;
    let chunk_size = 3200usize;

    let (mut socket, _) = connect_async("ws://127.0.0.1:8080/v1/realtime").await?;

    socket
        .send(Message::Text(r#"{"event":"start"}"#.into()))
        .await?;

    for offset in (0..pcm.len()).step_by(chunk_size) {
        let end = (offset + chunk_size).min(pcm.len());
        socket
            .send(Message::Binary(pcm[offset..end].to_vec().into()))
            .await?;
        sleep(Duration::from_millis(100)).await;
    }

    socket
        .send(Message::Text(r#"{"event":"stop"}"#.into()))
        .await?;

    while let Some(message) = socket.next().await {
        println!("server: {:?}", message?);
    }

    Ok(())
}
```

## Notes

- `POST /v1/transcriptions` is the easiest integration path if you already have a browser upload, a recorded WAV file, or a server-side audio file.
- `GET /v1/realtime` is lower-level and expects transport-ready PCM chunks.
- For the current `transcribe-rs` ONNX adapters, `detected_language` may be `null` and `segments` may be an empty array when the backend only exposes plain transcript text.
- Browser microphone capture is available interactively from Swagger UI at `http://127.0.0.1:8080/docs/`.
