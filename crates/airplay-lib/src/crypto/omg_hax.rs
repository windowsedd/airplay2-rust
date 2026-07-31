//! OmgHax FairPlay crypto (ported from OmgHax.java).

use super::modified_md5::modified_md5;
use super::omg_hax_const::{
    DEFAULT_SAP, INDEX_MANGLE, INITIAL_SESSION_KEY, MESSAGE_IV, MESSAGE_KEY, STATIC_SOURCE_1,
    STATIC_SOURCE_2, TABLE_S1, TABLE_S10, TABLE_S2, TABLE_S3, TABLE_S4, TABLE_S5, TABLE_S6, TABLE_S7,
    TABLE_S8, TABLE_S9, T_KEY, X_KEY, Z_KEY,
};
use super::sap_hash::sap_hash;

/// FairPlay OmgHax crypto engine.
#[derive(Debug, Default, Clone)]
pub struct OmgHax;

impl OmgHax {
    pub fn new() -> Self {
        Self
    }

    /// Decrypt AES key from cipher material using `message3` (key_msg from fp-setup).
    ///
    /// Port of Java `decryptAesKey(message3, cipherText, keyOut)`.
    pub fn decrypt_aes_key(&self, message3: &[u8], cipher_text: &[u8], key_out: &mut [u8; 16]) {
        let chunk1 = &cipher_text[16..];
        let chunk2 = &cipher_text[56..];
        let mut block_in = [0u8; 16];
        let mut sap_key = [0u8; 16];
        let mut key_schedule = [[0i32; 4]; 11];

        self.generate_session_key(&DEFAULT_SAP, message3, &mut sap_key);
        self.generate_key_schedule(&sap_key, &mut key_schedule);
        z_xor(chunk2, &mut block_in, 1);
        self.cycle(&mut block_in, &key_schedule);
        for i in 0..16 {
            key_out[i] = block_in[i] ^ chunk1[i];
        }
        let mut tmp = *key_out;
        x_xor(&tmp, key_out, 1);
        tmp = *key_out;
        z_xor(&tmp, key_out, 1);
    }

