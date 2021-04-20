use crate::curve_arithmetic::CurveDecodingError;
use ff::{Field, PrimeField, SqrtField};
use group::{CurveProjective, EncodedPoint};
use pairing::bls12_381::{Fq, Fq2, Fr, FqRepr, G2Uncompressed, G2};
use sha2::{Digest, Sha256};
use std::{
    convert::TryInto,
    io::{Cursor, Write},
};

/// Implements https://tools.ietf.org/html/draft-irtf-cfrg-hash-to-curve-10#section-3
/// It follows the steps
///    1. u = hash_to_field(msg, 2)
///    2. Q0 = map_to_curve(u[0])
///    3. Q1 = map_to_curve(u[1])
///    4. R = Q0 + Q1              
///    5. P = clear_cofactor(R) = h_eff * R   # Clearing cofactor
///    6. return P,
/// where the choices of hash_to_field, map_to_curve and h_eff are as described in https://tools.ietf.org/html/draft-irtf-cfrg-hash-to-curve-10#section-8.8.1.
pub fn hash_to_curve_g2(msg: &[u8], dst: &[u8]) -> G2 {
    let (u0, u1) = hash_to_field_fq2(msg, dst);

    let q0 = map_to_curve_g2(u0); // This is on E, but not necessarily in G2
    let q1 = map_to_curve_g2(u1); // This is on E, but not necessarily in G2

    let mut r = q0;
    r.add_assign(&q1); // This is on E, but not necessarily in G2
    // Clearing cofactor with h_eff
    // h_eff: 0xbc69f08f2ee75b3584c6a0ea91b352888e2a8e9145ad7689986ff031508ffe1329c2f178731db956d82bf015d1212b02ec0ec69d7477c1ae954cbc06689f6a359894c0adebbf6b4e8020005aaa95551
    // h_eff: 209869847837335686905080341498658477663839067235703451875306851526599783796572738804459333109033834234622528588876978987822447936461846631641690358257586228683615991308971558879306463436166481
    let h_eff = Fr::from_str("209869847837335686905080341498658477663839067235703451875306851526599783796572738804459333109033834234622528588876978987822447936461846631641690358257586228683615991308971558879306463436166481").unwrap();
    r.mul_assign(h_eff);
    r // This now guaranteed to be in G2
}

fn map_to_curve_g2(u: Fq2) -> G2 {
    let (x, y) = sswu(u);
    let (x, y, z) = iso_map(x, y, Fq2::one());
    from_coordinates_unchecked(x, y, z)
}

fn sswu(u: Fq2) -> (Fq2, Fq2) {
    let a = Fq2 {
        c0: Fq::zero(),
        c1: Fq::from_repr(FqRepr::from(240)).unwrap()
    };
    let b = Fq2 {
        c0: Fq::from_repr(FqRepr::from(1012)).unwrap(),
        c1: Fq::from_repr(FqRepr::from(1012)).unwrap()
    };
    let mut z = Fq2 {
        c0: Fq::from_repr(FqRepr::from(2)).unwrap(),
        c1: Fq::from_repr(FqRepr::from(1)).unwrap()
    };
    z.negate();

    // Constants:
    // 1.  c1 = -B / A
    let mut c1 = a;
    c1 = c1.inverse().unwrap();
    c1.mul_assign(&b);
    c1.negate();
    // 2.  c2 = -1 / Z
    let mut c2 = z.inverse().unwrap();
    c2.negate();

    // todo everything above is a constant

    // Steps:
    // 1.  tv1 = Z * u^2
    let mut tv1 = u;
    tv1.square();
    tv1.mul_assign(&z);
    // 2.  tv2 = tv1^2
    let mut tv2 = tv1;
    tv2.square();
    // 3.   x1 = tv1 + tv2
    let mut x1 = tv1;
    x1.add_assign(&tv2);
    // 4.   x1 = inv0(x1)
    x1 = match x1.inverse() {
        None => Fq2::zero(),
        Some(x1inv) => x1inv
    };
    // 5.   e1 = x1 == 0
    let e1 = x1.is_zero();
    // 6.   x1 = x1 + 1
    x1.add_assign(&Fq2::one());
    // 7.   x1 = CMOV(x1, c2, e1)    # If (tv1 + tv2) == 0, set x1 = -1 / Z
    if e1 {
        x1 = c2;
    }
    // 8.   x1 = x1 * c1      # x1 = (-B / A) * (1 + (1 / (Z^2 * u^4 + Z * u^2)))
    x1.mul_assign(&c1);
    // 9.  gx1 = x1^2
    let mut gx1 = x1;
    gx1.square();
    // 10. gx1 = gx1 + A
    gx1.add_assign(&a);
    // 11. gx1 = gx1 * x1
    gx1.mul_assign(&x1);
    // 12. gx1 = gx1 + B             # gx1 = g(x1) = x1^3 + A * x1 + B
    gx1.add_assign(&b);
    // 13.  x2 = tv1 * x1            # x2 = Z * u^2 * x1
    let mut x2 = tv1;
    x2.mul_assign(&x1);
    // 14. tv2 = tv1 * tv2
    tv2.mul_assign(&tv1);
    // 15. gx2 = gx1 * tv2           # gx2 = (Z * u^2)^3 * gx1
    let mut gx2 = gx1;
    gx2.mul_assign(&tv2);
    // 16.  e2 = is_square(gx1)
    let e2 = match gx1.sqrt() {
        None => false,
        Some(_) => true
    };
    // 17.   x = CMOV(x2, x1, e2)    # If is_square(gx1), x = x1, else x = x2
    // 18.  y2 = CMOV(gx2, gx1, e2)  # If is_square(gx1), y2 = gx1, else y2 = gx2
    let mut x = x2;
    let mut y2 = gx2;
    if e2 {
        x = x1;
        y2 = gx1;
    }
    // 19.   y = sqrt(y2)
    let mut y = y2.sqrt().unwrap();
    // 20.  e3 = sgn0(u) == sgn0(y)  # Fix sign of y
    let e3 = sgn0(u) == sgn0(y);
    // 21.   y = CMOV(-y, y, e3)
    if !e3 {
        y.negate();
    }
    // 22. return (x, y)
    (x, y)
}

