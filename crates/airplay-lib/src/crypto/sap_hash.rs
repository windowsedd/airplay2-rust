//! SapHash (ported from SapHash.java).

use super::hand_garble::garble;

#[inline]
fn rol8(input: u8, count: i32) -> u8 {
    let input = input as i32;
    ((((input << count) & 0xff) | ((input & 0xff) >> (8 - count))) as u8)
}

/// Port of Java `SapHash.sap_hash`.
///
/// `block_in` is read as little-endian 32-bit words (at least 64 bytes used cyclically).
/// `key_out` must be 16 bytes.
pub fn sap_hash(block_in: &[u8], key_out: &mut [u8]) {
    // Java signed-byte literals → u8
    let mut buffer0: [u8; 20] = [
        150, 95, 198, 83, 248, 70, 204, 24, 223, 190, 178, 248, 56, 215, 236, 34, 3, 209, 32, 143,
    ];
    let mut buffer1 = [0u8; 210];
    let mut buffer2: [u8; 35] = [
        67, 84, 98, 122, 24, 195, 214, 179, 154, 86, 246, 28, 20, 63, 12, 29, 59, 54, 131, 177, 57,
        81, 74, 170, 9, 62, 254, 68, 175, 222, 195, 32, 157, 66, 58,
    ];
    let mut buffer3 = [0u8; 132];
    let mut buffer4: [u8; 21] = [
        237, 37, 209, 187, 188, 39, 159, 2, 162, 169, 17, 0, 12, 179, 82, 192, 189, 227, 27, 73,
        199,
    ];

    let i0_index: [usize; 11] = [18, 22, 23, 0, 5, 19, 32, 31, 10, 21, 30];

    // Load the input into the buffer (little-endian words, byte-swapped within word)
    for i in 0..210 {
        let word_idx = ((i % 64) >> 2) * 4;
        let in_word = if word_idx + 4 <= block_in.len() {
            u32::from_le_bytes(block_in[word_idx..word_idx + 4].try_into().unwrap()) as i32
        } else {
            // Java ByteBuffer.getInt would throw; tests always provide enough bytes via copyOfRange views
            0
        };
        let in_byte = ((in_word >> ((3 - (i % 4)) << 3)) & 0xff) as u8;
        buffer1[i] = in_byte;
    }

    // Scrambling
    for i in 0..840 {
        // unsigned 32-bit modulo
        let x = buffer1[(((i as i32).wrapping_sub(155) as u32) % 210) as usize];
        let y = buffer1[(((i as i32).wrapping_sub(57) as u32) % 210) as usize];
        let z = buffer1[(((i as i32).wrapping_sub(13) as u32) % 210) as usize];
        let w = buffer1[((i as u32) % 210) as usize];
        // Java: (byte) ((rol8(y, 5) + (rol8(z, 3) ^ w) - rol8(x, 7)) & 0xff)
        // rol8 returns byte; + and - promote with sign extension
        let y5 = rol8(y, 5);
        let z3 = rol8(z, 3);
        let x7 = rol8(x, 7);
        buffer1[i % 210] = (((y5 as i8 as i32)
            + ((z3 as i8 as i32) ^ (w as i8 as i32))
            - (x7 as i8 as i32))
            & 0xff) as u8;
    }

    garble(
        &mut buffer0,
        &mut buffer1,
        &mut buffer2,
        &mut buffer3,
        &mut buffer4,
    );

    // Fill the output with 0xE1
    for i in 0..16 {
        key_out[i] = 0xE1;
    }

    // buffer3
    for i in 0..11 {
        if i == 3 {
            key_out[i] = 0x3d;
        } else {
            key_out[i] = ((key_out[i] as i32 + buffer3[i0_index[i] * 4] as i8 as i32) & 0xff) as u8;
        }
    }

    // buffer0
    for i in 0..20 {
        key_out[i % 16] ^= buffer0[i];
    }

    // buffer2
    for i in 0..35 {
        key_out[i % 16] ^= buffer2[i];
    }

    // buffer1
    for i in 0..210 {
        key_out[i % 16] ^= buffer1[i];
    }

    // reverse-scramble
    for _j in 0..16 {
        for i in 0..16 {
            let x = key_out[(((i as i32).wrapping_sub(7) as u32) % 16) as usize];
            let y = key_out[i % 16];
            let z = key_out[(((i as i32).wrapping_sub(37) as u32) % 16) as usize];
            let w = key_out[(((i as i32).wrapping_sub(177) as u32) % 16) as usize];
            key_out[i] = rol8(x, 1) ^ y ^ rol8(z, 6) ^ rol8(w, 5);
        }
    }
}