    /// Port of Java `decryptMessage`.
    pub fn decrypt_message(&self, message_in: &[u8], decrypted_message: &mut [u8]) {
        let mut buffer = [0u8; 16];
        let mode = message_in[12] as usize; // 0,1,2,3

        for i in 0..8 {
            for j in 0..16 {
                if mode == 3 {
                    buffer[j] = message_in[(0x80 - 0x10 * i) + j];
                } else {
                    buffer[j] = message_in[(0x10 * (i + 1)) + j];
                }
            }

            for j in 0..9 {
                let base = 0x80 - 0x10 * j;
                let mk = |off: usize| MESSAGE_KEY[mode][base + off];

                buffer[0x0] = message_table_index((base + 0x0) as i32)[buffer[0x0] as usize] ^ mk(0x0);
                buffer[0x4] = message_table_index((base + 0x4) as i32)[buffer[0x4] as usize] ^ mk(0x4);
                buffer[0x8] = message_table_index((base + 0x8) as i32)[buffer[0x8] as usize] ^ mk(0x8);
                buffer[0xc] = message_table_index((base + 0xc) as i32)[buffer[0xc] as usize] ^ mk(0xc);

                let tmp = buffer[0x0d];
                buffer[0xd] = message_table_index((base + 0xd) as i32)[buffer[0x9] as usize] ^ mk(0xd);
                buffer[0x9] = message_table_index((base + 0x9) as i32)[buffer[0x5] as usize] ^ mk(0x9);
                buffer[0x5] = message_table_index((base + 0x5) as i32)[buffer[0x1] as usize] ^ mk(0x5);
                buffer[0x1] = message_table_index((base + 0x1) as i32)[tmp as usize] ^ mk(0x1);

                let tmp = buffer[0x02];
                buffer[0x2] = message_table_index((base + 0x2) as i32)[buffer[0xa] as usize] ^ mk(0x2);
                buffer[0xa] = message_table_index((base + 0xa) as i32)[tmp as usize] ^ mk(0xa);
                let tmp = buffer[0x06];
                buffer[0x6] = message_table_index((base + 0x6) as i32)[buffer[0xe] as usize] ^ mk(0x6);
                buffer[0xe] = message_table_index((base + 0xe) as i32)[tmp as usize] ^ mk(0xe);

                let tmp = buffer[0x3];
                buffer[0x3] = message_table_index((base + 0x3) as i32)[buffer[0x7] as usize] ^ mk(0x3);
                buffer[0x7] = message_table_index((base + 0x7) as i32)[buffer[0xb] as usize] ^ mk(0x7);
                buffer[0xb] = message_table_index((base + 0xb) as i32)[buffer[0xf] as usize] ^ mk(0xb);
                buffer[0xf] = message_table_index((base + 0xf) as i32)[tmp as usize] ^ mk(0xf);

                // T-table mix (little-endian putInt of XOR'd i32 words)
                let w0 = TABLE_S9[0x000 + buffer[0x0] as usize]
                    ^ TABLE_S9[0x100 + buffer[0x1] as usize]
                    ^ TABLE_S9[0x200 + buffer[0x2] as usize]
                    ^ TABLE_S9[0x300 + buffer[0x3] as usize];
                let w1 = TABLE_S9[0x000 + buffer[0x4] as usize]
                    ^ TABLE_S9[0x100 + buffer[0x5] as usize]
                    ^ TABLE_S9[0x200 + buffer[0x6] as usize]
                    ^ TABLE_S9[0x300 + buffer[0x7] as usize];
                let w2 = TABLE_S9[0x000 + buffer[0x8] as usize]
                    ^ TABLE_S9[0x100 + buffer[0x9] as usize]
                    ^ TABLE_S9[0x200 + buffer[0xa] as usize]
                    ^ TABLE_S9[0x300 + buffer[0xb] as usize];
                let w3 = TABLE_S9[0x000 + buffer[0xc] as usize]
                    ^ TABLE_S9[0x100 + buffer[0xd] as usize]
                    ^ TABLE_S9[0x200 + buffer[0xe] as usize]
                    ^ TABLE_S9[0x300 + buffer[0xf] as usize];
                buffer[0..4].copy_from_slice(&w0.to_le_bytes());
                buffer[4..8].copy_from_slice(&w1.to_le_bytes());
                buffer[8..12].copy_from_slice(&w2.to_le_bytes());
                buffer[12..16].copy_from_slice(&w3.to_le_bytes());
            }

            buffer[0x0] = TABLE_S10[(0x0 << 8) + buffer[0x0] as usize];
            buffer[0x4] = TABLE_S10[(0x4 << 8) + buffer[0x4] as usize];
            buffer[0x8] = TABLE_S10[(0x8 << 8) + buffer[0x8] as usize];
            buffer[0xc] = TABLE_S10[(0xc << 8) + buffer[0xc] as usize];

            let tmp = buffer[0x0d];
            buffer[0xd] = TABLE_S10[(0xd << 8) + buffer[0x9] as usize];
            buffer[0x9] = TABLE_S10[(0x9 << 8) + buffer[0x5] as usize];
            buffer[0x5] = TABLE_S10[(0x5 << 8) + buffer[0x1] as usize];
            buffer[0x1] = TABLE_S10[(0x1 << 8) + tmp as usize];

            let tmp = buffer[0x02];
            buffer[0x2] = TABLE_S10[(0x2 << 8) + buffer[0xa] as usize];
            buffer[0xa] = TABLE_S10[(0xa << 8) + tmp as usize];
            let tmp = buffer[0x06];
            buffer[0x6] = TABLE_S10[(0x6 << 8) + buffer[0xe] as usize];
            buffer[0xe] = TABLE_S10[(0xe << 8) + tmp as usize];

            let tmp = buffer[0x3];
            buffer[0x3] = TABLE_S10[(0x3 << 8) + buffer[0x7] as usize];
            buffer[0x7] = TABLE_S10[(0x7 << 8) + buffer[0xb] as usize];
            buffer[0xb] = TABLE_S10[(0xb << 8) + buffer[0xf] as usize];
            buffer[0xf] = TABLE_S10[(0xf << 8) + tmp as usize];

            let mut xor_result = [0u8; 16];
            if mode == 2 || mode == 1 || mode == 0 {
                if i > 0 {
                    xor_blocks(
                        &buffer,
                        &message_in[0x10 * i..0x10 * i + 16],
                        &mut xor_result,
                    );
                    decrypted_message[0x10 * i..0x10 * i + 16].copy_from_slice(&xor_result);
                } else {
                    xor_blocks(&buffer, &MESSAGE_IV[mode], &mut xor_result);
                    decrypted_message[0x10 * i..0x10 * i + 16].copy_from_slice(&xor_result);
                }
            } else if i < 7 {
                xor_blocks(
                    &buffer,
                    &message_in[0x70 - 0x10 * i..(0x70 - 0x10 * i) + 16],
                    &mut xor_result,
                );
                decrypted_message[0x70 - 0x10 * i..0x70 - 0x10 * i + 16]
                    .copy_from_slice(&xor_result);
            } else {
                xor_blocks(&buffer, &MESSAGE_IV[mode], &mut xor_result);
                decrypted_message[0x70 - 0x10 * i..0x70 - 0x10 * i + 16]
                    .copy_from_slice(&xor_result);
            }
        }
    }

