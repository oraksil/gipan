pub mod converter {
    // https://en.wikipedia.org/wiki/Chroma_subsampling
    pub fn bgra_to_yuv420(width: usize, height: usize, bgra: &[u8], y: &mut [u8], u: &mut [u8], v: &mut [u8]) {
        const BYTES_PER_PIXEL: usize = 4;

        let num_pixels = width * height;
        let mut uv_idx = 0;
        for i in 0..num_pixels {
            let b = i32::from(bgra[(i*BYTES_PER_PIXEL) + 0]);
            let g = i32::from(bgra[(i*BYTES_PER_PIXEL) + 1]);
            let r = i32::from(bgra[(i*BYTES_PER_PIXEL) + 2]);

            // w = 10
            //  0  1  2  3  4  5  6  7  8  9
            // 10 11 12 13 14 15 16 17 18 19
            // (0, 1, 10, 11) --> Same U, V
            // (2, 3, 12, 13) --> Same U, V
            // (4, 5, 14, 15) --> Same U, V
            // ...

            y[i] = clamp((77*r + 150*g + 29*b + 128) >> 8);
            if i % 2 == 0 && (i/width) % 2 == 0 {
                u[uv_idx] = clamp(((-43*r - 84*g + 127*b) >> 8) + 128);
                v[uv_idx] = clamp(((127*r - 106*g - 21*b) >> 8) + 128);
                uv_idx += 1;
            }
        }
    }

    fn clamp(val: i32) -> u8 {
        match val {
            ref v if *v < 0 => 0,
            ref v if *v > 255 => 255,
            v => v as u8,
        }
    }

}