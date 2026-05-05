use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::Emitter;

// ─── Global state ─────────────────────────────────────────────────────────────

pub static VOICE_ENABLED: AtomicBool = AtomicBool::new(false);

static IS_RECORDING: AtomicBool = AtomicBool::new(false);

// ─── Control ──────────────────────────────────────────────────────────────────

pub fn set_enabled(enabled: bool, app: &tauri::AppHandle) {
    VOICE_ENABLED.store(enabled, Ordering::SeqCst);
    if !enabled {
        IS_RECORDING.store(false, Ordering::SeqCst);
    }
    let _ = app.emit("aria-voice-toggled", enabled);
    log::info!("[voice] voice mode {}", if enabled { "ON" } else { "OFF" });
}

/// Called when Ctrl+Space is pressed. Starts a single record→energy-VAD→resample→transcribe cycle.
/// A second press cancels the active recording.
pub fn handle_hotkey(app: tauri::AppHandle) {
    if !VOICE_ENABLED.load(Ordering::SeqCst) {
        return;
    }

    if IS_RECORDING.swap(true, Ordering::SeqCst) {
        // Already recording — cancel
        IS_RECORDING.store(false, Ordering::SeqCst);
        let _ = app.emit("aria-listening-stop", ());
        log::info!("[voice] recording cancelled");
        return;
    }

    let _ = app.emit("aria-listening-start", ());

    tauri::async_runtime::spawn(async move {
        let result = tokio::task::spawn_blocking(|| capture_audio(&IS_RECORDING)).await;

        IS_RECORDING.store(false, Ordering::SeqCst);
        let _ = app.emit("aria-listening-stop", ());

        match result {
            Ok(Ok(samples)) if !samples.is_empty() => {
                log::info!("[voice] {} 16 kHz samples captured — writing WAV", samples.len());

                let wav_path = match write_wav_to_file(&samples) {
                    Ok(p)  => p,
                    Err(e) => { let _ = app.emit("aria-voice-error", &e); return; }
                };
                let wav_str = wav_path.to_string_lossy().into_owned();

                // Transcribe via local sidecar (blocking; model loads once on first call)
                let transcribe_result = tokio::task::spawn_blocking(move || {
                    let r = crate::whisper_sidecar::transcribe(&wav_str, Some("auto"));
                    let _ = std::fs::remove_file(&wav_str);
                    r
                })
                .await;

                match transcribe_result {
                    Ok(Ok(text)) if !text.is_empty() => {
                        log::info!("[voice] transcribed: {:?}", text);
                        let _ = app.emit("aria-voice-transcribed", &text);
                    }
                    Ok(Ok(_))  => log::info!("[voice] empty transcription, ignoring"),
                    Ok(Err(e)) => { log::error!("[voice] STT error: {e}"); let _ = app.emit("aria-voice-error", &e); }
                    Err(e)     => log::error!("[voice] spawn error: {e}"),
                }
            }
            Ok(Ok(_))  => log::info!("[voice] capture too short, ignoring"),
            Ok(Err(e)) => { log::error!("[voice] capture error: {e}"); let _ = app.emit("aria-voice-error", &e); }
            Err(e)     => log::error!("[voice] spawn error: {e}"),
        }
    });
}

// ─── Audio capture — native rate, energy-based VAD ───────────────────────────

fn capture_audio(is_recording: &AtomicBool) -> Result<Vec<f32>, String> {
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

    let host   = cpal::default_host();
    let device = host.default_input_device()
        .ok_or_else(|| "No microphone found".to_string())?;

    // Use whatever rate/channels the device prefers — no format negotiation
    let native  = device.default_input_config()
        .map_err(|e| format!("No input config available: {e}"))?;
    let sample_rate  = native.sample_rate().0;
    let channels     = native.channels() as usize;

    log::info!("[voice] capture: {} Hz, {} channel(s) (will resample to 16 kHz)", sample_rate, channels);

    let stream_config = cpal::StreamConfig {
        channels:    native.channels(),
        sample_rate: native.sample_rate(),
        buffer_size: cpal::BufferSize::Default,
    };

    let ring: Arc<Mutex<VecDeque<f32>>> = Arc::new(Mutex::new(VecDeque::new()));
    let ring_w = ring.clone();

    let stream = device.build_input_stream::<f32, _, _>(
        &stream_config,
        move |data, _: &_| {
            ring_w.lock().unwrap().extend(data.iter().copied());
        },
        |e| log::error!("[voice] cpal error: {e}"),
        None,
    ).map_err(|e| format!("Microphone open failed: {e}"))?;

    stream.play().map_err(|e| format!("Failed to start recording: {e}"))?;

    // Energy-based VAD — same thresholds as the old D:\personal-dev\aria\voice\listener.py
    const CHUNK_MS: usize     = 30;
    const SILENCE_THRESHOLD: f32 = 0.001; // headset mics are quiet
    const SILENCE_SECS: f32   = 1.5;
    const MIN_SPEECH_SECS: f32 = 0.2;
    const MAX_SECS: f32       = 30.0;

    let chunk_per_ch = sample_rate as usize * CHUNK_MS / 1_000;  // samples/channel per chunk
    let chunk_total  = chunk_per_ch * channels;                   // interleaved samples per chunk
    let silence_limit = (SILENCE_SECS * 1_000.0 / CHUNK_MS as f32) as usize;  // 50 chunks
    let min_speech    = (MIN_SPEECH_SECS * 1_000.0 / CHUNK_MS as f32) as usize; // 7 chunks
    let max_chunks    = (MAX_SECS * 1_000.0 / CHUNK_MS as f32) as usize;       // 1000 chunks

    let mut speech_buf: Vec<f32> = Vec::new(); // mono, native rate
    let mut silence_count = 0usize;
    let mut speech_started = false;
    let mut speech_chunks = 0usize;

    for _ in 0..max_chunks {
        if !is_recording.load(Ordering::SeqCst) {
            break; // cancelled
        }

        // Wait for a full interleaved chunk
        loop {
            if ring.lock().unwrap().len() >= chunk_total { break; }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }

        let raw: Vec<f32> = {
            let mut buf = ring.lock().unwrap();
            (0..chunk_total).map(|_| buf.pop_front().unwrap()).collect()
        };

        let mono = to_mono(&raw, channels);

        // RMS energy
        let rms = (mono.iter().map(|&s| s * s).sum::<f32>() / mono.len() as f32).sqrt();
        let is_speech = rms > SILENCE_THRESHOLD;

        if is_speech {
            if !speech_started {
                speech_started = true;
                log::info!("[voice] speech started (rms={:.4})", rms);
            }
            silence_count = 0;
            speech_chunks += 1;
            speech_buf.extend_from_slice(&mono);
        } else if speech_started {
            silence_count += 1;
            speech_buf.extend_from_slice(&mono);
            if silence_count >= silence_limit {
                log::info!("[voice] silence end — {} speech chunks", speech_chunks);
                break;
            }
        }
        // pre-speech silence: discard
    }

    drop(stream);

    if speech_chunks < min_speech {
        log::info!("[voice] too short ({} chunks < {} min) — discarded", speech_chunks, min_speech);
        return Ok(Vec::new());
    }

    // Resample mono native-rate → 16 kHz
    Ok(resample_to_16k(&speech_buf, sample_rate))
}