    /// Port of Java `generate_key_schedule`.
    pub fn generate_key_schedule(&self, key_material: &[u8], key_schedule: &mut [[i32; 4]; 11]) {
        let mut key_data = [0i32; 4];
        for i in 0..11 {
            key_schedule[i][0] = 0xdead_beef_u32 as i32;
            key_schedule[i][1] = 0xdead_beef_u32 as i32;
            key_schedule[i][2] = 0xdead_beef_u32 as i32;
            key_schedule[i][3] = 0xdead_beef_u32 as i32;
        }
        let mut buffer = [0u8; 16];
        let mut ti = 0i32;

        t_xor(key_material, &mut buffer);

        for i in 0..4 {
            key_data[i] = i32::from_le_bytes(buffer[i * 4..i * 4 + 4].try_into().unwrap());
        }

        for round in 0..11 {
            key_schedule[round][0] = key_data[0];

            let table1 = table_index(ti);
            let table2 = table_index(ti + 1);
            let table3 = table_index(ti + 2);
            let table4 = table_index(ti + 3);
            ti += 4;

            buffer[0] ^= table1[buffer[0x0d] as usize] ^ INDEX_MANGLE[round as usize];
            buffer[1] ^= table2[buffer[0x0e] as usize];
            buffer[2] ^= table3[buffer[0x0f] as usize];
            buffer[3] ^= table4[buffer[0x0c] as usize];

            key_data[0] = i32::from_le_bytes(buffer[0..4].try_into().unwrap());

            key_schedule[round][1] = key_data[1];
            key_data[1] ^= key_data[0];
            buffer[4..8].copy_from_slice(&key_data[1].to_le_bytes());

            key_schedule[round][2] = key_data[2];
            key_data[2] ^= key_data[1];
            buffer[8..12].copy_from_slice(&key_data[2].to_le_bytes());

            key_schedule[round][3] = key_data[3];
            key_data[3] ^= key_data[2];
            buffer[12..16].copy_from_slice(&key_data[3].to_le_bytes());
        }
    }

