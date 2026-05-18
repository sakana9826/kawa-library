# FFmpeg binaries

Put bundled FFmpeg binaries here before packaging:

- `ffmpeg.exe`
- `ffprobe.exe`

At runtime the app searches this packaged `ffmpeg` resource directory first, then falls back to the system `PATH`.