/// The function sgn0 given at https://tools.ietf.org/html/draft-irtf-cfrg-hash-to-curve-10#section-4.1
fn sgn0(x: Fq2) -> u64 {
    let sign_0 = x.c0.into_repr().0[0] % 2;
    let zero_0 = x.c0.is_zero();
    let sign_1 = x.c1.into_repr().0[0] % 2;
    sign_0 | (zero_0 as u64 & sign_1)
}

/// Implements https://tools.ietf.org/html/draft-irtf-cfrg-hash-to-curve-10#section-5.4.1
/// len_in_bytes is fixed to 256
/// Domain separation string (dst) should be at most 255 bytes
fn expand_message_xmd(msg: &[u8], dst: &[u8]) -> [[u8; 32]; 8] {
    // DST_prime = DST || I2OSP(len(DST), 1)
    let mut dst_prime = dst.to_vec();
    dst_prime.push(dst.len().try_into().unwrap()); // panics if dst is more than 255 bytes

    // b_0 = H(msg_prime), msg_prime = Z_pad || msg || l_i_b_str || I2OSP(0, 1) || DST_prime
    let mut h = Sha256::new();
    h.update(vec![0; 64]); // z_pad = I2OSP(0, 64), 64 is the input block size of Sha265
    h.update(msg);
    h.update(vec![1, 0]); // l_i_b_str = I2OSP(256, 2)
    h.update([0u8]);
    h.update(&dst_prime);
    let mut b_0: [u8; 32] = [0u8; 32];
    b_0.copy_from_slice(h.finalize().as_slice());

    // b_1 = H(b_0 || I2OSP(1, 1) || DST_prime)
    let mut h = Sha256::new();
    h.update(b_0);
    h.update([1u8]);
    h.update(&dst_prime);

    let mut b = [[0u8; 32]; 8]; //b[i] corresponds to b_i+1 in specification.
    b[0].copy_from_slice(h.finalize().as_slice());

    //compute remaining uniform bytes
    for i in 1u8..8 {
        // b_i = H(strxor(b_0, b_i-1)  || I2OSP(i, 1) || DST_prime)
        let mut h = Sha256::new();
        let xor: Vec<u8> = b_0.iter().zip(b[i as usize - 1].iter()).map(|(x, y)| x ^ y).collect();
        h.update(xor);
        h.update([i+1]); // offset as standard drops b_0 and returns index b_1-b_8
        h.update(&dst_prime);
        b[i as usize].copy_from_slice(h.finalize().as_slice());
    }

    b
}

/// Implements https://tools.ietf.org/html/draft-irtf-cfrg-hash-to-curve-10#section-3
/// with the choice of expand_message being expand_message_xmd, as specified in 
/// https://tools.ietf.org/html/draft-irtf-cfrg-hash-to-curve-10#section-8.8.2.
fn hash_to_field_fq2(msg: &[u8], dst: &[u8]) -> (Fq2, Fq2) {
    let b = expand_message_xmd(msg, dst); 
    let u0 = Fq2 {
        c0: fq_from_bytes(&b[0], &b[1]),
        c1: fq_from_bytes(&b[2], &b[3])
    };
    let u1 = Fq2 {
        c0: fq_from_bytes(&b[4], &b[5]),
        c1: fq_from_bytes(&b[6], &b[7])
    };
    (u0, u1)
}