    /// Port of Java `generate_session_key`.
    pub fn generate_session_key(&self, old_sap: &[u8], message_in: &[u8], session_key: &mut [u8]) {
        let mut decrypted_message = [0u8; 128];
        let mut new_sap = [0u8; 320];
        let mut md5 = [0u8; 16];

        self.decrypt_message(message_in, &mut decrypted_message);

        new_sap[0..0x11].copy_from_slice(&STATIC_SOURCE_1);
        new_sap[0x11..0x11 + 0x80].copy_from_slice(&decrypted_message);
        new_sap[0x091..0x091 + 0x80].copy_from_slice(&old_sap[0x80..0x80 + 0x80]);
        new_sap[0x111..0x111 + 0x2f].copy_from_slice(&STATIC_SOURCE_2);
        session_key[..16].copy_from_slice(&INITIAL_SESSION_KEY);

        for round in 0..5 {
            let base = &new_sap[round * 64..];
            modified_md5(base, session_key, &mut md5);
            sap_hash(base, session_key);
            for i in 0..4 {
                let sk = u32::from_le_bytes(session_key[i * 4..i * 4 + 4].try_into().unwrap());
                let m = u32::from_le_bytes(md5[i * 4..i * 4 + 4].try_into().unwrap());
                session_key[i * 4..i * 4 + 4].copy_from_slice(&sk.wrapping_add(m).to_le_bytes());
            }
        }

        for i in (0..16).step_by(4) {
            session_key.swap(i, i + 3);
            session_key.swap(i + 1, i + 2);
        }

        for i in 0..16 {
            session_key[i] ^= 121;
        }
    }

    /// Port of Java `cycle`.
    pub fn cycle(&self, block: &mut [u8], key_schedule: &[[i32; 4]; 11]) {
        for i in 0..4 {
            let v = i32::from_le_bytes(block[i * 4..i * 4 + 4].try_into().unwrap())
                ^ key_schedule[10][i];
            block[i * 4..i * 4 + 4].copy_from_slice(&v.to_le_bytes());
        }

        permute_block_1(block);

        for round in 0..9 {
            let mut key = [0u8; 16];
            for i in 0..4 {
                key[i * 4..i * 4 + 4].copy_from_slice(&key_schedule[9 - round][i].to_le_bytes());
            }

            let ptr1 = TABLE_S5[(block[3] ^ key[3]) as usize];
            let ptr2 = TABLE_S6[(block[2] ^ key[2]) as usize];
            let ptr3 = TABLE_S8[(block[0] ^ key[0]) as usize];
            let ptr4 = TABLE_S7[(block[1] ^ key[1]) as usize];
            let ab = ptr1 ^ ptr2 ^ ptr3 ^ ptr4;
            block[0..4].copy_from_slice(&ab.to_le_bytes());

            let ptr2 = TABLE_S5[(block[7] ^ key[7]) as usize];
            let ptr1 = TABLE_S6[(block[6] ^ key[6]) as usize];
            let ptr4 = TABLE_S7[(block[5] ^ key[5]) as usize];
            let ptr3 = TABLE_S8[(block[4] ^ key[4]) as usize];
            let ab = ptr1 ^ ptr2 ^ ptr3 ^ ptr4;
            block[4..8].copy_from_slice(&ab.to_le_bytes());

            let ab = TABLE_S5[(block[11] ^ key[11]) as usize]
                ^ TABLE_S6[(block[10] ^ key[10]) as usize]
                ^ TABLE_S7[(block[9] ^ key[9]) as usize]
                ^ TABLE_S8[(block[8] ^ key[8]) as usize];
            block[8..12].copy_from_slice(&ab.to_le_bytes());

            let ab = TABLE_S5[(block[15] ^ key[15]) as usize]
                ^ TABLE_S6[(block[14] ^ key[14]) as usize]
                ^ TABLE_S7[(block[13] ^ key[13]) as usize]
                ^ TABLE_S8[(block[12] ^ key[12]) as usize];
            block[12..16].copy_from_slice(&ab.to_le_bytes());

            permute_block_2(block, 8 - round as i32);
        }

        for i in 0..4 {
            let v =
                i32::from_le_bytes(block[i * 4..i * 4 + 4].try_into().unwrap()) ^ key_schedule[0][i];
            block[i * 4..i * 4 + 4].copy_from_slice(&v.to_le_bytes());
        }
    }
}

