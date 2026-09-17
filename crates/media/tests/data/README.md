`rgb-gop-v3.mkv` is a synthetic, independently encoded interoperability fixture:
6 RGB frames, 16×16, 25 Hz; byte `i` of frame `f` is
`(i * 37 + f * 53 + i / 7) % 256` (integer division).

Encoded once with FFmpeg 9.0.1: FFV1 version 3, range coder 1, GOP length 3,
four slices, slice CRC enabled, `bgr0`. Frames 1/2/4/5 retain entropy state from
previous frames. No source game assets are included. Tests read the checked-in
fixture directly and require no external tools.
