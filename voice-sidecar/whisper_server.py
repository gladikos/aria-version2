#!/usr/bin/env python3
"""Aria voice sidecar — transcribe WAV files via faster-whisper.

Protocol (stdin/stdout, one JSON line each direction):
  Startup:  prints {"event": "ready"} once the model is loaded.
  Request:  {"id": "<str>", "wav_path": "<abs path>", "language": "auto"|"en"|"el"|...}
  Response: {"id": "<str>", "ok": true,  "text": "...", "language": "en", "duration": 3.2}
            {"id": "<str>", "ok": false, "error": "..."}
"""

import sys
import json
from faster_whisper import WhisperModel

# Model config — GPU float16 with CPU int8 fallback
try:
    print("[whisper] loading model: small float16 cuda", file=sys.stderr, flush=True)
    model = WhisperModel("small", device="cuda", compute_type="float16")
except Exception as e:
    print(f"[whisper] CUDA failed ({e}) — falling back to CPU", file=sys.stderr, flush=True)
    model = WhisperModel("small", device="cpu", compute_type="int8")
print("[whisper] ready", file=sys.stderr, flush=True)

# Announce readiness to the Rust host
print(json.dumps({"event": "ready"}), flush=True)

# Initial prompt: helps recognition of app names and Aria-specific terms
INITIAL_PROMPT = (
    "George is talking to Aria, his personal AI assistant running on Windows. "
    "App names: Word, Excel, PowerPoint, Spotify, Discord, Chrome, Firefox, "
    "VS Code, PyCharm, Outlook, Teams, Slack, Zoom, Notepad, Calculator, Steam. "
    "Other terms: Aria, NTUA, Python, Rust, Tauri, ElevenLabs, Whisper."
)

for line in sys.stdin:
    line = line.strip()
    if not line:
        continue
    req = {}
    try:
        req = json.loads(line)
        req_id   = req.get("id", "")
        wav_path = req["wav_path"]
        language = req.get("language", "auto")
        if language == "auto":
            language = None  # faster-whisper auto-detects when language=None

        segments, info = model.transcribe(
            wav_path,
            language=language,
            beam_size=5,
            vad_filter=False,   # VAD already done in Rust
            initial_prompt=INITIAL_PROMPT,
        )
        text = " ".join(seg.text.strip() for seg in segments).strip()
        print(
            f"[whisper] {text!r}  (lang={info.language}, "
            f"prob={info.language_probability:.2f}, dur={info.duration:.1f}s)",
            file=sys.stderr, flush=True,
        )

        response = {
            "id":       req_id,
            "ok":       True,
            "text":     text,
            "language": info.language,
            "duration": info.duration,
        }
    except Exception as e:
        response = {
            "id":    req.get("id", "") if isinstance(req, dict) else "",
            "ok":    False,
            "error": str(e),
        }
        print(f"[whisper] error: {e}", file=sys.stderr, flush=True)

    print(json.dumps(response), flush=True)