fn xor_blocks(a: &[u8], b: &[u8], out: &mut [u8]) {
    for i in 0..16 {
        out[i] = a[i] ^ b[i];
    }
}

fn z_xor(input: &[u8], out: &mut [u8], blocks: usize) {
    for j in 0..blocks {
        for i in 0..16 {
            out[j * 16 + i] = input[j * 16 + i] ^ Z_KEY[i];
        }
    }
}

fn x_xor(input: &[u8], out: &mut [u8], blocks: usize) {
    for j in 0..blocks {
        for i in 0..16 {
            out[j * 16 + i] = input[j * 16 + i] ^ X_KEY[i];
        }
    }
}

fn t_xor(input: &[u8], out: &mut [u8]) {
    for i in 0..16 {
        out[i] = input[i] ^ T_KEY[i];
    }
}

fn table_index(i: i32) -> &'static [u8] {
    let start = (((31 * i) % 0x28) << 8) as usize;
    &TABLE_S1[start..]
}

fn message_table_index(i: i32) -> &'static [u8] {
    let start = ((97 * i % 144) << 8) as usize;
    &TABLE_S2[start..]
}

fn permute_block_1(block: &mut [u8]) {
    block[0] = TABLE_S3[block[0] as usize];
    block[4] = TABLE_S3[0x400 + block[4] as usize];
    block[8] = TABLE_S3[0x800 + block[8] as usize];
    block[12] = TABLE_S3[0xc00 + block[12] as usize];

    let tmp = block[13];
    block[13] = TABLE_S3[0x100 + block[9] as usize];
    block[9] = TABLE_S3[0xd00 + block[5] as usize];
    block[5] = TABLE_S3[0x900 + block[1] as usize];
    block[1] = TABLE_S3[0x500 + tmp as usize];

    let tmp = block[2];
    block[2] = TABLE_S3[0xa00 + block[10] as usize];
    block[10] = TABLE_S3[0x200 + tmp as usize];
    let tmp = block[6];
    block[6] = TABLE_S3[0xe00 + block[14] as usize];
    block[14] = TABLE_S3[0x600 + tmp as usize];

    let tmp = block[3];
    block[3] = TABLE_S3[0xf00 + block[7] as usize];
    block[7] = TABLE_S3[0x300 + block[11] as usize];
    block[11] = TABLE_S3[0x700 + block[15] as usize];
    block[15] = TABLE_S3[0xb00 + tmp as usize];
}

fn permute_table_2(i: i32) -> &'static [u8] {
    let start = (((71 * i) % 144) << 8) as usize;
    &TABLE_S4[start..]
}

fn permute_block_2(block: &mut [u8], round: i32) {
    block[0] = permute_table_2(round * 16 + 0)[block[0] as usize];
    block[4] = permute_table_2(round * 16 + 4)[block[4] as usize];
    block[8] = permute_table_2(round * 16 + 8)[block[8] as usize];
    block[12] = permute_table_2(round * 16 + 12)[block[12] as usize];

    let tmp = block[13];
    block[13] = permute_table_2(round * 16 + 13)[block[9] as usize];
    block[9] = permute_table_2(round * 16 + 9)[block[5] as usize];
    block[5] = permute_table_2(round * 16 + 5)[block[1] as usize];
    block[1] = permute_table_2(round * 16 + 1)[tmp as usize];

    let tmp = block[2];
    block[2] = permute_table_2(round * 16 + 2)[block[10] as usize];
    block[10] = permute_table_2(round * 16 + 10)[tmp as usize];
    let tmp = block[6];
    block[6] = permute_table_2(round * 16 + 6)[block[14] as usize];
    block[14] = permute_table_2(round * 16 + 14)[tmp as usize];

    let tmp = block[3];
    block[3] = permute_table_2(round * 16 + 3)[block[7] as usize];
    block[7] = permute_table_2(round * 16 + 7)[block[11] as usize];
    block[11] = permute_table_2(round * 16 + 11)[block[15] as usize];
    block[15] = permute_table_2(round * 16 + 15)[tmp as usize];
}
