// -*- mode: rust; -*-
//
// Authors:
// - bm@concordium.com

//! A known message

#[cfg(feature = "serde")]
use serde::de::Error as SerdeError;
#[cfg(feature = "serde")]
use serde::de::Visitor;
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};
#[cfg(feature = "serde")]
use serde::{Deserializer, Serializer};

use crate::errors::{
    InternalError::{CurveDecodingError, FieldDecodingError, PublicKeyLengthError},
    *,
};
use curve_arithmetic::curve_arithmetic::*;

use pairing::bls12_381::Bls12;

use curve_arithmetic::bls12_381_instance::*;
use rand::*;

/// A message
#[derive(Debug)]
pub struct PublicKey<C: Pairing>(
    pub(crate) Vec<C::G_1>,
    pub(crate) Vec<C::G_2>,
    pub(crate) C::G_2,
);

impl<C: Pairing> PartialEq for PublicKey<C> {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0 && self.1 == other.1 && self.2 == other.2
    }
}

impl<C: Pairing> Eq for PublicKey<C> {}

impl<C: Pairing> PublicKey<C> {
    // turn message vector into a byte aray
    #[inline]
    pub fn to_bytes(&self) -> Box<[u8]> {
        let vs = &self.0;
        let us = &self.1;
        let s = &self.2;
        let mut bytes: Vec<u8> = Vec::new();
        for v in vs.iter() {
            bytes.extend_from_slice(&*C::G_1::curve_to_bytes(&v));
        }
        for u in us.iter() {
            bytes.extend_from_slice(&*C::G_2::curve_to_bytes(&u));
        }
        bytes.extend_from_slice(&*C::G_2::curve_to_bytes(s));
        bytes.into_boxed_slice()
    }

    /// Construct a message vec from a slice of bytes.
    ///
    /// A `Result` whose okay value is a message vec  or whose error value
    /// is an `SignatureError` wrapping the internal error that occurred.
    #[inline]
    pub fn from_bytes(bytes: &[u8]) -> Result<PublicKey<C>, SignatureError> {
        let l = bytes.len();
        if l < (C::G_2::GROUP_ELEMENT_LENGTH * 2 + C::G_1::GROUP_ELEMENT_LENGTH)
            || (l - C::G_2::GROUP_ELEMENT_LENGTH)
                % (C::G_1::GROUP_ELEMENT_LENGTH + C::G_2::GROUP_ELEMENT_LENGTH)
                != 0
        {
            return Err(SignatureError(PublicKeyLengthError));
        }
        let vlen = (l - C::G_2::GROUP_ELEMENT_LENGTH)
            / (C::G_1::GROUP_ELEMENT_LENGTH + C::G_2::GROUP_ELEMENT_LENGTH);
        let mut vs: Vec<C::G_1> = Vec::new();
        for i in 0..vlen {
            let j = i * C::G_1::GROUP_ELEMENT_LENGTH;
            let k = j + C::G_1::GROUP_ELEMENT_LENGTH;
            match C::G_1::bytes_to_curve(&bytes[j..k]) {
                Err(x) => return Err(SignatureError(CurveDecodingError)),
                Ok(fr) => vs.push(fr),
            }
        }

        let index = vlen * C::G_1::GROUP_ELEMENT_LENGTH;
        let mut us: Vec<C::G_2> = Vec::new();

        for i in 0..vlen {
            let j = i * C::G_2::GROUP_ELEMENT_LENGTH + index;
            let k = j + C::G_2::GROUP_ELEMENT_LENGTH;
            match C::G_2::bytes_to_curve(&bytes[j..k]) {
                Err(x) => return Err(SignatureError(CurveDecodingError)),
                Ok(fr) => us.push(fr),
            }
        }

        match C::G_2::bytes_to_curve(&bytes[(l - C::G_2::GROUP_ELEMENT_LENGTH)..]) {
            Err(x) => Err(SignatureError(CurveDecodingError)),
            Ok(fr) => Ok(PublicKey(vs, us, fr)),
        }
    }

    /// Generate a secret key  from a `csprng`.
    pub fn generate<T>(n: usize, csprng: &mut T) -> PublicKey<C>
    where
        T: Rng, {
        let mut vs: Vec<C::G_1> = Vec::new();
        for _i in 0..n {
            vs.push(C::G_1::generate(csprng));
        }

        let mut us: Vec<C::G_2> = Vec::new();
        for _i in 0..n {
            us.push(C::G_2::generate(csprng));
        }

        PublicKey(vs, us, C::G_2::generate(csprng))
    }
}

macro_rules! macro_test_public_key_to_byte_conversion {
    ($function_name:ident, $pairing_type:path) => {
        #[test]
        pub fn $function_name() {
            let mut csprng = thread_rng();
            for i in 1..20 {
                let val = PublicKey::<$pairing_type>::generate(i, &mut csprng);
                let res_val2 = PublicKey::<$pairing_type>::from_bytes(&*val.to_bytes());
                assert!(res_val2.is_ok());
                let val2 = res_val2.unwrap();
                assert_eq!(val2, val);
            }
        }
    };
}

macro_test_public_key_to_byte_conversion!(public_key_to_byte_conversion_bls12_381, Bls12);
