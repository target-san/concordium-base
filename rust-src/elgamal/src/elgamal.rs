// Authors:
// - bm@concordium.com
use crate::{cipher::*, errors::*, message::*, public::*, secret::*};
use bitvec::Bits;
use libc::size_t;
use rand::*;
use rayon::prelude::*;
use std::slice;

// #[cfg(test)]
// use pairing::bls12_381::FrRepr;
// #[cfg(test)]
// use pairing::PrimeField;
use curve_arithmetic::curve_arithmetic::Curve;
use pairing::bls12_381::{G1, G2};

// TODO: Duplicated from ps_sig!
macro_rules! slice_from_c_bytes_worker {
    ($cstr:expr, $length:expr, $null_ptr_error:expr, $reader:expr) => {{
        assert!(!$cstr.is_null(), $null_ptr_error);
        unsafe { $reader($cstr, $length) }
    }};
}

macro_rules! slice_from_c_bytes {
    ($cstr:expr, $length:expr) => {
        slice_from_c_bytes_worker!($cstr, $length, "Null pointer.", slice::from_raw_parts)
    };
    ($cstr:expr, $length:expr, $null_ptr_error:expr) => {
        slice_from_c_bytes_worker!($cstr, $length, $null_ptr_error, slice::from_raw_parts)
    };
}

macro_rules! mut_slice_from_c_bytes {
    ($cstr:expr, $length:expr) => {
        slice_from_c_bytes_worker!($cstr, $length, "Null pointer.", slice::from_raw_parts_mut)
    };
    ($cstr:expr, $length:expr, $null_ptr_error:expr) => {
        slice_from_c_bytes_worker!($cstr, $length, $null_ptr_error, slice::from_raw_parts_mut)
    };
}

// foreign function interface
macro_rules! macro_new_secret_key_ffi {
    ($function_name:ident, $curve_type:path) => {
        #[no_mangle]
        #[allow(clippy::not_unsafe_ptr_arg_deref)]
        pub extern "C" fn $function_name() -> *mut SecretKey<$curve_type> {
            let mut csprng = thread_rng();
            Box::into_raw(Box::new(SecretKey::generate(&mut csprng)))
        }
    };
}

macro_rules! macro_free_ffi {
    ($function_name:ident, $type:path) => {
        #[no_mangle]
        #[allow(clippy::not_unsafe_ptr_arg_deref)]
        pub extern "C" fn $function_name(ptr: *mut $type) {
            if ptr.is_null() {
                return;
            }
            unsafe {
                Box::from_raw(ptr);
            }
        }
    };
}
macro_rules! from_ptr {
    ($ptr:expr) => {{
        assert!(!$ptr.is_null());
        unsafe { &*$ptr }
    }};
}

macro_rules! macro_derive_public_key_ffi {
    ($function_name:ident, $curve_type:path) => {
        #[no_mangle]
        #[allow(clippy::not_unsafe_ptr_arg_deref)]
        pub extern "C" fn $function_name(
            ptr: *mut SecretKey<$curve_type>,
        ) -> *mut PublicKey<$curve_type> {
            let sk: &SecretKey<$curve_type> = from_ptr!(ptr);
            Box::into_raw(Box::new(PublicKey::from(sk)))
        }
    };
}

macro_rules! macro_encrypt_ffi {
    ($function_name:ident, $curve_type:path) => {
        #[no_mangle]
        #[allow(clippy::not_unsafe_ptr_arg_deref)]
        pub extern "C" fn $function_name(
            pb_key_ptr: *mut PublicKey<$curve_type>,
            message_ptr: *mut Message<$curve_type>,
        ) -> *mut Cipher<$curve_type> {
            let pb_key = from_ptr!(pb_key_ptr);
            let message = from_ptr!(message_ptr);
            let mut csprng = thread_rng();
            Box::into_raw(Box::new(pb_key.encrypt(&mut csprng, &message)))
        }
    };
}