// Interpret input as integers (big endian)
// Return (left*2^256 + right) as Fq
fn fq_from_bytes(left_bytes: &[u8; 32], right_bytes: &[u8; 32]) -> Fq {
    fn le_u64s_from_be_bytes(bytes: &[u8; 32]) -> Fq {
        let mut digits = [0u64; 6];

        for (place, chunk) in digits.iter_mut().zip(bytes.chunks(8).rev()) {
            *place = u64::from_be_bytes(chunk.try_into().expect("Chunk Sie is always 8"))
        }

        Fq::from_repr(FqRepr(digits)).expect("Only the leading 4 u64s are initialized")
    }

    let two_to_256_fqrepr = [0u64, 0, 0, 0, 1, 0]; // 2^256
    let two_to_256_fq = Fq::from_repr(FqRepr(two_to_256_fqrepr)).expect("2^256 fits in modulus");

    let mut left_fq = le_u64s_from_be_bytes(left_bytes);
    let right_fq = le_u64s_from_be_bytes(right_bytes);
    left_fq.mul_assign(&two_to_256_fq); // u_0[..32] * 2^256
    left_fq.add_assign(&right_fq); // u_0[32..] + u_0[32..] * 2^256 = u_0

    left_fq
}

fn iso_map(x: Fq2, y: Fq2, z: Fq2) -> (Fq2, Fq2, Fq2) {
    // Compute Z^2i for i = 1,...,15
    let mut z_pow_2i: [Fq2; 15] = [z; 15];
    z_pow_2i[0].square(); // Z^2
    z_pow_2i[1] = z_pow_2i[0];
    z_pow_2i[1].square(); // Z^4
    let mut z_ = z_pow_2i[1];
    z_.mul_assign(&z_pow_2i[0]);
    z_pow_2i[2] = z_; // Z^6
    z_pow_2i[3] = z_pow_2i[1];
    z_pow_2i[3].square(); // Z^8
    for i in 0..3 {
        // Z^10, Z^12, Z^14,
        z_ = z_pow_2i[3 + i];
        z_.mul_assign(&z_pow_2i[0]);
        z_pow_2i[4 + i] = z_;
    }
    z_pow_2i[7] = z_pow_2i[3];
    z_pow_2i[7].square(); // Z^16
    for i in 0..7 {
        // Z^18, Z^20, Z^22, Z^24, Z^26, Z^28, Z^30,
        z_ = z_pow_2i[7 + i];
        z_.mul_assign(&z_pow_2i[0]);
        z_pow_2i[8 + i] = z_;
    }

    let x_num = horner(&K1, &z_pow_2i, &x);

    let x_den_ = horner(&K2, &z_pow_2i, &x);
    let mut x_den = z_pow_2i[0];
    x_den.mul_assign(&x_den_);

    let y_num_ = horner(&K3, &z_pow_2i, &x);
    let mut y_num = y;
    y_num.mul_assign(&y_num_);

    let y_den_ = horner(&K4, &z_pow_2i, &x);
    let mut y_den = z_pow_2i[0];
    y_den.mul_assign(&z);
    y_den.mul_assign(&y_den_);

    let mut z_jac = x_den;
    z_jac.mul_assign(&y_den);
    let mut x_jac = x_num;
    x_jac.mul_assign(&y_den);
    x_jac.mul_assign(&z_jac);
    let mut z_jac_pow2 = z_jac;
    z_jac_pow2.square();
    let mut y_jac = y_num;
    y_jac.mul_assign(&x_den);
    y_jac.mul_assign(&z_jac_pow2);

    (x_jac, y_jac, z_jac)
}


