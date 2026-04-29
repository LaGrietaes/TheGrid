# TheGrid Media Care Dependency Matrix v1

Status: Draft v1
Date: 2026-04-29
Depends on: MEDIA_CARE_BLUEPRINT_V1.md

## 1. Goal
Provide a practical dependency decision matrix for media processing stack selection across image, video, audio, and AI operations.

## 2. Decision Summary
Recommended baseline:
- FFmpeg orchestration: ffmpeg-sidecar
- Image processing: image (existing)
- Audio decode/analysis: symphonia + hound + rubato
- Silence detection: silero (fallback webrtc-vad)
- Denoise: nnnoiseless (optional deep_filter profile)
- Transcription: whisper-rs
- ONNX runtime: ort
- Stabilization: external Gyroflow integration in v1

## 3. Matrix

| Domain | Candidate | License | Maturity | Platform | Perf | Integration Complexity | Risk | Decision |
|---|---|---|---|---|---|---|---|---|
| FFmpeg orchestration | ffmpeg-sidecar | MIT | Good | Win/Linux/macOS | High (delegated to ffmpeg) | Low | Low | Adopt |
| FFmpeg native wrapper | rsmpeg | MIT | Good | Win/Linux/macOS | High | Medium/High | Medium (ffi complexity) | Keep as future option |
| FFmpeg high-level wrapper | ffmpeg-next | WTFPL | Mature | Win/Linux/macOS | High | Medium | Medium/High (license posture) | Do not use in v1 |
| Audio decode | symphonia | MPL-2.0 | Good | Cross-platform | Good | Medium | Low/Medium | Adopt |
| Audio resample | rubato | MIT | Good | Cross-platform | Good | Low | Low | Adopt |
| WAV IO | hound | Apache-2.0 | Mature | Cross-platform | Good | Low | Low | Adopt |
| VAD | silero | MIT OR Apache-2.0 | Good | Cross-platform | Good | Medium | Low | Adopt primary |
| VAD fallback | webrtc-vad | MIT | Good | Cross-platform | Very good | Medium | Low | Adopt fallback |
| Denoise baseline | nnnoiseless | BSD-3-Clause | Good | Cross-platform | Good | Medium | Low | Adopt baseline |
| Denoise advanced | deep_filter | MIT/Apache-2.0 | Good | Cross-platform | Medium/High | Medium/High | Medium (heavier runtime) | Optional profile |
| Transcription | whisper-rs | Unlicense | Mature community | Cross-platform | Medium/High | Medium | Medium (model size/runtime) | Adopt optional in v1 |
| ONNX runtime | ort | MIT OR Apache-2.0 | Good | Win/Linux/macOS | High | Medium | Medium (binary/runtime mgmt) | Adopt for AI features |
| Stabilization app | Gyroflow external | GPLv3 app | Mature | Win/Linux/macOS | High | Medium | Medium (tool presence/licensing) | Adopt external workflow |

## 4. Why These Choices
- Minimize custom codec pipeline complexity by keeping ffmpeg external
- Prioritize permissive or operationally acceptable licenses for application distribution
- Keep heavy AI features optional and capability-detected
- Avoid tight coupling to GPL internals by integrating Gyroflow as external app in v1

## 5. Packaging and Distribution Notes
- ffmpeg binaries must be discoverable via PATH or configured location
- gyroflow executable path must be configurable and validated
- ai model files should be optional downloads with clear size/compute hints

## 6. Runtime Capability Tiers
Define runtime feature gates:
- Tier 0: image-only operations available
- Tier 1: ffmpeg available (video/audio transforms)
- Tier 2: ffmpeg + vad/denoise available
- Tier 3: transcription and AI recommendations enabled
- Tier 4: gyro stabilization enabled

UI must reflect active tier and hide unsupported actions or show install guidance.

## 7. License and Compliance Checklist
Before each release:
1. Validate all dependency licenses and transitive updates
2. Ensure attribution/notice requirements are satisfied
3. Reconfirm Gyroflow integration remains external in v1
4. Verify no copyleft linkage is introduced accidentally

## 8. Dependency Lock Policy
- Pin versions in workspace manifests for reproducibility
- Upgrade in controlled batches by domain
- Record benchmark and regression notes per upgrade

## 9. Validation Tests per Dependency Domain
- ffmpeg-sidecar: command spawn, failure propagation, timeout handling
- symphonia/rubato/hound: decode and sample pipeline integrity
- silero/webrtc-vad: silence cut correctness on known fixtures
- nnnoiseless/deep_filter: denoise quality and runtime bounds
- whisper-rs/ort: model load time and inference stability
- gyroflow external: detection, invocation, artifact reimport

## 10. Final Decision Set (v1)
Mandatory:
- ffmpeg-sidecar
- symphonia
- rubato
- hound
- silero
- nnnoiseless

Optional feature flags:
- webrtc-vad fallback
- deep_filter advanced denoise
- whisper-rs transcription
- ort onnx-backed ai assist
- gyroflow external integration adapter
