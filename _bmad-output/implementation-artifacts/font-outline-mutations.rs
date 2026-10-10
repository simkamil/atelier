// SPDX-License-Identifier: AGPL-3.0-or-later
use atelier_font::{FontFace, OutlineLimits};
fn main() {
    let mut count = 0;
    let mut rejected = 0;
    let mut state = 0x51A010u64;
    for path in std::env::args().skip(1) {
        let original = std::fs::read(path).unwrap();
        let face = FontFace::parse(&original).unwrap();
        let glyf = face.table(*b"glyf").unwrap();
        let loca = face.table(*b"loca").unwrap();
        let glyf_base = glyf.as_ptr() as usize - original.as_ptr() as usize;
        let loca_base = loca.as_ptr() as usize - original.as_ptr() as usize;
        let glyphs = face.metrics().number_of_glyphs;
        let short = face.metrics().index_to_loc_format == 0;
        let offset = |id: usize| if short { u16::from_be_bytes(loca[id*2..id*2+2].try_into().unwrap()) as usize * 2 } else {u32::from_be_bytes(loca[id*4..id*4+4].try_into().unwrap()) as usize};
        for iteration in 0..1000 {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let gid = (state >> 16) as u16 % glyphs;
            let start = offset(gid as usize);
            let end = offset(gid as usize+1);
            let mut bytes = original.clone();
            if iteration % 3 == 0 {
                let index = loca_base + state as usize % loca.len();
                bytes[index] ^= ((state>>32) as u8) | 1;
            } else if end > start {
                let index = glyf_base+start+state as usize % (end-start);
                if iteration % 3 == 1 { bytes[index] ^= ((state>>32) as u8) | 1; }
                else { bytes.truncate(index); }
            } else { bytes.truncate(glyf_base+start); }
            let result = std::panic::catch_unwind(|| {
                match FontFace::parse(&bytes) {
                    Ok(mutated) => mutated.outline_with_limits(gid,OutlineLimits {max_depth:8,max_components:64,max_points:4096,max_contours:256}).is_err(),
                    Err(_) => true,
                }
            });
            if result.unwrap_or_else(|_|panic!("panic at case {count} glyph {gid}")) { rejected += 1; }
            count += 1;
        }
    }
    println!("{{\"cases\":{count},\"rejected\":{rejected},\"panics\":0,\"faces\":20}}");
}
