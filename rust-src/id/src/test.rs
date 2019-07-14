use std::fmt;

use crate::{account_holder::*, chain::*, identity_provider::*, types::*};
use curve_arithmetic::{Curve, Pairing};
use dodis_yampolskiy_prf::secret as prf;
use eddsa_ed25519 as ed25519;
use elgamal::{public::PublicKey, secret::SecretKey};
use pairing::{
    bls12_381::{Bls12, Fr, FrRepr},
    PrimeField,
};
use ps_sig;

use chrono::NaiveDateTime;

use rand::*;

use pedersen_scheme::key as pedersen_key;

type ExampleCurve = <Bls12 as Pairing>::G_1;

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
#[allow(dead_code)]
pub enum ExampleAttribute {
    Age(u8),
    Citizenship(u16),
    MaxAccount(u16),
    Business(bool),
}

type ExampleAttributeList = AttributeList<<Bls12 as Pairing>::ScalarField, ExampleAttribute>;

impl fmt::Display for ExampleAttribute {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExampleAttribute::Age(x) => write!(f, "Age({})", x),
            ExampleAttribute::Citizenship(c) => write!(f, "Citizenship({})", c),
            ExampleAttribute::MaxAccount(x) => write!(f, "MaxAccount({})", x),
            ExampleAttribute::Business(b) => write!(f, "Business({})", b),
        }
    }
}

impl Attribute<<Bls12 as Pairing>::ScalarField> for ExampleAttribute {
    fn to_field_element(&self) -> <Bls12 as Pairing>::ScalarField {
        match self {
            ExampleAttribute::Age(x) => Fr::from_repr(FrRepr::from(u64::from(*x))).unwrap(),
            ExampleAttribute::Citizenship(c) => Fr::from_repr(FrRepr::from(u64::from(*c))).unwrap(),
            ExampleAttribute::MaxAccount(x) => Fr::from_repr(FrRepr::from(u64::from(*x))).unwrap(),
            ExampleAttribute::Business(b) => Fr::from_repr(FrRepr::from(u64::from(*b))).unwrap(),
        }
    }
}

#[test]
fn test_pipeline() {
    let mut csprng = thread_rng();

    let secret = ExampleCurve::generate_scalar(&mut csprng);
    let public = ExampleCurve::one_point().mul_by_scalar(&secret);
    let ah_info = CredentialHolderInfo::<ExampleCurve, ExampleCurve> {
        id_ah:   "ACCOUNT_HOLDER".to_owned(),
        id_cred: IdCredentials {
            id_cred_sec:    secret,
            id_cred_pub:    public,
            id_cred_pub_ip: public,
        },
    };

    let id_secret_key = ps_sig::secret::SecretKey::<Bls12>::generate(10, &mut csprng);
    let id_public_key = ps_sig::public::PublicKey::from(&id_secret_key);

    let ar_secret_key = SecretKey::generate(&mut csprng);
    let ar_public_key = PublicKey::from(&ar_secret_key);
    let ar_info = ArInfo {
        ar_name: "AR".to_owned(),
        ar_public_key,
        ar_elgamal_generator: PublicKey::generator(),
    };

    let ip_info = IpInfo {
        ip_identity: "ID".to_owned(),
        ip_verify_key: id_public_key,
        ar_info,
    };

    let prf_key = prf::SecretKey::generate(&mut csprng);

    let variant = 0;
    let expiry_date = NaiveDateTime::from_timestamp(12334, 0);
    let alist = Vec::new();
    let aci = AccCredentialInfo {
        acc_holder_info: ah_info,
        prf_key,
        attributes: ExampleAttributeList {
            variant,
            expiry: expiry_date,
            alist,
            _phantom: Default::default(),
        },
    };

    let context = make_context_from_ip_info(ip_info.clone());
    let (pio, randomness) = generate_pio(&context, &aci);

    let sig_ok = verify_credentials(&pio, context, &id_secret_key);

    // First test, check that we have a valid signature.
    assert!(sig_ok.is_ok());

    let ip_sig = sig_ok.unwrap();

    let global_ctx = GlobalContext {
        dlog_base_chain:         ExampleCurve::one_point(),
        on_chain_commitment_key: pedersen_key::CommitmentKey::generate(1, &mut csprng),
    };

    let policy = Policy {
        variant,
        expiry: expiry_date,
        policy_vec: Vec::new(),
    };

    let kp = ed25519::generate_keypair();
    let acc_data = AccountData {
        sign_key:   kp.secret,
        verify_key: kp.public,
    };

    let cdi = generate_cdi(
        &ip_info,
        &global_ctx,
        &aci,
        &pio,
        0,
        &ip_sig,
        &policy,
        &acc_data,
        &randomness,
    );
    // now check that the generated credentials are indeed valid.
    let cdi_check = verify_cdi(&global_ctx, ip_info, cdi);
    assert_eq!(cdi_check, Ok(()));
}