const K1: [[[u64;6];2];4] = [
    [   
        //k_(1,0) = 
        // 0x5c759507e8e333ebb5b7a9a47d7ed8532c52d39fd3a042a88b5842
        // 3c50ae15d5c2638e343d9c71c6238aaaaaaaa97d6
        // + 
        // 0x
        // 5c759507e8e333e
        // bb5b7a9a47d7ed85
        // 32c52d39fd3a042a
        // 88b58423c50ae15d
        // 5c2638e343d9c71c
        // 6238aaaaaaaa97d6 * I
        [ 
            0x6238aaaaaaaa97d6,
            0x5c2638e343d9c71c,
            0x88b58423c50ae15d,
            0x32c52d39fd3a042a,
            0xbb5b7a9a47d7ed85,
            0x5c759507e8e333e
        ],
        [
            0x6238aaaaaaaa97d6,
            0x5c2638e343d9c71c,
            0x88b58423c50ae15d,
            0x32c52d39fd3a042a,
            0xbb5b7a9a47d7ed85,
            0x5c759507e8e333e
        ]
    ],
    [
        // k_(1,1) = 0x
        // 11560bf17baa99bc
        // 32126fced787c88f
        // 984f87adf7ae0c7f
        // 9a208c6b4f20a418
        // 1472aaa9cb8d5555
        // 26a9ffffffffc71a * I
        [0, 0, 0, 0, 0, 0],
        [
            0x26a9ffffffffc71a,
            0x1472aaa9cb8d5555,
            0x9a208c6b4f20a418,
            0x984f87adf7ae0c7f,
            0x32126fced787c88f,
            0x11560bf17baa99bc
        ]
    ],
    [
        // k_(1,2) = 
        // 0x
        // 11560bf17baa99bc
        // 32126fced787c88f
        // 984f87adf7ae0c7f
        // 9a208c6b4f20a418
        // 1472aaa9cb8d5555
        // 26a9ffffffffc71e + 
        // 0x
        // 8ab05f8bdd54cde
        // 190937e76bc3e447
        // cc27c3d6fbd7063f
        // cd104635a790520c
        // 0a395554e5c6aaaa
        // 9354ffffffffe38d * I
        [
            0x26a9ffffffffc71e,
            0x1472aaa9cb8d5555,
            0x9a208c6b4f20a418,
            0x984f87adf7ae0c7f,
            0x32126fced787c88f,
            0x11560bf17baa99bc
        ],
        [
            0x9354ffffffffe38d,
            0x0a395554e5c6aaaa,
            0xcd104635a790520c,
            0xcc27c3d6fbd7063f,
            0x190937e76bc3e447,
            0x8ab05f8bdd54cde
        ]
    ],
    [
        // k_(1,3) = 0x
        // 171d6541fa38ccfa
        // ed6dea691f5fb614
        // cb14b4e7f4e810aa
        // 22d6108f142b8575
        // 7098e38d0f671c71
        // 88e2aaaaaaaa5ed1
        [
            0x88e2aaaaaaaa5ed1,
            0x7098e38d0f671c71,
            0x22d6108f142b8575,
            0xcb14b4e7f4e810aa,
            0xed6dea691f5fb614,
            0x171d6541fa38ccfa
        ],
        [0, 0, 0, 0, 0, 0]
    ]
];

const K2: [[[u64;6];2];3] = [
    [   
        // k_(2,0) = 0x
        // 1a0111ea397fe69a
        // 4b1ba7b6434bacd7
        // 64774b84f38512bf
        // 6730d2a0f6b0f624
        // 1eabfffeb153ffff
        // b9feffffffffaa63 * I
        [0, 0, 0, 0, 0, 0],
        [
            0xb9feffffffffaa63,
            0x1eabfffeb153ffff,
            0x6730d2a0f6b0f624,
            0x64774b84f38512bf,
            0x4b1ba7b6434bacd7,
            0x1a0111ea397fe69a
        ]
    ],
    [
        // k_(2,1) = 0xc + 0x
        // 1a0111ea397fe69a
        // 4b1ba7b6434bacd7
        // 64774b84f38512bf
        // 6730d2a0f6b0f624
        // 1eabfffeb153ffff
        // b9feffffffffaa9f * I
        [0xc,0, 0, 0, 0, 0],
        [
            0xb9feffffffffaa9f,
            0x1eabfffeb153ffff,
            0x6730d2a0f6b0f624,
            0x64774b84f38512bf,
            0x4b1ba7b6434bacd7,
            0x1a0111ea397fe69a
        ]
    ],
    [
        // k_(2,2) = 1 // todo test
        [1, 0, 0, 0, 0, 0],
        [0, 0, 0, 0, 0, 0]
    ]
];

