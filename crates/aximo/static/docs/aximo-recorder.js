class AximoDocsRecorder {
  constructor() {
    this.targetSampleRate = 16000;
    this.endpointSelect = document.getElementById("aximo-endpoint");
    this.startButton = document.getElementById("aximo-start");
    this.stopButton = document.getElementById("aximo-stop");
    this.clearButton = document.getElementById("aximo-clear");
    this.statusOutput = document.getElementById("aximo-status");
    this.resultOutput = document.getElementById("aximo-result");

    this.resetState();
    this.bindEvents();
    this.setStatus("Idle.");
    this.setResult("Waiting for recording.");
  }

  bindEvents() {
    this.startButton.addEventListener("click", () => {
      void this.start();
    });
    this.stopButton.addEventListener("click", () => {
      void this.stop();
    });
    this.clearButton.addEventListener("click", () => {
      this.clear();
    });
  }

  resetState() {
    this.stream = null;
    this.audioContext = null;
    this.sourceNode = null;
    this.processorNode = null;
    this.sinkNode = null;
    this.socket = null;
    this.mode = null;
    this.isRecording = false;
    this.partialText = "";
    this.finalText = "";
    this.sessionId = "";
    this.pcmChunks = [];
    this.stopResolver = null;
  }

  clear() {
    if (this.isRecording) {
      this.setStatus("Stop the active recording before clearing the panel.");
      return;
    }

    this.partialText = "";
    this.finalText = "";
    this.sessionId = "";
    this.setStatus("Idle.");
    this.setResult("Waiting for recording.");
  }

  async start() {
    if (this.isRecording) {
      return;
    }

    if (!window.isSecureContext) {
      this.setStatus("Microphone capture needs localhost or HTTPS.");
      return;
    }

    if (!navigator.mediaDevices || !navigator.mediaDevices.getUserMedia) {
      this.setStatus("This browser does not expose microphone capture APIs.");
      return;
    }

    this.clear();
    this.mode = this.endpointSelect.value;
    this.pcmChunks = [];

    try {
      if (this.mode === "realtime") {
        await this.openRealtimeSocket();
      }

      await this.startCapture();
      this.isRecording = true;
      this.endpointSelect.disabled = true;
      this.startButton.disabled = true;
      this.stopButton.disabled = false;
      this.setStatus(
        this.mode === "realtime"
          ? "Realtime capture started. Streaming microphone audio to /v1/realtime."
          : "Short Audio capture started. A WAV file will be sent to /v1/transcriptions on stop.",
      );
    } catch (error) {
      this.setStatus(this.describeError(error));
      this.cleanupCapture();
      this.closeSocket();
      this.endpointSelect.disabled = false;
      this.startButton.disabled = false;
      this.stopButton.disabled = true;
    }
  }

  async stop() {
    if (!this.isRecording) {
      return;
    }

    this.isRecording = false;
    this.startButton.disabled = false;
    this.stopButton.disabled = true;
    this.endpointSelect.disabled = false;

    this.cleanupCapture();

    try {
      if (this.mode === "short") {
        this.setStatus("Encoding WAV and calling /v1/transcriptions...");
        const wavBlob = this.buildWavBlob();
        await this.sendShortAudio(wavBlob);
      } else {
        this.setStatus("Finalizing realtime session...");
        await this.finishRealtime();
      }
    } catch (error) {
      this.setStatus(this.describeError(error));
      this.closeSocket();
    }
  }

  async startCapture() {
    this.stream = await navigator.mediaDevices.getUserMedia({
      audio: {
        channelCount: 1,
        echoCancellation: true,
        noiseSuppression: true,
        autoGainControl: true,
      },
    });

    const AudioContextCtor = window.AudioContext || window.webkitAudioContext;
    this.audioContext = new AudioContextCtor();

    if (this.audioContext.sampleRate < this.targetSampleRate) {
      throw new Error(
        `Input sample rate ${this.audioContext.sampleRate} is below ${this.targetSampleRate}.`,
      );
    }

    await this.audioContext.resume();

    this.sourceNode = this.audioContext.createMediaStreamSource(this.stream);
    this.processorNode = this.audioContext.createScriptProcessor(4096, 1, 1);
    this.sinkNode = this.audioContext.createGain();
    this.sinkNode.gain.value = 0;

    this.processorNode.onaudioprocess = (event) => {
      const channelData = event.inputBuffer.getChannelData(0);
      const downsampled = this.downsampleBuffer(
        channelData,
        this.audioContext.sampleRate,
        this.targetSampleRate,
      );

      if (!downsampled.length) {
        return;
      }

      const pcmChunk = this.floatTo16BitPCM(downsampled);
      this.pcmChunks.push(pcmChunk);

      if (this.mode === "realtime" && this.socket && this.socket.readyState === WebSocket.OPEN) {
        this.socket.send(pcmChunk.buffer.slice(0));
      }
    };

    this.sourceNode.connect(this.processorNode);
    this.processorNode.connect(this.sinkNode);
    this.sinkNode.connect(this.audioContext.destination);
  }

  cleanupCapture() {
    if (this.processorNode) {
      this.processorNode.disconnect();
      this.processorNode.onaudioprocess = null;
      this.processorNode = null;
    }

    if (this.sourceNode) {
      this.sourceNode.disconnect();
      this.sourceNode = null;
    }

    if (this.sinkNode) {
      this.sinkNode.disconnect();
      this.sinkNode = null;
    }

    if (this.stream) {
      this.stream.getTracks().forEach((track) => track.stop());
      this.stream = null;
    }

    if (this.audioContext) {
      void this.audioContext.close();
      this.audioContext = null;
    }
  }

  async sendShortAudio(wavBlob) {
    const response = await fetch("/v1/transcriptions", {
      method: "POST",
      headers: {
        "Content-Type": "audio/wav",
      },
      body: wavBlob,
    });

    const text = await response.text();

    if (!response.ok) {
      let message = `Short Audio request failed: ${response.status}`;

      try {
        const payload = JSON.parse(text);
        if (payload.code && payload.message) {
          message = `Short Audio request failed: ${response.status} ${payload.code}: ${payload.message}`;
        } else if (text) {
          message = `Short Audio request failed: ${response.status} ${text}`;
        }
      } catch (_error) {
        if (text) {
          message = `Short Audio request failed: ${response.status} ${text}`;
        }
      }

      throw new Error(message);
    }

    const payload = JSON.parse(text);
    this.setStatus("Short Audio transcription completed.");
    this.setResult(JSON.stringify(payload, null, 2));
  }

  async openRealtimeSocket() {
    const scheme = window.location.protocol === "https:" ? "wss:" : "ws:";
    const socketUrl = `${scheme}//${window.location.host}/v1/realtime`;

    await new Promise((resolve, reject) => {
      const socket = new WebSocket(socketUrl);
      socket.binaryType = "arraybuffer";

      socket.addEventListener("open", () => {
        this.socket = socket;
        socket.send(JSON.stringify({ event: "start" }));
        resolve();
      });

      socket.addEventListener("message", (event) => {
        this.handleRealtimeMessage(event.data);
      });

      socket.addEventListener("close", () => {
        if (this.stopResolver) {
          this.stopResolver();
          this.stopResolver = null;
        }
      });

      socket.addEventListener("error", () => {
        reject(new Error("Could not connect to /v1/realtime."));
      });
    });
  }

  async finishRealtime() {
    if (!this.socket) {
      this.setStatus("Realtime socket was not initialized.");
      return;
    }

    const completion = new Promise((resolve) => {
      const timeoutId = window.setTimeout(() => {
        resolve();
      }, 4000);

      this.stopResolver = () => {
        window.clearTimeout(timeoutId);
        resolve();
      };
    });

    if (this.socket.readyState === WebSocket.OPEN) {
      this.socket.send(JSON.stringify({ event: "stop" }));
    }

    await completion;
    this.closeSocket();
    this.setStatus("Realtime transcription completed.");
    this.renderRealtimeResult();
  }

  closeSocket() {
    if (this.socket) {
      if (
        this.socket.readyState === WebSocket.OPEN ||
        this.socket.readyState === WebSocket.CONNECTING
      ) {
        this.socket.close();
      }
      this.socket = null;
    }
  }

  handleRealtimeMessage(message) {
    let payload = null;

    try {
      payload = JSON.parse(message);
    } catch (_error) {
      this.setStatus(`Received non-JSON message: ${message}`);
      return;
    }

    switch (payload.event) {
      case "session_started":
        this.sessionId = payload.session_id || "";
        this.setStatus(
          this.sessionId
            ? `Realtime session started: ${this.sessionId}`
            : "Realtime session started.",
        );
        break;
      case "partial":
        this.partialText = payload.text || "";
        this.setStatus("Receiving partial transcript...");
        this.renderRealtimeResult();
        break;
      case "final":
        this.finalText = payload.text || "";
        this.renderRealtimeResult();
        if (this.stopResolver) {
          this.stopResolver();
          this.stopResolver = null;
        }
        break;
      case "error":
        this.setStatus(
          payload.code && payload.reason
            ? `Realtime error: ${payload.code}: ${payload.reason}`
            : "Realtime endpoint returned an error event.",
        );
        this.renderRealtimeResult();
        if (this.stopResolver) {
          this.stopResolver();
          this.stopResolver = null;
        }
        break;
      default:
        this.setStatus(`Unhandled realtime event: ${payload.event}`);
    }
  }

  renderRealtimeResult() {
    const sections = [];

    if (this.sessionId) {
      sections.push(`Session:\n${this.sessionId}`);
    }

    if (this.partialText) {
      sections.push(`Partial:\n${this.partialText}`);
    }

    if (this.finalText) {
      sections.push(`Final:\n${this.finalText}`);
    }

    this.setResult(sections.join("\n\n") || "Waiting for realtime events.");
  }

  buildWavBlob() {
    const pcm = this.mergePcmChunks();
    const buffer = new ArrayBuffer(44 + pcm.length * 2);
    const view = new DataView(buffer);

    this.writeAscii(view, 0, "RIFF");
    view.setUint32(4, 36 + pcm.length * 2, true);
    this.writeAscii(view, 8, "WAVE");
    this.writeAscii(view, 12, "fmt ");
    view.setUint32(16, 16, true);
    view.setUint16(20, 1, true);
    view.setUint16(22, 1, true);
    view.setUint32(24, this.targetSampleRate, true);
    view.setUint32(28, this.targetSampleRate * 2, true);
    view.setUint16(32, 2, true);
    view.setUint16(34, 16, true);
    this.writeAscii(view, 36, "data");
    view.setUint32(40, pcm.length * 2, true);

    let offset = 44;
    for (let index = 0; index < pcm.length; index += 1) {
      view.setInt16(offset, pcm[index], true);
      offset += 2;
    }

    return new Blob([buffer], { type: "audio/wav" });
  }

  mergePcmChunks() {
    const sampleCount = this.pcmChunks.reduce((total, chunk) => total + chunk.length, 0);
    const merged = new Int16Array(sampleCount);
    let offset = 0;

    for (const chunk of this.pcmChunks) {
      merged.set(chunk, offset);
      offset += chunk.length;
    }

    return merged;
  }

  writeAscii(view, offset, value) {
    for (let index = 0; index < value.length; index += 1) {
      view.setUint8(offset + index, value.charCodeAt(index));
    }
  }

  downsampleBuffer(samples, inputRate, outputRate) {
    if (inputRate === outputRate) {
      return new Float32Array(samples);
    }

    const ratio = inputRate / outputRate;
    const newLength = Math.round(samples.length / ratio);
    const downsampled = new Float32Array(newLength);
    let inputOffset = 0;

    for (let outputOffset = 0; outputOffset < newLength; outputOffset += 1) {
      const nextInputOffset = Math.round((outputOffset + 1) * ratio);
      let total = 0;
      let count = 0;

      for (
        let sampleIndex = inputOffset;
        sampleIndex < nextInputOffset && sampleIndex < samples.length;
        sampleIndex += 1
      ) {
        total += samples[sampleIndex];
        count += 1;
      }

      downsampled[outputOffset] = count > 0 ? total / count : 0;
      inputOffset = nextInputOffset;
    }

    return downsampled;
  }

  floatTo16BitPCM(samples) {
    const pcm = new Int16Array(samples.length);

    for (let index = 0; index < samples.length; index += 1) {
      const sample = Math.max(-1, Math.min(1, samples[index]));
      pcm[index] = sample < 0 ? sample * 0x8000 : sample * 0x7fff;
    }

    return pcm;
  }

  describeError(error) {
    if (error instanceof Error) {
      return error.message;
    }

    return String(error);
  }

  setStatus(text) {
    this.statusOutput.textContent = text;
  }

  setResult(text) {
    this.resultOutput.textContent = text;
  }
}

window.addEventListener("DOMContentLoaded", () => {
  window.aximoDocsRecorder = new AximoDocsRecorder();
});