macro_rules! macro_decrypt_ffi {
    ($function_name:ident, $curve_type:path) => {
        #[no_mangle]
        #[allow(clippy::not_unsafe_ptr_arg_deref)]
        pub extern "C" fn $function_name(
            sk_key_ptr: *mut SecretKey<$curve_type>,
            cipher_ptr: *mut Cipher<$curve_type>,
        ) -> *mut Message<$curve_type> {
            let sk_key = from_ptr!(sk_key_ptr);
            let cipher = from_ptr!(cipher_ptr);
            Box::into_raw(Box::new(sk_key.decrypt(&cipher)))
        }
    };
}

macro_rules! macro_derive_to_bytes {
    ($function_name:ident, $type:ty) => {
        #[no_mangle]
        #[allow(clippy::not_unsafe_ptr_arg_deref)]
        pub extern "C" fn $function_name(
            input_ptr: *mut $type,
            output_len: *mut size_t,
        ) -> *const u8 {
            let input = from_ptr!(input_ptr);
            let bytes = input.to_bytes();
            unsafe { *output_len = bytes.len() as size_t }
            let ret_ptr = bytes.as_ptr();
            ::std::mem::forget(bytes);
            ret_ptr
        }
    };
}

macro_rules! macro_derive_from_bytes {
    ($function_name:ident, $type:ty, $from:path) => {
        #[no_mangle]
        #[allow(clippy::not_unsafe_ptr_arg_deref)]
        pub extern "C" fn $function_name(input_bytes: *mut u8, input_len: size_t) -> *const $type {
            let len = input_len as usize;
            let bytes = slice_from_c_bytes!(input_bytes, len);
            let e = $from(&bytes);
            match e {
                Ok(r) => Box::into_raw(Box::new(r)),
                Err(_) => ::std::ptr::null(),
            }
        }
    };
}

#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn free_array_len(ptr: *mut u8, len: size_t) {
    let s = mut_slice_from_c_bytes!(ptr, len as usize);
    unsafe {
        Box::from_raw(s.as_mut_ptr());
    }
}

pub fn encrypt_u64_bitwise_iter<C: Curve>(
    pk: PublicKey<C>,
    e: u64,
) -> impl IndexedParallelIterator<Item = Cipher<C>> {
    (0..64).into_par_iter().map(move |i| {
        let mut csprng = thread_rng();
        pk.hide_binary_exp(&C::generate_scalar(&mut csprng), e.get(i as u8))
    })
}

/// Generate code to encrypt a single 64 bit integer bitwise.
macro_rules! macro_encrypt_u64_ffi {
    ($function_name:ident, $curve_type:path) => {
        #[no_mangle]
        #[allow(clippy::not_unsafe_ptr_arg_deref)]
        pub extern "C" fn $function_name(ptr: *mut PublicKey<$curve_type>, e: u64, out: *mut u8) {
            let pk: &PublicKey<$curve_type> = unsafe {
                assert!(!ptr.is_null());
                &*ptr
            };
            let elen = 2 * 64 * <$curve_type as Curve>::GROUP_ELEMENT_LENGTH;
            let out_bytes = mut_slice_from_c_bytes!(out, elen);
            out_bytes.par_chunks_mut(2 * <$curve_type as Curve>::GROUP_ELEMENT_LENGTH) // each ciphertext is of this length
                                        .zip(encrypt_u64_bitwise_iter(*pk, e))
                                        .for_each(|(out_chunk, cipher)| {
                                            let mut cipher_bytes = Cipher::to_bytes(&cipher);
                                            out_chunk.swap_with_slice(&mut cipher_bytes);
                                        })
        }
    };
}

pub fn encrypt_u64_bitwise<C: Curve>(pk: PublicKey<C>, e: u64) -> Vec<Cipher<C>> {
    encrypt_u64_bitwise_iter(pk, e).collect()
}

// take an array of zero's and ones and returns a u64
pub fn group_bits_to_u64<'a, C, I>(v: I) -> u64
where
    C: Curve,
    I: Iterator<Item = &'a C>, {
    let mut r = 0u64;
    let one = C::one_point();
    for (i, &e) in v.enumerate() {
        r.set(i as u8, e == one);
    }
    r
}