const K3: [[[u64;6];2];4] = [
    [
        // k_(3,0) = 0x1530477c7ab4113b59a4c18b076d11930f7da5d4a07f649bf54439
        // d87d27e500fc8c25ebf8c92f6812cfc71c71c6d706
        //  + 
        // 0x
        // 1530477c7ab4113b
        // 59a4c18b076d1193
        // 0f7da5d4a07f649b
        // f54439d87d27e500
        // fc8c25ebf8c92f68
        // 12cfc71c71c6d706 * I
        [ 
            0x12cfc71c71c6d706,
            0xfc8c25ebf8c92f68,
            0xf54439d87d27e500,
            0x0f7da5d4a07f649b,
            0x59a4c18b076d1193,
            0x1530477c7ab4113b
        ],
        [
            0x12cfc71c71c6d706,
            0xfc8c25ebf8c92f68,
            0xf54439d87d27e500,
            0x0f7da5d4a07f649b,
            0x59a4c18b076d1193,
            0x1530477c7ab4113b
        ]
    ],
    [
        // k_(3,1) = 
        // 0x
        // 5c759507e8e333e
        // bb5b7a9a47d7ed85
        // 32c52d39fd3a042a
        // 88b58423c50ae15d
        // 5c2638e343d9c71c
        // 6238aaaaaaaa97be * I
        [0, 0, 0, 0, 0, 0],
        [
            0x6238aaaaaaaa97be,
            0x5c2638e343d9c71c,
            0x88b58423c50ae15d,
            0x32c52d39fd3a042a,
            0xbb5b7a9a47d7ed85,
            0x5c759507e8e333e
        ]
    ],
    [
        // k_(3,2) = 
        // 0x
        // 11560bf17baa99bc
        // 32126fced787c88f
        // 984f87adf7ae0c7f
        // 9a208c6b4f20a418
        // 1472aaa9cb8d5555
        // 26a9ffffffffc71c
        //  + 
        // 0x
        // 8ab05f8bdd54cde
        // 190937e76bc3e447
        // cc27c3d6fbd7063f
        // cd104635a790520c
        // 0a395554e5c6aaaa
        // 9354ffffffffe38f * I
        [
            0x26a9ffffffffc71c,
            0x1472aaa9cb8d5555,
            0x9a208c6b4f20a418,
            0x984f87adf7ae0c7f,
            0x32126fced787c88f,
            0x11560bf17baa99bc
        ],
        [
            0x9354ffffffffe38f,
            0x0a395554e5c6aaaa,
            0xcd104635a790520c,
            0xcc27c3d6fbd7063f,
            0x190937e76bc3e447,
            0x8ab05f8bdd54cde
        ]
    ],
    [
        // k_(3,3) = 
        // 0x
        // 124c9ad43b6cf79b
        // fbf7043de3811ad0
        // 761b0f37a1e26286
        // b0e977c69aa27452
        // 4e79097a56dc4bd9
        // e1b371c71c718b10
        [
            0xe1b371c71c718b10,
            0x4e79097a56dc4bd9,
            0xb0e977c69aa27452,
            0x761b0f37a1e26286,
            0xfbf7043de3811ad0,
            0x124c9ad43b6cf79b
        ],
        [0, 0, 0, 0, 0, 0]
    ]
];

const K4: [[[u64;6];2];4] = [
    [
        // k_(4,0) = 0x
        // 1a0111ea397fe69a
        // 4b1ba7b6434bacd7
        // 64774b84f38512bf
        // 6730d2a0f6b0f624
        // 1eabfffeb153ffff
        // b9feffffffffa8fb
        //  + 0x
        // 1a0111ea397fe69a
        // 4b1ba7b6434bacd7
        // 64774b84f38512bf
        // 6730d2a0f6b0f624
        // 1eabfffeb153ffff
        // b9feffffffffa8fb * I
        [ 
            0xb9feffffffffa8fb,
            0x1eabfffeb153ffff,
            0x6730d2a0f6b0f624,
            0x64774b84f38512bf,
            0x4b1ba7b6434bacd7,
            0x1a0111ea397fe69a
        ],
        [
            0xb9feffffffffa8fb,
            0x1eabfffeb153ffff,
            0x6730d2a0f6b0f624,
            0x64774b84f38512bf,
            0x4b1ba7b6434bacd7,
            0x1a0111ea397fe69a
        ]
    ],
    [
        // k_(4,1) = 
        // 0x
        // 1a0111ea397fe69a
        // 4b1ba7b6434bacd7
        // 64774b84f38512bf
        // 6730d2a0f6b0f624
        // 1eabfffeb153ffff
        // b9feffffffffa9d3 * I
        [0, 0, 0, 0, 0, 0],
        [
            0xb9feffffffffa9d3,
            0x1eabfffeb153ffff,
            0x6730d2a0f6b0f624,
            0x64774b84f38512bf,
            0x4b1ba7b6434bacd7,
            0x1a0111ea397fe69a
        ]
    ],
    [
        // k_(4,2) = 0x12 + 
        // 0x
        // 1a0111ea397fe69a
        // 4b1ba7b6434bacd7
        // 64774b84f38512bf
        // 6730d2a0f6b0f624
        // 1eabfffeb153ffff
        // b9feffffffffaa99 * I
        [
            0x12,
            0x0,
            0x0,
            0x0,
            0x0,
            0x0
        ],
        [
            0xb9feffffffffaa99,
            0x1eabfffeb153ffff,
            0x6730d2a0f6b0f624,
            0x64774b84f38512bf,
            0x4b1ba7b6434bacd7,
            0x1a0111ea397fe69a
        ]
    ],
    [
        // k_(4,3) = 1 todo test
        [1, 0, 0, 0, 0, 0],
        [0, 0, 0, 0, 0, 0]
    ]
];

