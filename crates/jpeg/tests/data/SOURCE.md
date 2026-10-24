# Test images

The JPEG files come from libjpeg-turbo 3.0.0, `testimages/`
(https://github.com/libjpeg-turbo/libjpeg-turbo/tree/3.0.0/testimages), under the IJG License
(README.ijg in this directory). The other two files are derived from them as described below.
`SHA256SUMS` lists every file.

| File | What it is |
|---|---|
| `testorig.jpg` | 227×149 YCbCr, 2×2 luma sampling, baseline Huffman (SOF0) |
| `testimgari.jpg` | the same image, arithmetic coded (SOF9), one interleaved scan |
| `testimgari-huffman.jpg` | `testimgari.jpg` losslessly transcoded to baseline Huffman (SOF0) |
| `testorig12.jpg` | 227×149 YCbCr, 12-bit extended sequential Huffman (SOF1) |
| `testorig12.ppm` | libjpeg-turbo's decode of `testorig12.jpg`: 12-bit RGB, binary PPM, maxval 4095 |

`testimgari.jpg` and `testorig.jpg` share their quantization tables but not their coefficients:
libjpeg-turbo decodes them to different pixels. `testimgari-huffman.jpg` holds exactly the
coefficients of `testimgari.jpg`, so the two must decode to identical samples.

## How the derived files were made

`testimgari-huffman.jpg`, with jpegtran from libjpeg-turbo 3.1.0 (Alpine 3.22,
`libjpeg-turbo-utils-3.1.0-r0`):

```sh
jpegtran -copy none testimgari.jpg > testimgari-huffman.jpg
```

`testorig12.ppm`, with libjpeg-turbo 3.2.0 through imagecodecs 2026.10.10 (default settings:
integer IDCT `JDCT_ISLOW`, fancy upsampling), run as
`uv run --with imagecodecs --with numpy python make.py testorig12.jpg testorig12.ppm`:

```python
import sys, imagecodecs, numpy as np
src, dst = sys.argv[1], sys.argv[2]
a = imagecodecs.jpeg8_decode(open(src, "rb").read())
h, w, c = a.shape
assert c == 3 and a.dtype == np.uint16 and a.max() <= 4095
with open(dst, "wb") as f:
    f.write(b"P6\n%d %d\n4095\n" % (w, h))
    f.write(a.astype(">u2").tobytes())
```

`djpeg -dct int testorig12.jpg` from libjpeg-turbo 3.1.0 writes a byte-identical file.

"This software is based in part on the work of the Independent JPEG Group."
