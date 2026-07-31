//! Modified MD5 used by FairPlay session-key derivation (ported from ModifiedMD5.java).

const SHIFT: [i32; 64] = [
    7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9,
    14, 20, 5, 9, 14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10, 15, 21,
    6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
];

/// Port of Java `ModifiedMD5.modified_md5`.
///
/// `originalblock_in` must be at least 64 bytes; only the first 64 are used.
/// `key_in` / `key_out` are 16-byte little-endian words (4 × u32).
pub fn modified_md5(originalblock_in: &[u8], key_in: &[u8], key_out: &mut [u8]) {
    let mut block_in = [0u8; 64];
    block_in.copy_from_slice(&originalblock_in[..64]);

    let mut a = u32::from_le_bytes(key_in[0..4].try_into().unwrap()) as i64 & 0xffff_ffff;
    let mut b = u32::from_le_bytes(key_in[4..8].try_into().unwrap()) as i64 & 0xffff_ffff;
    let mut c = u32::from_le_bytes(key_in[8..12].try_into().unwrap()) as i64 & 0xffff_ffff;
    let mut d = u32::from_le_bytes(key_in[12..16].try_into().unwrap()) as i64 & 0xffff_ffff;

    for i in 0..64 {
        let j = if i < 16 {
            i
        } else if i < 32 {
            (5 * i + 1) % 16
        } else if i < 48 {
            (3 * i + 5) % 16
        } else {
            (7 * i) % 16
        };

        let input = ((block_in[4 * j] as i32 & 0xff) << 24)
            | ((block_in[4 * j + 1] as i32 & 0xff) << 16)
            | ((block_in[4 * j + 2] as i32 & 0xff) << 8)
            | (block_in[4 * j + 3] as i32 & 0xff);

        // Java: (long) ((1L << 32) * Math.abs(Math.sin(i + 1)))
        let sin_term = ((1i64 << 32) as f64 * (i as f64 + 1.0).sin().abs()) as i64;

        let mut z = a + input as i64 + sin_term;
        if i < 16 {
            z = rol(z + f(b, c, d), SHIFT[i] as i64);
        } else if i < 32 {
            z = rol(z + g(b, c, d), SHIFT[i] as i64);
        } else if i < 48 {
            z = rol(z + h(b, c, d), SHIFT[i] as i64);
        } else {
            z = rol(z + i_fn(b, c, d), SHIFT[i] as i64);
        }
        z += b;
        let tmp = d;
        d = c;
        c = b;
        b = z;
        a = tmp;

        if i == 31 {
            // swapsies
            swap_le_u32(&mut block_in, 4 * ((a & 15) as usize), 4 * ((b & 15) as usize));
            swap_le_u32(&mut block_in, 4 * ((c & 15) as usize), 4 * ((d & 15) as usize));
            swap_le_u32(
                &mut block_in,
                4 * (((a & (15 << 4)) >> 4) as usize),
                4 * (((b & (15 << 4)) >> 4) as usize),
            );
            swap_le_u32(
                &mut block_in,
                4 * (((a & (15 << 8)) >> 8) as usize),
                4 * (((b & (15 << 8)) >> 8) as usize),
            );
            swap_le_u32(
                &mut block_in,
                4 * (((a & (15 << 12)) >> 12) as usize),
                4 * (((b & (15 << 12)) >> 12) as usize),
            );
        }
    }

    let k0 = u32::from_le_bytes(key_in[0..4].try_into().unwrap());
    let k1 = u32::from_le_bytes(key_in[4..8].try_into().unwrap());
    let k2 = u32::from_le_bytes(key_in[8..12].try_into().unwrap());
    let k3 = u32::from_le_bytes(key_in[12..16].try_into().unwrap());

    key_out[0..4].copy_from_slice(&(k0.wrapping_add(a as u32)).to_le_bytes());
    key_out[4..8].copy_from_slice(&(k1.wrapping_add(b as u32)).to_le_bytes());
    key_out[8..12].copy_from_slice(&(k2.wrapping_add(c as u32)).to_le_bytes());
    key_out[12..16].copy_from_slice(&(k3.wrapping_add(d as u32)).to_le_bytes());
}

fn f(b: i64, c: i64, d: i64) -> i64 {
    (b & c) | (!b & d)
}

fn g(b: i64, c: i64, d: i64) -> i64 {
    (b & d) | (c & !d)
}

fn h(b: i64, c: i64, d: i64) -> i64 {
    b ^ c ^ d
}

fn i_fn(b: i64, c: i64, d: i64) -> i64 {
    c ^ (b | !d)
}

fn rol(input: i64, count: i64) -> i64 {
    // Java: ((input << count) & 0xffffffffL) | (input & 0xffffffffL) >> (32 - count)
    ((input << count) & 0xffff_ffff) | ((input & 0xffff_ffff) >> (32 - count))
}

fn swap_le_u32(arr: &mut [u8], idx_a: usize, idx_b: usize) {
    let a = u32::from_le_bytes(arr[idx_a..idx_a + 4].try_into().unwrap());
    let b = u32::from_le_bytes(arr[idx_b..idx_b + 4].try_into().unwrap());
    arr[idx_b..idx_b + 4].copy_from_slice(&a.to_le_bytes());
    arr[idx_a..idx_a + 4].copy_from_slice(&b.to_le_bytes());
}