fn horner(coefficients: &[[[u64; 6];2]], z_powers: &[Fq2], variable: &Fq2) -> Fq2 {
    fn fq2_from_u64s(u64s: [[u64; 6];2]) -> Fq2 {
        // unwrapping the Ki constants never fails:
        Fq2{
            c0: Fq::from_repr(FqRepr(u64s[0])).unwrap(),
            c1: Fq::from_repr(FqRepr(u64s[1])).unwrap()
        }
    }

    let clen = coefficients.len();
    let mut res = fq2_from_u64s(coefficients[clen - 1]);
    // skip the last coefficient since we already used it
    for (coeff, pow) in coefficients.iter().rev().skip(1).zip(z_powers.iter()) {
        res.mul_assign(variable);
        let mut coeff = fq2_from_u64s(*coeff);
        coeff.mul_assign(pow);
        res.add_assign(&coeff);
    }
    res
}

// Returns a point on E1 with coordinates x,y,z.
// CAREFUL! This point is NOT guaranteed to be in the correct order subgroup
// To get the point into the correct order subgroup, multiply by  todo fix description
#[inline]
fn from_coordinates_unchecked(x: Fq2, y: Fq2, z: Fq2) -> G2 {
    if z.is_zero() {
        G2::zero()
    } else {
        let z_inv = z.inverse().unwrap();
        let mut z_inv2 = z_inv;
        z_inv2.square();
        let mut p_x = x;
        p_x.mul_assign(&z_inv2);
        let mut p_y = y;
        p_y.mul_assign(&z_inv);
        p_y.mul_assign(&z_inv2);

        let mut uncompress_point = G2Uncompressed::empty();
        let mut cursor = Cursor::new(uncompress_point.as_mut());

        for digit in p_x.c1.into_repr().as_ref().iter().rev() {
            cursor
                .write_all(&digit.to_be_bytes())
                .expect("This write will always succeed.");
        }
        for digit in p_x.c0.into_repr().as_ref().iter().rev() {
            cursor
                .write_all(&digit.to_be_bytes())
                .expect("This write will always succeed.");
        }
        for digit in p_y.c1.into_repr().as_ref().iter().rev() {
            cursor
            .write_all(&digit.to_be_bytes())
            .expect("This write will always succeed.");
        }
        for digit in p_y.c0.into_repr().as_ref().iter().rev() {
            cursor
                .write_all(&digit.to_be_bytes())
                .expect("This write will always succeed.");
        }

        // The below is safe, since xiso, yiso, z are in Fq.
        // The into_affine_unchecked() used below can fail if
        // at least one of the bits representing 2^5, 2^6 or 2^7 in the first entry of
        // the `uncompress_point` are set, but this will not happen.
        // The field size q is
        // 4002409555221667393417789825735904156556882819939007885332058136124031650490837864442687629129015664037894272559787,
        // and and since 27 * 2^(47*8) > q, the first entry of
        // `uncompress_point` will always be < 27 < 2^5, since this entry
        // represents the number of 2^(47*8)'s.
        let res = uncompress_point.into_affine_unchecked();
        G2::from(res.expect("Should not happen, since input coordinates are in Fq."))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_on_curve_iso(x: Fq2, y: Fq2) {
        let a = Fq2 {
            c0: Fq::zero(),
            c1: Fq::from_repr(FqRepr::from(240)).unwrap()
        };
        let b = Fq2 {
            c0: Fq::from_repr(FqRepr::from(1012)).unwrap(),
            c1: Fq::from_repr(FqRepr::from(1012)).unwrap()
        };
        let mut y2 = y;
        y2.square();

        let mut x3axb = x;
        x3axb.square();
        x3axb.add_assign(&a);
        x3axb.mul_assign(&x);
        x3axb.add_assign(&b);

        assert_eq!(y2, x3axb);
    }

    #[test]
    fn test_hash_to_field_fq2() {
        let dst = b"QUUX-V01-CS02-with-BLS12381G2_XMD:SHA-256_SSWU_RO_";
        
        {
            //    msg     =
            //    u[0]    = 03dbc2cce174e91ba93cbb08f26b917f98194a2ea08d1cce75b2b9
            //              cc9f21689d80bd79b594a613d0a68eb807dfdc1cf8
            //        + I * 05a2acec64114845711a54199ea339abd125ba38253b70a92c876d
            //              f10598bd1986b739cad67961eb94f7076511b3b39a
            //    u[1]    = 02f99798e8a5acdeed60d7e18e9120521ba1f47ec090984662846b
            //              c825de191b5b7641148c0dbc237726a334473eee94
            //        + I * 145a81e418d4010cc027a68f14391b30074e89e60ee7a22f87217b
            //              2f6eb0c4b94c9115b436e6fa4607e95a98de30a435
            let msg = b"";
            let (u0, u1) = hash_to_field_fq2(msg, dst);
            assert_eq!(
                u0,
                Fq2{
                c0: Fq::from_str("593868448310005448561172252387029516360409945786457439875974315031640021389835649561235021338510064922970633805048").unwrap(),
                c1: Fq::from_str("867375309489067512797459860887365951877054038763818448057326190302701649888849997836339069389536967202878289851290").unwrap()}
            );
            assert_eq!(
                u1,
                Fq2{
                c0: Fq::from_str("457889704519948843474026022562641969443315715595459159112874498082953431971323809145630315884223143822925947137684").unwrap(),
                c1: Fq::from_str("3132697209754082586339430915081913810572071485832539443682634025529375380328136128542015469873094481703191673087029").unwrap()}
            );
        }

        {
            //    msg     = abc
            // u[0]    = 15f7c0aa8f6b296ab5ff9c2c7581ade64f4ee6f1bf18f55179ff44
            //         a2cf355fa53dd2a2158c5ecb17d7c52f63e7195771
            //   + I * 01c8067bf4c0ba709aa8b9abc3d1cef589a4758e09ef53732d670f
            //         d8739a7274e111ba2fcaa71b3d33df2a3a0c8529dd
            // u[1]    = 187111d5e088b6b9acfdfad078c4dacf72dcd17ca17c82be35e79f
            //         8c372a693f60a033b461d81b025864a0ad051a06e4
            //   + I * 08b852331c96ed983e497ebc6dee9b75e373d923b729194af8e72a
            //         051ea586f3538a6ebb1e80881a082fa2b24df9f566
            let msg = b"abc";
            let (u0, u1) = hash_to_field_fq2(msg, dst);
            assert_eq!(
                u0,
                Fq2{
                c0: Fq::from_str("3381151350286428005095780827831774583653641216459357823974407145557165174365389989442078766443621078367363453769585").unwrap(),
                c1: Fq::from_str("274174695370444263853418070745339731640467919355184108253716879519695397069963034977795744692362177212201505728989").unwrap()}
            );
            assert_eq!(
                u1,
                Fq2{
                c0: Fq::from_str("3761918608077574755256083960277010506684793456226386707192711779006489497410866269311252402421709839991039401264868").unwrap(),
                c1: Fq::from_str("1342131492846344403298252211066711749849099599627623100864413228392326132610002371925674088601653350525231531947366").unwrap()}
            );
        }
    }
    
    #[test]
    fn test_hash_to_curve_g2() {
        let dst = b"QUUX-V01-CS02-with-BLS12381G2_XMD:SHA-256_SSWU_RO_";
        
        {


            //    msg     =
            //    P.x     = 0141ebfbdca40eb85b87142e130ab689c673cf60f1a3e98d69335266f30d9b8d4ac44c1038e9dcdd5393faf5c41fb78a
            //        + I * 05cb8437535e20ecffaef7752baddf98034139c38452458baeefab379ba13dff5bf5dd71b72418717047f5b0f37da03d
            //    P.y     = 0503921d7f6a12805e72940b963c0cf3471c7b2a524950ca195d11062ee75ec076daf2d4bc358c4b190c0c98064fdd92
            //        + I * 12424ac32561493f3fe3c260708a12b7c620e7be00099a974e259ddc7d1f6395c3c811cdd19f1e8dbf3e9ecfdcbab8d6
            //    Q0.x    = 019ad3fc9c72425a998d7ab1ea0e646a1f6093444fc6965f1cad5a3195a7b1e099c050d57f45e3fa191cc6d75ed7458c
            //        + I * 171c88b0b0efb5eb2b88913a9e74fe111a4f68867b59db252ce5868af4d1254bfab77ebde5d61cd1a86fb2fe4a5a1c1d
            //    Q0.y    = 0ba10604e62bdd9eeeb4156652066167b72c8d743b050fb4c1016c31b505129374f76e03fa127d6a156213576910fef3
            //        + I * 0eb22c7a543d3d376e9716a49b72e79a89c9bfe9feee8533ed931cbb5373dde1fbcd7411d8052e02693654f71e15410a
            //    Q1.x    = 113d2b9cd4bd98aee53470b27abc658d91b47a78a51584f3d4b950677cfb8a3e99c24222c406128c91296ef6b45608be
            //        + I * 13855912321c5cb793e9d1e88f6f8d342d49c0b0dbac613ee9e17e3c0b3c97dfbb5a49cc3fb45102fdbaf65e0efe2632
            //    Q1.y    = 0fd3def0b7574a1d801be44fde617162aa2e89da47f464317d9bb5abc3a7071763ce74180883ad7ad9a723a9afafcdca
            //        + I * 056f617902b3c0d0f78a9a8cbda43a26b65f602f8786540b9469b060db7b38417915b413ca65f875c130bebfaa59790c
            //    Decimal:
            //    P.x     = 193548053368451749411421515628510806626565736652086807419354395577367693778571452628423727082668900187036482254730
            //        + I * 891930009643099423308102777951250899694559203647724988361022851024990473423938537113948850338098230396747396259901
            let p_x = Fq2{
                c0: Fq::from_str("193548053368451749411421515628510806626565736652086807419354395577367693778571452628423727082668900187036482254730").unwrap(),
                c1: Fq::from_str("891930009643099423308102777951250899694559203647724988361022851024990473423938537113948850338098230396747396259901").unwrap()
            };
            //    P.y     = 771717272055834152378281705972671257005357145478800908373659404991537354153455452961747174765859335819766715637138
            //        + I * 2810310118582126634041133454180705304393079139103252956502404531123692847658283858246402311867775854528543237781718
            let p_y = Fq2{
                c0: Fq::from_str("771717272055834152378281705972671257005357145478800908373659404991537354153455452961747174765859335819766715637138").unwrap(),
                c1: Fq::from_str("2810310118582126634041133454180705304393079139103252956502404531123692847658283858246402311867775854528543237781718").unwrap()
            };
            let p_should_be = from_coordinates_unchecked(p_x, p_y, Fq2::one());
            //    Q0.x    = 247000889425909073323253760662594248478519539052718751429094202182751397921429811614953873291195197910072700650892
            //        + I * 3557179370195599083109501581838000826052321867195478666908439992121263526125384222649169667449608345548902519938077
            let q0_x = Fq2{
                c0: Fq::from_str("247000889425909073323253760662594248478519539052718751429094202182751397921429811614953873291195197910072700650892").unwrap(),
                c1: Fq::from_str("3557179370195599083109501581838000826052321867195478666908439992121263526125384222649169667449608345548902519938077").unwrap()
            };
            //    Q0.y    = 1789866621042807238102907475382506332034840965291028464945081245097279248221497616806901995510849528127582528143091
            //        + I * 2261920060396917200558995605865363952988463533408187402812091326590595155556733986360256617149524560595567798206730
            let q0_y = Fq2{
                c0: Fq::from_str("1789866621042807238102907475382506332034840965291028464945081245097279248221497616806901995510849528127582528143091").unwrap(),
                c1: Fq::from_str("2261920060396917200558995605865363952988463533408187402812091326590595155556733986360256617149524560595567798206730").unwrap()
            };
            let q0_should_be = from_coordinates_unchecked(q0_x, q0_y, Fq2::one());
            //    Q1.x    = 2653316741049867356846339142779301820246227038474367602164293991028731662252487887055483099994673757855056102033598
            //        + I * 3004540012464469496149443751502035824386563338581531881619960946487251912156763500062348703680303970725657264924210
            let q1_x = Fq2{
                c0: Fq::from_str("2653316741049867356846339142779301820246227038474367602164293991028731662252487887055483099994673757855056102033598").unwrap(),
                c1: Fq::from_str("3004540012464469496149443751502035824386563338581531881619960946487251912156763500062348703680303970725657264924210").unwrap()
            };
            //    Q1.y    = 2436093761503339277710533452184041720241350573820092656898129088132931367043020801076222585008031239228777997258186
            //        + I * 836535538336124528574557550904612322806859485510882466665227695209180661987073534533776044142505491651567017359628
            let q1_y = Fq2{
                c0: Fq::from_str("2436093761503339277710533452184041720241350573820092656898129088132931367043020801076222585008031239228777997258186").unwrap(),
                c1: Fq::from_str("836535538336124528574557550904612322806859485510882466665227695209180661987073534533776044142505491651567017359628").unwrap()
            };
            let q1_should_be = from_coordinates_unchecked(q1_x, q1_y, Fq2::one());

            let msg = b"";
            let (u0, u1) = hash_to_field_fq2(msg, dst);
            // let q0x0 = Fq::from_str("247000889425909073323253760662594248478519539052718751429094202182751397921429811614953873291195197910072700650892").unwrap();
            // let q0x1 = Fq::from_str("3557179370195599083109501581838000826052321867195478666908439992121263526125384222649169667449608345548902519938077").unwrap();
            // let q0x = Fq2{c0: q0x0, c1: q0x1};
            // let q0y0 = Fq::from_str("1789866621042807238102907475382506332034840965291028464945081245097279248221497616806901995510849528127582528143091").unwrap();
            // let q0y1 = Fq::from_str("2261920060396917200558995605865363952988463533408187402812091326590595155556733986360256617149524560595567798206730").unwrap();
            // let q0y = Fq2{c0: q0y0, c1: q0y1};
            
            let q0 = map_to_curve_g2(u0);
            assert_eq!(q0, q0_should_be);
            let q1 = map_to_curve_g2(u1);
            assert_eq!(q1, q1_should_be);

            let p = hash_to_curve_g2(msg, dst);

            println!("P computed:  {:#x?}", p);
            println!("P specified: {:#x?}", p_should_be);

            assert_eq!(p, p_should_be);
            
            
            
        }
    }
}