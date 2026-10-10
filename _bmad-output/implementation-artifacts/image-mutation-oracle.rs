// SPDX-License-Identifier: AGPL-3.0-or-later
//! Standalone agent-only mutation runner linked against the actual Cargo artifact.
use atelier_image::{DecodeLimits, Image, ImageError, decode_bmp, decode_gif, decode_png};
type Decoder = fn(&[u8], DecodeLimits) -> Result<Image, ImageError>;

fn check(decoder: Decoder, data: &[u8], count: &mut usize) {
    let limits = DecodeLimits { max_pixels: 65_536, max_input_bytes: 1_048_576 };
    let result = std::panic::catch_unwind(|| decoder(data, limits));
    assert!(result.is_ok(), "decoder panicked, case {}", count);
    if let Ok(Ok(image)) = result {
        assert!(u64::from(image.width)*u64::from(image.height) <= limits.max_pixels);
        assert_eq!(image.pixels.len(), image.width as usize*image.height as usize*4);
    }
    *count += 1;
}

fn main() {
    let root = std::path::PathBuf::from(std::env::args().nth(1).unwrap());
    let manifest = std::fs::read_to_string(root.join("cases.tsv")).unwrap();
    let mut count = 0;
    for line in manifest.lines() {
        let fields: Vec<_> = line.split('\t').collect();
        let decoder: Decoder = match fields[0] {
            "png" => decode_png, "gif" => decode_gif, "bmp" => decode_bmp,
            _ => panic!("unknown format"),
        };
        let data = std::fs::read(root.join(fields[1])).unwrap();
        let cuts = (0..data.len().min(160)).chain((0..160).map(|i| i*data.len()/160));
        for cut in cuts { check(decoder, &data[..cut], &mut count); }
        for i in 0..data.len().min(160) {
            for value in [0, 0xff, data[i]^0x80] {
                let mut changed = data.clone(); changed[i] = value;
                check(decoder, &changed, &mut count);
            }
        }
    }
    println!("{{\"mutation_cases\":{count},\"panics\":0}}");
}
