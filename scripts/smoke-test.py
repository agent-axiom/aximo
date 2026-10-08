#!/usr/bin/env python3
"""Check a running Aximo instance using generated silence, without a microphone."""
import argparse
import io
import json
import urllib.error
import urllib.request
import wave


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--base-url", default="http://127.0.0.1:8080")
    args = parser.parse_args()
    base = args.base_url.rstrip("/")

    def request(path, data=None, media_type=None, expected=200):
        headers = {"Content-Type": media_type} if media_type else {}
        req = urllib.request.Request(base + path, data=data, headers=headers)
        try:
            response = urllib.request.urlopen(req, timeout=130)
        except urllib.error.HTTPError as error:
            response = error
        with response:
            body = response.read()
            if response.status != expected:
                raise RuntimeError(f"{path}: HTTP {response.status}, expected {expected}: {body[:500]!r}")
            print(f"PASS {path}: HTTP {response.status}")
            return body

    request("/health/live")
    json.loads(request("/health/ready"))
    capabilities = json.loads(request("/v1/capabilities"))
    assert "offline" in capabilities and "realtime" in capabilities, capabilities
    schema = json.loads(request("/openapi.json"))
    assert "/v1/transcriptions" in schema["paths"], schema.keys()
    assert b"aximo_http_requests_total" in request("/metrics")
    assert b"html" in request("/docs/").lower()

    audio = io.BytesIO()
    with wave.open(audio, "wb") as wav:
        wav.setnchannels(1)
        wav.setsampwidth(2)
        wav.setframerate(16000)
        wav.writeframes(b"\x00\x00" * 16000)
    result = json.loads(request("/v1/transcriptions", audio.getvalue(), "audio/wav"))
    assert isinstance(result["text"], str), result
    assert result["engine"] == capabilities["offline"]["configured_engine"], result
    assert result["duration_ms"] == 1000, result
    assert result["processing_ms"] >= 0, result

    error = json.loads(request("/v1/transcriptions", b"not audio", "text/plain", 415))
    assert error["code"] == "unsupported_media_type", error
    error = json.loads(request("/v1/transcriptions", b"\x00", "audio/pcm", 400))
    assert error["code"] == "invalid_audio", error
    print("Aximo smoke check passed. Silence verifies wiring, not speech accuracy.")


if __name__ == "__main__":
    main()