/// Generate code to decrypt a single 64 bit integer bitwise.
macro_rules! macro_decrypt_u64_ffi {
    ($function_name:ident, $curve_type:path) => {
        #[no_mangle]
        #[allow(clippy::not_unsafe_ptr_arg_deref)]
        pub extern "C" fn $function_name(
            ptr: *mut SecretKey<$curve_type>,
            cipher_bytes: *const u8,
            result_ptr: *mut u64,
        ) -> i32 {
            assert!(!result_ptr.is_null());
            assert!(!ptr.is_null());
            let cipher_len = 2 * <$curve_type as Curve>::GROUP_ELEMENT_LENGTH;
            let clen = 64 * cipher_len;
            let cipher = slice_from_c_bytes!(cipher_bytes, clen);
            let sk: &SecretKey<$curve_type> = unsafe { &*ptr };
            let v: Result<Vec<$curve_type>, ElgamalError> = cipher
                .par_chunks(cipher_len)
                .map(|x| {
                    let c = Cipher::from_bytes(x)?;
                    let Message(m) = sk.decrypt(&c);
                    Ok(m)
                })
                .collect();
            match v {
                Err(_) => -1,
                Ok(vv) => {
                    let result = group_bits_to_u64(vv.iter());
                    unsafe { *result_ptr = result }
                    0
                }
            }
        }
    };
}

/// Generate code to decrypt a single 64 bit integer bitwise. This function
/// not check that the cipher is valid. It uses the unchecked conversion
/// bytes to group elements. It will panic if the ciphertext is invalid.
macro_rules! macro_decrypt_u64_unsafe_ffi {
    ($function_name:ident, $curve_type:path) => {
        #[no_mangle]
        #[allow(clippy::not_unsafe_ptr_arg_deref)]
        pub extern "C" fn $function_name(
            ptr: *mut SecretKey<$curve_type>,
            cipher_bytes: *const u8,
        ) -> u64 {
            assert!(!ptr.is_null());
            let cipher_len = 2 * <$curve_type as Curve>::GROUP_ELEMENT_LENGTH;
            let clen = 64 * cipher_len;
            let cipher = slice_from_c_bytes!(cipher_bytes, clen);
            let sk: &SecretKey<$curve_type> = unsafe { &*ptr };
            let v: Vec<$curve_type> = cipher
                .par_chunks(cipher_len)
                .map(|x| {
                    let c = Cipher::from_bytes_unchecked(x).unwrap();
                    let Message(m) = sk.decrypt(&c);
                    m
                })
                .collect();
            group_bits_to_u64(v.iter())
        }
    };
}

pub fn decrypt_u64_bitwise<C: Curve>(sk: &SecretKey<C>, v: &[Cipher<C>]) -> u64 {
    let dr: Vec<C> = v
        .par_iter()
        .map(|x| {
            let Message(m) = sk.decrypt(&x);
            m
        })
        .collect();
    group_bits_to_u64(dr.iter())
}

macro_new_secret_key_ffi!(new_secret_key_g1, G1);
macro_derive_public_key_ffi!(derive_public_key_g1, G1);
macro_encrypt_ffi!(encrypt_g1, G1);
macro_decrypt_ffi!(decrypt_g1, G1);
macro_encrypt_u64_ffi!(encrypt_u64_g1, G1);
macro_decrypt_u64_ffi!(decrypt_u64_g1, G1);
macro_decrypt_u64_unsafe_ffi!(decrypt_u64_unsafe_g1, G1);
macro_free_ffi!(free_secret_key_g1, SecretKey<G1>);
macro_free_ffi!(free_public_key_g1, PublicKey<G1>);
macro_free_ffi!(free_message_g1, Message<G1>);
macro_free_ffi!(free_cipher_g1, Cipher<G1>);
macro_derive_from_bytes!(bytes_to_message_g1, Message<G1>, Message::from_bytes);
macro_derive_from_bytes!(bytes_to_cipher_g1, Cipher<G1>, Cipher::from_bytes);
macro_derive_from_bytes!(bytes_to_secret_key_g1, SecretKey<G1>, SecretKey::from_bytes);
macro_derive_from_bytes!(bytes_to_public_key_g1, PublicKey<G1>, PublicKey::from_bytes);
macro_derive_to_bytes!(message_to_bytes_g1, Message<G1>);
macro_derive_to_bytes!(public_key_to_bytes_g1, PublicKey<G1>);
macro_derive_to_bytes!(secret_key_to_bytes_g1, SecretKey<G1>);
macro_derive_to_bytes!(cipher_to_bytes_g1, Cipher<G1>);

