use core::fmt;

pub use bellman_ce::bn256::{Bn256, Fr};
use bellman_ce::groth16::{self, PreparedVerifyingKey as RawPreparedVerifyingKey};
use bellman_ce::groth16::{Parameters as RawParameters, Proof as RawProof};
use bellman_ce::pairing::ff::PrimeField;
use bellman_ce::Circuit as RawCircuit;
pub use bellman_ce::Circuit as BellmanCircuit;
use bellman_ce::ConstraintSystem as RawConstraintSystem;
pub use bellman_ce::ConstraintSystem as BellmanConstraintSystem;
use rand_core::RngCore;

struct LegacyRng<'a, R> {
    inner: &'a mut R,
}

impl<'a, R> LegacyRng<'a, R> {
    fn new(inner: &'a mut R) -> Self {
        Self { inner }
    }
}

impl<'a, R> rand04::Rng for LegacyRng<'a, R>
where
    R: RngCore,
{
    fn next_u32(&mut self) -> u32 {
        self.inner.next_u32()
    }

    fn next_u64(&mut self) -> u64 {
        self.inner.next_u64()
    }

    fn fill_bytes(&mut self, dest: &mut [u8]) {
        self.inner.fill_bytes(dest)
    }
}
use thiserror::Error;

pub use bellman_ce::SynthesisError;

#[derive(Clone)]
pub struct Parameters(RawParameters<Bn256>);

#[derive(Clone)]
pub struct Proof(RawProof<Bn256>);

pub struct PreparedVerifyingKey(RawPreparedVerifyingKey<Bn256>);

#[derive(Clone, PartialEq, Eq)]
pub struct FieldElement(Fr);

#[derive(Debug, Error)]
pub enum Groth16Error {
    #[error("synthesis failed: {0}")]
    Synthesis(#[from] SynthesisError),
    #[error("proof system error: {0}")]
    Proof(String),
    #[error("field conversion failed: {0}")]
    FieldConversion(String),
}

pub trait Circuit: Clone {
    type Inner: RawCircuit<Bn256> + Clone;

    fn into_inner(self) -> Self::Inner;
}

impl<T> Circuit for T
where
    T: Clone + RawCircuit<Bn256>,
{
    type Inner = T;

    fn into_inner(self) -> Self::Inner {
        self
    }
}

pub trait ConstraintSystem: RawConstraintSystem<Bn256> {}

impl<T> ConstraintSystem for T where T: RawConstraintSystem<Bn256> {}

pub struct Groth16Bn256;

impl Groth16Bn256 {
    pub fn setup<C, R>(circuit: C, rng: &mut R) -> Result<Parameters, Groth16Error>
    where
        C: Circuit,
        R: RngCore,
    {
        let mut legacy_rng = LegacyRng::new(rng);
        let params = groth16::generate_random_parameters::<Bn256, _, _>(
            circuit.into_inner(),
            &mut legacy_rng,
        )?;
        Ok(Parameters(params))
    }

    pub fn prove<C, R>(params: &Parameters, circuit: C, rng: &mut R) -> Result<Proof, Groth16Error>
    where
        C: Circuit,
        R: RngCore,
    {
        let mut legacy_rng = LegacyRng::new(rng);
        let proof = groth16::create_random_proof::<Bn256, _, _, _>(
            circuit.into_inner(),
            &params.0,
            &mut legacy_rng,
        )?;
        Ok(Proof(proof))
    }

    pub fn prepare_verifying_key(params: &Parameters) -> PreparedVerifyingKey {
        PreparedVerifyingKey(groth16::prepare_verifying_key(&params.0.vk))
    }

    pub fn verify(
        pvk: &PreparedVerifyingKey,
        proof: &Proof,
        inputs: &[FieldElement],
    ) -> Result<bool, Groth16Error> {
        let scalars: Vec<Fr> = inputs.iter().map(|f| f.0.clone()).collect();
        groth16::verify_proof(&pvk.0, &proof.0, &scalars)
            .map_err(|e| Groth16Error::Proof(e.to_string()))
    }
}

impl Parameters {
    pub fn inner(&self) -> &RawParameters<Bn256> {
        &self.0
    }
}

impl Proof {
    pub fn inner(&self) -> &RawProof<Bn256> {
        &self.0
    }
}

impl PreparedVerifyingKey {
    pub fn inner(&self) -> &RawPreparedVerifyingKey<Bn256> {
        &self.0
    }
}

impl fmt::Debug for Parameters {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Parameters").finish_non_exhaustive()
    }
}

impl fmt::Debug for Proof {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Proof").finish_non_exhaustive()
    }
}

impl fmt::Debug for PreparedVerifyingKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PreparedVerifyingKey")
            .finish_non_exhaustive()
    }
}

impl fmt::Debug for FieldElement {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("FieldElement").finish()
    }
}

impl FieldElement {
    pub fn from_u64(value: u64) -> Self {
        FieldElement::from_str(&value.to_string())
            .expect("u64 values should map into the scalar field")
    }

    pub fn from_str(value: &str) -> Result<Self, Groth16Error> {
        Fr::from_str(value)
            .ok_or_else(|| Groth16Error::FieldConversion(format!("invalid field element: {value}")))
            .map(Self)
    }

    pub fn inner(&self) -> &Fr {
        &self.0
    }

    pub fn clone_inner(&self) -> Fr {
        self.0.clone()
    }
}

impl From<Fr> for FieldElement {
    fn from(value: Fr) -> Self {
        FieldElement(value)
    }
}

impl From<FieldElement> for Fr {
    fn from(value: FieldElement) -> Self {
        value.0
    }
}