// ─── Audio helpers ────────────────────────────────────────────────────────────

fn to_mono(interleaved: &[f32], channels: usize) -> Vec<f32> {
    if channels == 1 {
        return interleaved.to_vec();
    }
    interleaved.chunks(channels)
        .map(|ch| ch.iter().sum::<f32>() / channels as f32)
        .collect()
}

fn resample_to_16k(input: &[f32], input_rate: u32) -> Vec<f32> {
    if input_rate == 16_000 {
        return input.to_vec();
    }
    let ratio = input_rate as f32 / 16_000.0;
    let output_len = (input.len() as f32 / ratio) as usize;
    let mut output = Vec::with_capacity(output_len);
    for i in 0..output_len {
        let src_pos = i as f32 * ratio;
        let src_idx = src_pos as usize;
        let frac    = src_pos - src_idx as f32;
        let s0 = input.get(src_idx).copied().unwrap_or(0.0);
        let s1 = input.get(src_idx + 1).copied().unwrap_or(s0);
        output.push(s0 + (s1 - s0) * frac);
    }
    output
}

// ─── WAV file writer ──────────────────────────────────────────────────────────

fn write_wav_to_file(samples_f32: &[f32]) -> Result<std::path::PathBuf, String> {
    let path = std::env::temp_dir().join(format!("aria_{}.wav", uuid::Uuid::new_v4()));
    let spec = hound::WavSpec {
        channels:        1,
        sample_rate:     16_000,
        bits_per_sample: 16,
        sample_format:   hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(&path, spec)
        .map_err(|e| format!("Failed to create temp WAV: {e}"))?;
    for &s in samples_f32 {
        let s16 = (s.clamp(-1.0, 1.0) * 32_767.0) as i16;
        writer.write_sample(s16).map_err(|e| format!("WAV write error: {e}"))?;
    }
    writer.finalize().map_err(|e| format!("WAV finalize error: {e}"))?;
    Ok(path)
}

// ─── ElevenLabs TTS ───────────────────────────────────────────────────────────

pub async fn speak_text(text: &str) -> Result<(), String> {
    let api_key = match std::env::var("ELEVENLABS_API_KEY") {
        Ok(k) if !k.is_empty() => k,
        _ => return Err("ELEVENLABS_API_KEY not set — text-to-speech unavailable".to_string()),
    };

    let voice_id = std::env::var("ELEVENLABS_VOICE_ID")
        .unwrap_or_else(|_| "21m00Tcm4TlvDq8ikWAM".to_string()); // Rachel

    let client = reqwest::Client::new();
    let response = client
        .post(format!("https://api.elevenlabs.io/v1/text-to-speech/{voice_id}"))
        .header("xi-api-key", &api_key)
        .header("content-type", "application/json")
        .json(&serde_json::json!({
            "text": text,
            "model_id": "eleven_turbo_v2_5",
            "voice_settings": { "stability": 0.5, "similarity_boost": 0.8 }
        }))
        .send()
        .await
        .map_err(|e| format!("ElevenLabs request failed: {e}"))?;

    if !response.status().is_success() {
        let status = response.status();
        let body   = response.text().await.unwrap_or_default();
        return Err(format!("ElevenLabs error {status}: {body}"));
    }

    let audio_bytes = response.bytes().await
        .map_err(|e| format!("Failed to read TTS audio: {e}"))?
        .to_vec();

    tokio::task::spawn_blocking(move || play_audio(audio_bytes))
        .await
        .map_err(|e| format!("Spawn error: {e}"))?
}

fn play_audio(bytes: Vec<u8>) -> Result<(), String> {
    let cursor = std::io::Cursor::new(bytes);
    let (_stream, handle) = rodio::OutputStream::try_default()
        .map_err(|e| format!("Audio output error: {e}"))?;
    let sink = rodio::Sink::try_new(&handle)
        .map_err(|e| format!("Audio sink error: {e}"))?;
    let source = rodio::Decoder::new(cursor)
        .map_err(|e| format!("Audio decode error: {e}"))?;
    sink.append(source);
    sink.sleep_until_end();
    Ok(())
}