macro_new_secret_key_ffi!(new_secret_key_g2, G2);
macro_derive_public_key_ffi!(derive_public_key_g2, G2);
macro_encrypt_ffi!(encrypt_g2, G2);
macro_decrypt_ffi!(decrypt_g2, G2);
macro_encrypt_u64_ffi!(encrypt_u64_g2, G2);
macro_decrypt_u64_ffi!(decrypt_u64_g2, G2);
macro_decrypt_u64_unsafe_ffi!(decrypt_u64_unsafe_g2, G2);
macro_free_ffi!(free_secret_key_g2, SecretKey<G2>);
macro_free_ffi!(free_public_key_g2, PublicKey<G2>);
macro_free_ffi!(free_message_g2, Message<G2>);
macro_free_ffi!(free_cipher_g2, Cipher<G2>);
macro_derive_from_bytes!(bytes_to_message_g2, Message<G2>, Message::from_bytes);
macro_derive_from_bytes!(bytes_to_cipher_g2, Cipher<G2>, Cipher::from_bytes);
macro_derive_from_bytes!(bytes_to_secret_key_g2, SecretKey<G2>, SecretKey::from_bytes);
macro_derive_from_bytes!(bytes_to_public_key_g2, PublicKey<G2>, PublicKey::from_bytes);
macro_derive_to_bytes!(message_to_bytes_g2, Message<G2>);
macro_derive_to_bytes!(public_key_to_bytes_g2, PublicKey<G2>);
macro_derive_to_bytes!(secret_key_to_bytes_g2, SecretKey<G2>);
macro_derive_to_bytes!(cipher_to_bytes_g2, Cipher<G2>);

#[cfg(test)]
mod tests {
    use super::*;
    use pairing::Field;
    macro_rules! macro_test_encrypt_decrypt_success {
        ($function_name:ident, $curve_type:path) => {
            #[test]
            pub fn $function_name() {
                let mut csprng = thread_rng();
                for _i in 1..100 {
                    let sk: SecretKey<$curve_type> = SecretKey::generate(&mut csprng);
                    let pk = PublicKey::from(&sk);
                    let m = Message::generate(&mut csprng);
                    let c = pk.encrypt(&mut csprng, &m);
                    let mm = sk.decrypt(&c);
                    assert_eq!(m, mm);

                    // encrypting again gives a different ciphertext (very likely at least)
                    let canother = pk.encrypt(&mut csprng, &m);
                    assert_ne!(c, canother);
                }
            }
        };
    }

    macro_test_encrypt_decrypt_success!(encrypt_decrypt_success_g1, G1);
    macro_test_encrypt_decrypt_success!(encrypt_decrypt_success_g2, G2);

    macro_rules! macro_test_encrypt_decrypt_exponent_success {
        ($function_name:ident, $curve_type:path) => {
            #[test]
            pub fn $function_name() {
                let mut csprng = thread_rng();
                let sk: SecretKey<$curve_type> = SecretKey::generate(&mut csprng);
                let pk = PublicKey::from(&sk);
                for _i in 1..100 {
                    let n = u64::rand(&mut csprng);
                    let mut e = <$curve_type as Curve>::Scalar::zero();
                    let one_scalar = <$curve_type as Curve>::Scalar::one();
                    for _ in 0..(n % 1000) {
                        e.add_assign(&one_scalar);
                    }
                    let c = pk.encrypt_exponent(&mut csprng, &e);
                    let e2 = sk.decrypt_exponent(&c);
                    assert_eq!(e, e2);
                }
            }
        };
    }

    macro_test_encrypt_decrypt_exponent_success!(encrypt_decrypt_exponent_success_g1, G1);
    macro_test_encrypt_decrypt_exponent_success!(encrypt_decrypt_exponent_success_g2, G2);

