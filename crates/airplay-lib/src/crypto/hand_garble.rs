//! HandGarble (ported line-by-line from HandGarble.java).
//!
//! Java `byte` is signed. We store `u8` and mirror promotions:
//! - `(x & 0xff)` → unsigned (`u`)
//! - bare `byte` in arithmetic → signed (`s`)
//! - `(byte) expr` → truncate (`b`)

#[inline]
fn u(x: u8) -> i32 {
    x as i32
}

#[inline]
fn s(x: u8) -> i32 {
    x as i8 as i32
}

#[inline]
fn b(x: i32) -> u8 {
    x as u8
}

/// Port of Java `HandGarble.garble`.
#[allow(clippy::all)]
#[allow(non_snake_case)]
#[allow(unused_assignments)]
pub fn garble(
    buffer0: &mut [u8],
    buffer1: &mut [u8],
    buffer2: &mut [u8],
    buffer3: &mut [u8],
    buffer4: &mut [u8],
) {

    buffer2[12] = b(
        0x14 + ((((u(buffer1[64]) & 92) | ((u(buffer1[99]) / 3) & 35))
            & u(buffer4[(rol8x(u(buffer4[(u(buffer1[206]) % 21) as usize]), 4) % 21) as usize]))),
    );

    buffer1[4] = b((u(buffer1[99]) / 5) * (u(buffer1[99]) / 5) * 2);

    buffer2[34] = 0xb8;

    {
        let idx = (u(buffer1[203]) % 35) as usize;
        buffer1[153] = b(s(buffer1[153]) ^ (s(buffer2[idx]) * s(buffer2[idx]) * u(buffer1[190])));
    }

    buffer0[3] = b(
        s(buffer0[3]) - (((s(buffer4[(u(buffer1[205]) % 21) as usize]) >> 1) & 80) | 0xe_6440),
    );

    buffer0[16] = 0x93;
    buffer0[13] = 0x62;

    buffer1[33] = b(s(buffer1[33]) - (s(buffer4[(u(buffer1[36]) % 21) as usize]) & 0xf6));

    let tmp2 = buffer2[(u(buffer1[67]) % 35) as usize];
    buffer2[12] = 0x07;

    let tmp = buffer0[(u(buffer1[181]) % 20) as usize];
    buffer1[2] = b(s(buffer1[2]) - 3136);

    buffer0[19] = buffer4[(u(buffer1[58]) % 21) as usize];

    buffer3[0] = b(92 - s(buffer2[(u(buffer1[32]) % 35) as usize]));

    buffer3[4] = b(u(buffer2[(u(buffer1[15]) % 35) as usize]) + 0x9e);

    buffer1[34] = b(
        s(buffer1[34])
            + (u(buffer4[(((u(buffer2[(u(buffer1[15]) % 35) as usize]) + 0x9e) & 0xff) % 21) as usize])
                / 5),
    );

    buffer0[19] = b(
        (u(buffer0[19]) + (0xffff_fee6u32 as i32))
            - ((u(buffer0[(u(buffer3[4]) % 20) as usize]) >> 1) & 102),
    );

    {
        let x = u(buffer4[(u(buffer1[190]) % 21) as usize]);
        let rot = ((u(buffer1[72]) >> (x & 7)) ^ (u(buffer1[72]) << ((7 - (x - 1)) & 7)))
            - (3 * s(buffer4[(u(buffer1[126]) % 21) as usize]));
        buffer1[15] = b((3 * rot) ^ s(buffer1[15]));
    }

    {
        let v = u(buffer2[(u(buffer1[181]) % 35) as usize]);
        buffer0[15] = b(s(buffer0[15]) ^ (v * v * v));
    }

    buffer2[4] = b(s(buffer2[4]) ^ (u(buffer1[202]) / 3));

    {
        let A = 92 - u(buffer0[(u(buffer3[0]) % 20) as usize]);
        let E = (A & 0xc6) | ((!u(buffer1[105])) & 0xc6) | (A & (!u(buffer1[105])));
        buffer2[1] = b(s(buffer2[1]) + (E * E * E));
    }

    buffer0[19] = b(
        s(buffer0[19])
            ^ (((224 | (u(buffer4[(u(buffer1[92]) % 21) as usize]) & 27))
                * u(buffer2[(u(buffer1[41]) % 35) as usize]))
                / 3),
    );

    buffer1[140] = b(s(buffer1[140]) + s(b(weird_ror8(92, u(buffer1[5]) & 7))));

    {
        let t = (!u(buffer1[4])) ^ u(buffer2[(u(buffer1[12]) % 35) as usize]);
        buffer2[12] = b(s(buffer2[12]) + (((t | u(buffer1[182])) & 192) | (t & u(buffer1[182]))));
    }

    buffer1[36] = b(s(buffer1[36]) + 125);

    {
        let p = (74 & u(buffer1[138])) | ((74 | u(buffer1[138])) & u(buffer0[15]));
        let q = u(buffer0[(u(buffer1[43]) % 20) as usize]);
        let val = (p & q) | ((p | q) & 95);
        buffer1[124] = b(rol8x(val, 4));
    }

    buffer3[8] = b(
        ((((u(buffer0[(u(buffer3[4]) % 20) as usize]) & 95)
            & ((u(buffer4[(u(buffer1[68]) % 21) as usize]) & 46) << 1))
            | 16)
            ^ 92),
    );

    {
        let A = u(buffer1[177]) + u(buffer4[(u(buffer1[79]) % 21) as usize]);
        let t = (3 * u(buffer1[148])) / 5;
        let D = (((A >> 1) | t) & u(buffer2[1])) | ((A >> 1) & t);
        buffer3[12] = b(-34 - D);
    }

    {
        let A = 8 - (u(buffer2[22]) & 7);
        let B = u(buffer1[33]) >> (A & 7);
        let C = u(buffer1[33]) << (u(buffer2[22]) & 7);
        buffer2[16] = b(
            s(buffer2[16])
                + (((u(buffer2[(u(buffer3[0]) % 35) as usize]) & 159)
                    | u(buffer0[(u(buffer3[4]) % 20) as usize])
                    | 8)
                    - ((B ^ C) | 128)),
        );
    }

    buffer0[14] = b(s(buffer0[14]) ^ u(buffer2[(u(buffer3[12]) % 35) as usize]));

    {
        let A = weird_rol8(
            u(buffer4[(u(buffer0[(u(buffer1[201]) % 20) as usize]) % 21) as usize]),
            (u(buffer2[(u(buffer1[112]) % 35) as usize]) << 1) & 7,
        );
        let D = (u(buffer0[(u(buffer1[208]) % 20) as usize]) & 131)
            | (u(buffer0[(u(buffer1[164]) % 20) as usize]) & 124);
        buffer1[19] = b(s(buffer1[19]) + ((A & (D / 5)) | ((A | (D / 5)) & 37)));
    }

    {
        let t = u(buffer4[(u(buffer1[45]) % 21) as usize]) + 92;
        buffer2[8] = b(weird_ror8(140, (t * t) & 7) & 0xff);
    }

    buffer1[190] = 56;

    buffer2[8] = b(s(buffer2[8]) ^ u(buffer3[0]));

    buffer1[53] = b(!((u(buffer0[(u(buffer1[83]) % 20) as usize]) | 204) / 5));

    buffer0[13] = b(s(buffer0[13]) + u(buffer0[(u(buffer1[41]) % 20) as usize]));

    {
        let b2 = u(buffer2[(u(buffer3[0]) % 35) as usize]);
        buffer0[10] = b(((b2 & u(buffer1[2])) | ((b2 | u(buffer1[2])) & u(buffer3[12]))) / 15);
    }

    {
        let bf = u(buffer4[(u(buffer1[2]) % 21) as usize]);
        let b2v = u(buffer2[(u(buffer3[8]) % 35) as usize]);
        let A = (((56 | (bf & 68)) | b2v) & 42) | (((bf & 68) | 56) & b2v);
        buffer3[16] = b((A * A) + 110);
    }

    buffer3[20] = b(202 - u(buffer3[16]));

    buffer3[24] = buffer1[151];

    buffer2[13] = b(s(buffer2[13]) ^ s(buffer4[(u(buffer3[0]) % 21) as usize]));

    {
        let t = u(buffer2[(u(buffer1[179]) % 35) as usize]) - 38;
        let B = (t & 177) | (u(buffer3[12]) & 177);
        let C = t & u(buffer3[12]);
        buffer3[28] = b(30 + ((B | C) * (B | C)));
    }

    buffer3[32] = b(u(buffer3[28]) + 62);

    let tmp3;
    {
        let A = ((u(buffer3[20]) + (u(buffer3[0]) & 74)) | !u(buffer4[(u(buffer3[0]) % 21) as usize]))
            & 121;
        let B = (u(buffer3[20]) + (u(buffer3[0]) & 74)) & !u(buffer4[(u(buffer3[0]) % 21) as usize]);
        tmp3 = A | B;
        let C = ((((A | B) ^ (0xffff_ffa6u32 as i32)) | u(buffer3[0])) & 4)
            | (((A | B) ^ (0xffff_ffa6u32 as i32)) & u(buffer3[0]));
        buffer1[47] = b((u(buffer2[(u(buffer1[89]) % 35) as usize]) + C) ^ u(buffer1[47]));
    }

    buffer3[36] = b(
        ((u(rol8(b((s(tmp) & 179) + 68), 2)) & u(buffer0[3])) | (s(tmp2) & !u(buffer0[3]))) - 15,
    );

    buffer1[123] = b(s(buffer1[123]) ^ 221);

    {
        let A2 = (u(buffer4[(u(buffer3[0]) % 21) as usize]) / 3)
            - u(buffer2[(u(buffer3[4]) % 35) as usize]);
        let C2 = (((s(buffer3[0]) & 163) + 92) & 246) | (s(buffer3[0]) & 92);
        let E = ((C2 | s(buffer3[24])) & 54) | (C2 & s(buffer3[24]));
        buffer3[40] = b(A2 - E);
    }

    buffer3[44] = b(tmp3 ^ 81 ^ ((((u(buffer3[0]) >> 1) & 101) + 26)));

    buffer3[48] = b(u(buffer2[(u(buffer3[4]) % 35) as usize]) & 27);

    buffer3[52] = 27;
    buffer3[56] = 199;

    {
        let b40 = u(buffer3[40]);
        let b24 = u(buffer3[24]);
        let b4_20 = u(buffer4[(u(buffer3[0]) % 20) as usize]);
        let b4_21 = u(buffer4[(u(buffer3[0]) % 21) as usize]);
        let part1 = ((b40 | b24) & 177) | (b40 & b24);
        let part2 = ((b4_20 & 177) | 176) | (b4_21 & !3);
        let part3 = ((((b40 & b24) | ((b40 | b24) & 177)) & 199)
            | (((((b4_21 & 1) & 0xff) + 176) | (b4_21 & !3)) & u(buffer3[56])));
        let inner = ((part1 & part2) | part3) & (!u(buffer3[52]));
        buffer3[64] = b(u(buffer3[4]) + (inner | u(buffer3[48])));
    }

    buffer2[33] = b(s(buffer2[33]) ^ s(buffer1[26]));

    buffer1[106] = b(s(buffer1[106]) ^ s(buffer3[20]) ^ 133);

    buffer2[30] = b(
        ((u(buffer3[64]) / 3) - (275 | (u(buffer3[0]) & 247)))
            ^ u(buffer0[(u(buffer1[122]) % 20) as usize]),
    );

    buffer1[22] = b((u(buffer2[(u(buffer1[90]) % 35) as usize]) & 95) | 68);

    {
        let A = (u(buffer4[(u(buffer3[36]) % 21) as usize]) & 184)
            | (u(buffer2[(u(buffer3[44]) % 35) as usize]) & !184);
        buffer2[18] = b(s(buffer2[18]) + ((A * A * A) >> 1));
    }

    buffer2[5] = b(s(buffer2[5]) - u(buffer4[(u(buffer1[92]) % 21) as usize]));

    {
        let A = ((((u(buffer1[41]) & !24) | (u(buffer2[(u(buffer1[183]) % 35) as usize]) & 24))
            & (u(buffer3[16]) + 53))
            | (s(buffer3[20]) & u(buffer2[(u(buffer3[20]) % 35) as usize])));
        let B = (u(buffer1[17]) & !u(buffer3[44]))
            | (u(buffer0[(u(buffer1[59]) % 20) as usize]) & u(buffer3[44]));
        buffer2[18] = b(s(buffer2[18]) ^ (A * B));
    }

    {
        let A = weird_ror8(u(buffer1[11]), u(buffer2[(u(buffer1[28]) % 35) as usize]) & 7) & 7;
        let B = ((((u(buffer0[(u(buffer1[93]) % 20) as usize]) & !u(buffer0[14]))
            | (u(buffer0[14]) & 150))
            & !28)
            | (u(buffer1[7]) & 28));
        let wr = weird_rol8(u(buffer2[(u(buffer3[0]) % 35) as usize]), A);
        buffer2[22] = b(((((B | wr) & u(buffer2[33])) | (B & wr)) + 74) & 0xff);
    }

    {
        let A = u(buffer4[((u(buffer0[(u(buffer1[39]) % 20) as usize]) ^ 217) % 21) as usize]);
        let t = ((u(buffer3[20]) | u(buffer3[0])) & 214) | (u(buffer3[20]) & u(buffer3[0]));
        buffer0[15] = b(s(buffer0[15]) - ((t & A) | ((t | A) & u(buffer3[32]))));
    }

    let T;
    {
        let b2v = s(buffer2[(u(buffer1[57]) % 35) as usize]);
        let b0v = s(buffer0[(u(buffer3[64]) % 20) as usize]);
        let B = (((b2v & b0v) | ((b0v | b2v) & 95) | (s(buffer3[64]) & 45) | 82) & 32);
        let C = ((b2v & b0v) | ((b2v | b0v) & 95)) & ((s(buffer3[64]) & 45) | 82);
        let D = ((((u(buffer3[0]) / 3) - (u(buffer3[64]) | u(buffer1[22]))))
            ^ (u(buffer3[28]) + 62)
            ^ (B | C));
        T = u(buffer0[(D & 0xff) as usize % 20]);
    }

    {
        let v = u(buffer0[(u(buffer1[99]) % 20) as usize]);
        buffer3[68] = b((v * v * v * v) | u(buffer2[(u(buffer3[64]) % 35) as usize]));
    }

    let U = u(buffer0[(u(buffer1[50]) % 20) as usize]);
    let W = u(buffer2[(u(buffer1[138]) % 35) as usize]);
    let X = u(buffer4[(u(buffer1[39]) % 21) as usize]);
    let Y = u(buffer0[(u(buffer1[4]) % 20) as usize]);
    let Z = u(buffer4[(u(buffer1[202]) % 21) as usize]);
    let V = u(buffer0[(u(buffer1[151]) % 20) as usize]);
    let S_val = u(buffer2[(u(buffer1[14]) % 35) as usize]);
    let R = u(buffer0[(u(buffer1[145]) % 20) as usize]);

    {
        let A = (u(buffer2[(u(buffer3[68]) % 35) as usize]) & u(buffer0[(u(buffer1[209]) % 20) as usize]))
            | ((u(buffer2[(u(buffer3[68]) % 35) as usize]) | u(buffer0[(u(buffer1[209]) % 20) as usize]))
                & 24);
        let B = weird_rol8(
            u(buffer4[(u(buffer1[127]) % 21) as usize]),
            u(buffer2[(u(buffer3[68]) % 35) as usize]) & 7,
        );
        let C = (A & u(buffer0[10])) | (B & !u(buffer0[10]));
        let D = 7 ^ (u(buffer4[(u(buffer2[(u(buffer3[36]) % 35) as usize]) % 21) as usize]) << 1);
        buffer3[72] = b((C & 71) | (D & !71));
    }

    buffer2[2] = b(
        s(buffer2[2])
            + ((((u(buffer0[(u(buffer3[20]) % 20) as usize]) << 1) & 159)
                | (u(buffer4[(u(buffer1[190]) % 21) as usize]) & !159))
                & ((((u(buffer4[(u(buffer3[64]) % 21) as usize]) & 110)
                    | (u(buffer0[(u(buffer1[25]) % 20) as usize]) & !110))
                    & !150)
                    | (u(buffer1[25]) & 150))),
    );

    buffer2[14] = b(
        s(buffer2[14])
            - (((u(buffer2[(u(buffer3[20]) % 35) as usize])
                & (u(buffer3[72]) ^ u(buffer2[(u(buffer1[100]) % 35) as usize])))
                & !34)
                | (u(buffer1[97]) & 34)),
    );

    buffer0[17] = 115;

    {
        let t1 = ((u(buffer4[(u(buffer1[17]) % 21) as usize]) | u(buffer0[(u(buffer3[20]) % 20) as usize]))
            & u(buffer3[72]))
            | (u(buffer4[(u(buffer1[17]) % 21) as usize]) & u(buffer0[(u(buffer3[20]) % 20) as usize]));
        let t2 = ((u(buffer4[(u(buffer1[17]) % 21) as usize]) | u(buffer0[(u(buffer3[20]) % 20) as usize]))
            & u(buffer3[72]))
            | (u(buffer4[(u(buffer1[17]) % 21) as usize]) & s(buffer0[(u(buffer3[20]) % 20) as usize]))
            | (u(buffer1[50]) / 3);
        let val = ((t1 & (u(buffer1[50]) / 3)) | (t2 & 246)) << 1;
        buffer1[23] = b(s(buffer1[23]) ^ val);
    }

    {
        let p = ((u(buffer0[(u(buffer3[40]) % 20) as usize]) | u(buffer1[10])) & 82)
            | (u(buffer0[(u(buffer3[40]) % 20) as usize]) & u(buffer1[10]));
        buffer0[13] = b(((p & 209) | ((u(buffer0[(u(buffer1[39]) % 20) as usize]) << 1) & 46)) >> 1);
    }

    buffer2[33] = b(s(buffer2[33]) - (s(buffer1[113]) & 9));

    buffer2[28] = b(
        s(buffer2[28]) - ((((2 | (s(buffer1[110]) & 222)) >> 1) & !223) | (s(buffer3[20]) & 223)),
    );

    let J = weird_rol8(V | Z, U & 7);
    let A = (u(buffer2[16]) & T) | (W & !u(buffer2[16]));
    let B = (u(buffer1[33]) & 17) | (X & !17);
    let E = ((Y | ((A + B) / 5)) & 147) | (Y & ((A + B) / 5));
    let M = (u(buffer3[40]) & u(buffer4[(((u(buffer3[8]) + J + E) & 0xff) % 21) as usize]))
        | ((u(buffer3[40]) | u(buffer4[(((u(buffer3[8]) + J + E) & 0xff) % 21) as usize]))
            & u(buffer2[23]));

    {
        let t = u(buffer4[(u(buffer3[20]) % 21) as usize]) - 48;
        buffer0[15] = b(((t & !u(buffer1[184])) | (t & 189) | (189 & !u(buffer1[184]))) & (M * M * M));
    }

    buffer2[22] = b(s(buffer2[22]) + s(buffer1[183]));

    buffer3[76] = b((3 * s(buffer4[(u(buffer1[1]) % 21) as usize])) ^ s(buffer3[0]));

    {
        let A = u(buffer2[(((u(buffer3[8]) + (J + E)) & 0xff) % 35) as usize]);
        let bf178 = u(buffer4[(u(buffer1[178]) % 21) as usize]);
        let F = (((bf178 & A) | ((bf178 | A) & 209)) * u(buffer0[(u(buffer1[13]) % 20) as usize]))
            * (u(buffer4[(u(buffer1[26]) % 21) as usize]) >> 1);
        let G = (F + (0x733f_fff9u32 as i32)) * 198
            - (((F + (0x733f_fff9u32 as i32)) * 396 + 212) & 212)
            + 85;
        buffer3[80] = b(u(buffer3[36]) + (G ^ 148) + ((G ^ 107) << 1) - 127);
    }

    buffer3[84] = b(
        (u(buffer2[(u(buffer3[64]) % 35) as usize]) & 245)
            | (u(buffer2[(u(buffer3[20]) % 35) as usize]) & 10),
    );

    {
        let A = u(buffer0[(u(buffer3[68]) % 20) as usize]) | 81;
        buffer2[18] = b(
            s(buffer2[18]) - (((A * A * A) & !s(buffer0[15])) | ((u(buffer3[80]) / 15) & u(buffer0[15]))),
        );
    }

    buffer3[88] = b(
        u(buffer3[8]) + J + E - u(buffer0[(u(buffer1[160]) % 20) as usize])
            + (u(buffer4[(u(buffer0[(((u(buffer3[8]) + J + E) & 255) % 20) as usize]) % 21) as usize])
                / 3),
    );

    {
        let B = ((R ^ u(buffer3[72])) & !198) | ((S_val * S_val) & 198);
        let F = (u(buffer4[(u(buffer1[69]) % 21) as usize]) & u(buffer1[172]))
            | ((u(buffer4[(u(buffer1[69]) % 21) as usize]) | u(buffer1[172]))
                & ((u(buffer3[12]) - B) + 77));
        buffer0[16] = b(
            147 - ((u(buffer3[72]) & ((F & 251) | 1)) | (((F & 250) | u(buffer3[72])) & 198)),
        );
    }

    {
        let C = ((u(buffer4[(u(buffer1[168]) % 21) as usize])
            & s(buffer0[(u(buffer1[29]) % 20) as usize])
            & 7)
            | ((s(buffer4[(u(buffer1[168]) % 21) as usize]) | s(buffer0[(u(buffer1[29]) % 20) as usize]))
                & 6));
        let F = (u(buffer4[(u(buffer1[155]) % 21) as usize]) & u(buffer1[105]))
            | ((u(buffer4[(u(buffer1[155]) % 21) as usize]) | u(buffer1[105])) & 141);
        let idx = weird_rol32(F, C) % 21;
        let idx = if idx < 0 { idx + 21 } else { idx } as usize;
        buffer0[3] = b(s(buffer0[3]) - s(buffer4[idx]));
    }

    {
        let left = weird_ror8(u(buffer0[12]), (u(buffer0[(u(buffer1[61]) % 20) as usize]) / 5) & 7);
        let right = ((!s(buffer2[(u(buffer3[84]) % 35) as usize]) as i64) & 0xffff_ffff) / 5;
        buffer1[5] = b(left ^ (right as i32));
    }

    buffer1[198] = b(s(buffer1[198]) + s(buffer1[3]));

    {
        let A = 162 | u(buffer2[(u(buffer3[64]) % 35) as usize]);
        buffer1[164] = b(s(buffer1[164]) + ((A * A) / 5));
    }

    {
        let G = weird_ror8(139, u(buffer3[80]) & 7);
        let bf = u(buffer4[(u(buffer3[64]) % 21) as usize]);
        let C = ((bf * bf * bf) & 95) | (u(buffer0[(u(buffer3[40]) % 20) as usize]) & !95);
        buffer3[92] = b(
            (G & 12)
                | (u(buffer0[(u(buffer3[20]) % 20) as usize]) & 12)
                | (G & u(buffer0[(u(buffer3[20]) % 20) as usize]))
                | C,
        );
    }

    buffer2[12] = b(
        s(buffer2[12])
            + (((u(buffer1[103]) & 32) | (u(buffer3[92]) & (u(buffer1[103]) | 60)) | 16) / 3),
    );

    buffer3[96] = buffer1[143];
    buffer3[100] = 27;

    buffer3[104] = b(
        (((u(buffer3[40]) & !u(buffer2[8])) | (u(buffer1[35]) & u(buffer2[8]))) & u(buffer3[64]))
            ^ 119,
    );

    buffer3[108] = b(
        238 & ((((u(buffer3[40]) & !u(buffer2[8])) | (u(buffer1[35]) & u(buffer2[8])))
            & u(buffer3[64]))
            << 1),
    );

    buffer3[112] = b((!u(buffer3[64]) & (u(buffer3[84]) / 3)) ^ 49);

    buffer3[116] = b(98 & ((!u(buffer3[64]) & (u(buffer3[84]) / 3)) << 1));

    {
        let A = (u(buffer1[35]) & u(buffer2[8])) | (u(buffer3[40]) & !u(buffer2[8]));
        let B = (A & s(buffer3[64])) | ((u(buffer3[84]) / 3) & !u(buffer3[64]));
        let inner = (B & (86 + ((u(buffer1[172]) & 64) >> 1)))
            | (((((u(buffer1[172]) & 65) >> 1) ^ 86)
                | ((!u(buffer3[64]) & (u(buffer3[84]) / 3))
                    | (((u(buffer3[40]) & !u(buffer2[8])) | (u(buffer1[35]) & u(buffer2[8])))
                        & u(buffer3[64]))))
                & u(buffer3[100]));
        buffer1[143] = b(u(buffer3[96]) - inner);
    }

    buffer2[29] = 162;

    {
        let A = (((u(buffer4[(u(buffer3[88]) % 21) as usize]) & 160)
            | (u(buffer0[(u(buffer1[125]) % 20) as usize]) & 95))
            >> 1);
        let B = u(buffer2[(u(buffer1[149]) % 35) as usize]) ^ (u(buffer1[43]) * u(buffer1[43]));
        buffer0[15] = b(s(buffer0[15]) + ((B & A) | ((A | B) & 115)));
    }

    buffer3[120] = b(u(buffer3[64]) - u(buffer0[(u(buffer3[40]) % 20) as usize]));

    buffer1[95] = buffer4[(u(buffer3[20]) % 21) as usize];

    {
        let t = u(buffer2[(u(buffer1[17]) % 35) as usize]);
        let A = weird_ror8(u(buffer2[(u(buffer3[80]) % 35) as usize]), (t * t * t) & 7);
        buffer0[7] = b(s(buffer0[7]) - (A * A));
    }

    {
        let bf = u(buffer4[(u(buffer1[202]) % 21) as usize]);
        buffer2[8] = b(u(buffer2[8]) - u(buffer1[184]) + (bf * bf * bf));
    }

    buffer0[16] = b((u(buffer2[(u(buffer1[102]) % 35) as usize]) << 1) & 132);

    buffer3[124] = b((u(buffer4[(u(buffer3[40]) % 21) as usize]) >> 1) ^ u(buffer3[68]));

    buffer0[7] = b(
        s(buffer0[7])
            - (u(buffer0[(u(buffer1[191]) % 20) as usize])
                - (((u(buffer4[(u(buffer1[80]) % 21) as usize]) << 1) & !177)
                    | (u(buffer4[(u(buffer4[(u(buffer3[88]) % 21) as usize]) % 21) as usize])
                        & 177))),
    );

    buffer0[6] = buffer0[(u(buffer1[119]) % 20) as usize];

    {
        let A = (s(buffer4[(u(buffer1[190]) % 21) as usize]) & !209) | (s(buffer1[118]) & 209);
        let B = s(buffer0[(u(buffer3[120]) % 20) as usize]) * s(buffer0[(u(buffer3[120]) % 20) as usize]);
        let left = s(buffer0[(u(buffer3[84]) % 20) as usize])
            ^ (s(buffer2[(u(buffer1[71]) % 35) as usize]) + s(buffer2[(u(buffer1[15]) % 35) as usize]));
        buffer0[12] = b(left & ((A & B) | ((A | B) & 27)));
    }

    {
        let B = (u(buffer1[32]) & u(buffer2[(u(buffer3[88]) % 35) as usize]))
            | ((u(buffer1[32]) | u(buffer2[(u(buffer3[88]) % 35) as usize])) & 23);
        let D = (((u(buffer4[(u(buffer1[57]) % 21) as usize]) * 231) & 169) | (B & 86));
        let F = ((((u(buffer0[(u(buffer1[82]) % 20) as usize]) & !29)
            | (u(buffer4[(u(buffer3[124]) % 21) as usize]) & 29))
            & 190)
            | (u(buffer4[((D / 5) % 21) as usize]) & !190));
        let h0 = u(buffer0[(u(buffer3[40]) % 20) as usize]);
        let H = h0 * h0 * h0;
        let K = (H & u(buffer1[82])) | (H & 92) | (u(buffer1[82]) & 92);
        buffer3[128] = b(((F & K) | ((F | K) & 192)) ^ (D / 5));
    }

    buffer2[25] = b(
        s(buffer2[25])
            ^ (((u(buffer0[(u(buffer3[120]) % 20) as usize]) << 1) * u(buffer1[5]))
                - (weird_rol8(u(buffer3[76]), u(buffer4[(u(buffer3[124]) % 21) as usize]) & 7)
                    & (u(buffer3[20]) + 110))),
    );

}

fn rol8(input: u8, count: i32) -> u8 {
    let input = u(input);
    b(((input << count) & 0xff) | (input >> (8 - count)))
}

fn rol8x(input: i32, count: i32) -> i32 {
    (input << count) | (input >> (8 - count))
}

fn weird_ror8(input: i32, count: i32) -> i32 {
    if count == 0 {
        return 0;
    }
    ((input >> count) & 0xff) | ((input & 0xff) << (8 - count))
}

fn weird_rol8(input: i32, count: i32) -> i32 {
    if count == 0 {
        return 0;
    }
    ((input << count) & 0xff) | ((input & 0xff) >> (8 - count))
}

fn weird_rol32(input: i32, count: i32) -> i32 {
    if count == 0 {
        return 0;
    }
    (input << count) ^ (input >> (8 - count))
}