    macro_rules! macro_test_encrypt_decrypt_bitwise_vec_success {
        ($function_name:ident, $curve_type:path) => {
            #[test]
            pub fn $function_name() {
                let mut csprng = thread_rng();
                let sk: SecretKey<$curve_type> = SecretKey::generate(&mut csprng);
                let pk = PublicKey::from(&sk);
                for _i in 1..100 {
                    let n = u64::rand(&mut csprng);
                    let c = encrypt_u64_bitwise(pk, n);
                    let n2 = decrypt_u64_bitwise(&sk, &c);
                    assert_eq!(n, n2);
                }
            }
        };
    }

    macro_test_encrypt_decrypt_bitwise_vec_success!(encrypt_decrypt_bitwise_vec_success_g1, G1);
    macro_test_encrypt_decrypt_bitwise_vec_success!(encrypt_decrypt_bitwise_vec_success_g2, G2);

    macro_rules! macro_test_encrypt_decrypt_u64_ffi {
        (
            $function_name:ident,
            $new_secret_key_name:ident,
            $derive_public_name:ident,
            $encrypt_name:ident,
            $decrypt_name:ident,
            $size:expr
        ) => {
            #[test]
            pub fn $function_name() {
                let byte_size = $size * 2 * 64;
                let sk = $new_secret_key_name();
                let pk = $derive_public_name(sk);
                let mut xs = vec![0 as u8; byte_size];
                let mut csprng = thread_rng();
                for _i in 1..100 {
                    let n = u64::rand(&mut csprng);
                    $encrypt_name(pk, n, xs.as_mut_ptr());
                    let result_ptr = Box::into_raw(Box::new(0));
                    let m = $decrypt_name(sk, xs.as_ptr(), result_ptr);
                    assert_eq!(m, 0);
                    assert_eq!(unsafe { *result_ptr }, n);
                }
            }
        };
    }

    macro_test_encrypt_decrypt_u64_ffi! {
        encrypt_decrypt_u64_g1_ffi,
        new_secret_key_g1,
        derive_public_key_g1,
        encrypt_u64_g1,
        decrypt_u64_g1,
        <G1 as Curve>::GROUP_ELEMENT_LENGTH
    }

    macro_test_encrypt_decrypt_u64_ffi! {
        encrypt_decrypt_u64_g2_ffi,
        new_secret_key_g2,
        derive_public_key_g2,
        encrypt_u64_g2,
        decrypt_u64_g2,
        <G2 as Curve>::GROUP_ELEMENT_LENGTH
    }

    macro_rules! macro_test_encrypt_decrypt_u64_unchecked_ffi {
        (
            $function_name:ident,
            $new_secret_key_name:ident,
            $derive_public_name:ident,
            $encrypt_name:ident,
            $decrypt_name:ident,
            $size:expr
        ) => {
            #[test]
            pub fn $function_name() {
                let byte_size = $size * 2 * 64;
                let sk = $new_secret_key_name();
                let pk = $derive_public_name(sk);
                let mut xs = vec![0; byte_size];
                let mut csprng = thread_rng();
                for _i in 1..100 {
                    let n = u64::rand(&mut csprng);
                    $encrypt_name(pk, n, xs.as_mut_ptr());
                    let m = $decrypt_name(sk, xs.as_ptr());
                    assert_eq!(m, n);
                }
            }
        };
    }

    macro_test_encrypt_decrypt_u64_unchecked_ffi! {
        encrypt_decrypt_u64_g1_ffi_unsafe,
        new_secret_key_g1,
        derive_public_key_g1,
        encrypt_u64_g1,
        decrypt_u64_unsafe_g1,
        <G1 as Curve>::GROUP_ELEMENT_LENGTH
    }

    macro_test_encrypt_decrypt_u64_unchecked_ffi! {
        encrypt_decrypt_u64_g2_ffi_unsafe,
        new_secret_key_g2,
        derive_public_key_g2,
        encrypt_u64_g2,
        decrypt_u64_unsafe_g2,
        <G2 as Curve>::GROUP_ELEMENT_LENGTH
    }
}
