#![forbid(unsafe_code)]

#[cfg(feature = "telemetry")]
use crate::telemetry;
use concurrency::{mutex, Lazy, MutexExt, MutexT};
use crypto_suite::hashing::blake3::Hasher;
use crypto_suite::zk::groth16::{
    Circuit as GrothCircuit, FieldElement, Groth16Bn256, Groth16Error, Parameters,
    PreparedVerifyingKey, Proof,
};
use foundation_bigint::BigUint;
use foundation_serialization::binary;
use foundation_serialization::serde::{
    de::{self, SeqAccess, Visitor},
    ser::SerializeTupleStruct,
};
use foundation_serialization::{Deserialize, Serialize};
use std::collections::HashMap;
use std::convert::TryFrom;
use std::fmt;
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use thiserror::Error;

static CIRCUIT_CACHE: Lazy<MutexT<HashMap<[u8; 32], Arc<CompiledCircuit>>>> =
    Lazy::new(|| mutex(HashMap::new()));

const SNARK_BACKEND_ENV: &str = "TB_SNARK_BACKEND";

#[derive(Clone)]
struct CompiledCircuit {
    digest: [u8; 32],
    params: Arc<Parameters>,
    verifier: Arc<PreparedVerifyingKey>,
}

#[derive(Clone)]
struct CircuitInputs {
    wasm_hash: [u8; 32],
    output_hash: [u8; 32],
    witness_hash: [u8; 32],
    program_fe: FieldElement,
    output_fe: FieldElement,
    witness_fe: FieldElement,
}

impl CircuitInputs {
    fn derive(wasm: &[u8], output: &[u8]) -> Self {
        let wasm_hash = digest(wasm);
        let output_hash = digest(output);
        let mut h = Hasher::new();
        h.update(&wasm_hash);
        h.update(&output_hash);
        let witness_hash = *h.finalize().as_bytes();
        let program_fe = field_from_bytes(&wasm_hash);
        let output_fe = field_from_bytes(&output_hash);
        let witness_fe = output_fe.clone() - program_fe.clone();

        Self {
            wasm_hash,
            output_hash,
            witness_hash,
            program_fe,
            output_fe,
            witness_fe,
        }
    }
}

#[derive(Clone)]
struct ProgramCircuit {
    program: FieldElement,
    output: FieldElement,
    witness: FieldElement,
}

impl ProgramCircuit {
    fn blank() -> Self {
        Self {
            program: FieldElement::zero(),
            output: FieldElement::zero(),
            witness: FieldElement::zero(),
        }
    }

    fn new(program: FieldElement, output: FieldElement, witness: FieldElement) -> Self {
        Self {
            program,
            output,
            witness,
        }
    }
}

impl GrothCircuit for ProgramCircuit {
    fn synthesize<CS: crypto_suite::zk::groth16::ConstraintSystem>(
        self,
        cs: &mut CS,
    ) -> Result<(), crypto_suite::zk::groth16::SynthesisError> {
        let program_var = cs.alloc_input(
            || "program_commitment".to_string(),
            || Ok(self.program.clone()),
        )?;
        let output_var = cs.alloc_input(
            || "output_commitment".to_string(),
            || Ok(self.output.clone()),
        )?;
        let witness_var = cs.alloc(
            || "witness_commitment".to_string(),
            || Ok(self.witness.clone()),
        )?;
        cs.enforce(
            || "program_plus_witness_equals_output".to_string(),
            |lc| lc + program_var + witness_var,
            |lc| lc + CS::one(),
            |lc| lc + output_var,
        );
        Ok(())
    }
}

#[derive(Debug, Error)]
pub enum SnarkError {
    #[error("snark backend error: {0}")]
    Backend(#[from] Groth16Error),
    #[error("encoding error: {0}")]
    Encoding(String),
    #[error("gpu backend requested but unavailable")]
    GpuUnavailable,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(crate = "foundation_serialization::serde")]
pub enum SnarkBackend {
    Cpu,
    Gpu,
}

impl SnarkBackend {
    #[allow(dead_code)]
    fn as_label(&self) -> &'static str {
        match self {
            SnarkBackend::Cpu => "cpu",
            SnarkBackend::Gpu => "gpu",
        }
    }

    fn tag(self) -> u8 {
        match self {
            SnarkBackend::Cpu => 0,
            SnarkBackend::Gpu => 1,
        }
    }

    fn from_tag(tag: u8) -> Result<Self, SnarkError> {
        match tag {
            0 => Ok(SnarkBackend::Cpu),
            1 => Ok(SnarkBackend::Gpu),
            other => Err(SnarkError::Encoding(format!(
                "unknown snark backend tag {other}"
            ))),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProofBundle {
    pub backend: SnarkBackend,
    pub circuit_hash: [u8; 32],
    pub program_commitment: [u8; 32],
    pub output_commitment: [u8; 32],
    pub witness_commitment: [u8; 32],
    pub public_inputs: Vec<[u8; 32]>,
    pub aux_assignments: Vec<[u8; 32]>,
    pub encoded: Vec<u8>,
    pub latency_ms: u64,
    pub artifact: CircuitArtifact,
}

impl ProofBundle {
    /// Return a unique identifier for auditing.
    pub fn fingerprint(&self) -> [u8; 32] {
        let mut h = Hasher::new();
        h.update(&self.encoded);
        h.update(&self.circuit_hash);
        h.update(&[self.backend as u8]);
        *h.finalize().as_bytes()
    }

    /// Validate that the stored field assignments satisfy the circuit relation.
    pub fn self_check(&self) -> bool {
        match (
            self.public_inputs.get(0),
            self.public_inputs.get(1),
            self.aux_assignments.get(0),
        ) {
            (Some(program), Some(output), Some(witness)) => {
                let program_fe = field_from_bytes(program);
                let output_fe = field_from_bytes(output);
                let witness_fe = field_from_bytes(witness);
                program_fe + witness_fe == output_fe
            }
            _ => false,
        }
    }

    /// Rehydrate a proof bundle from serialized commitments and encoded witness data.
    pub fn from_encoded_parts(
        backend: SnarkBackend,
        circuit_hash: [u8; 32],
        program_commitment: [u8; 32],
        output_commitment: [u8; 32],
        witness_commitment: [u8; 32],
        encoded: Vec<u8>,
        latency_ms: u64,
        artifact: CircuitArtifact,
    ) -> Result<Self, SnarkError> {
        let payload = decode_proof(&encoded)?;
        if payload.backend != backend {
            return Err(SnarkError::Encoding(
                "backend mismatch in encoded proof".into(),
            ));
        }
        Ok(Self {
            backend,
            circuit_hash,
            program_commitment,
            output_commitment,
            witness_commitment,
            public_inputs: payload.public_inputs,
            aux_assignments: payload.aux_assignments,
            encoded,
            latency_ms,
            artifact,
        })
    }
}

impl Serialize for ProofBundle {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: foundation_serialization::serde::Serializer,
    {
        let mut state = serializer.serialize_tuple_struct("ProofBundle", 10)?;
        let backend_tag = self.backend.tag();
        state.serialize_field(&backend_tag)?;
        state.serialize_field(&self.circuit_hash)?;
        state.serialize_field(&self.program_commitment)?;
        state.serialize_field(&self.output_commitment)?;
        state.serialize_field(&self.witness_commitment)?;
        state.serialize_field(&self.public_inputs)?;
        state.serialize_field(&self.aux_assignments)?;
        state.serialize_field(&self.encoded)?;
        state.serialize_field(&self.latency_ms)?;
        state.serialize_field(&self.artifact)?;
        state.end()
    }
}

impl<'de> Deserialize<'de> for ProofBundle {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: foundation_serialization::serde::Deserializer<'de>,
    {
        deserializer.deserialize_tuple_struct("ProofBundle", 10, ProofBundleVisitor)
    }
}

struct ProofBundleVisitor;

impl<'de> Visitor<'de> for ProofBundleVisitor {
    type Value = ProofBundle;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("proof bundle tuple representation")
    }

    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let backend_tag: u8 = expect_seq_field(&mut seq, "backend")?;
        let circuit_hash: [u8; 32] = expect_seq_field(&mut seq, "circuit_hash")?;
        let program_commitment: [u8; 32] = expect_seq_field(&mut seq, "program_commitment")?;
        let output_commitment: [u8; 32] = expect_seq_field(&mut seq, "output_commitment")?;
        let witness_commitment: [u8; 32] = expect_seq_field(&mut seq, "witness_commitment")?;
        let public_inputs: Vec<[u8; 32]> = expect_seq_field(&mut seq, "public_inputs")?;
        let aux_assignments: Vec<[u8; 32]> = expect_seq_field(&mut seq, "aux_assignments")?;
        let encoded: Vec<u8> = expect_seq_field(&mut seq, "encoded")?;
        let latency_ms: u64 = expect_seq_field(&mut seq, "latency_ms")?;
        let artifact: CircuitArtifact = expect_seq_field(&mut seq, "artifact")?;
        let backend = SnarkBackend::from_tag(backend_tag).map_err(de::Error::custom)?;
        Ok(ProofBundle {
            backend,
            circuit_hash,
            program_commitment,
            output_commitment,
            witness_commitment,
            public_inputs,
            aux_assignments,
            encoded,
            latency_ms,
            artifact,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CircuitArtifact {
    pub circuit_hash: [u8; 32],
    pub wasm_hash: [u8; 32],
    pub generated_at: u64,
}

impl CircuitArtifact {
    pub fn new(circuit_hash: [u8; 32], wasm_hash: [u8; 32]) -> Self {
        Self {
            circuit_hash,
            wasm_hash,
            generated_at: now_ts(),
        }
    }
}

impl Serialize for CircuitArtifact {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: foundation_serialization::serde::Serializer,
    {
        let mut state = serializer.serialize_tuple_struct("CircuitArtifact", 3)?;
        state.serialize_field(&self.circuit_hash)?;
        state.serialize_field(&self.wasm_hash)?;
        state.serialize_field(&self.generated_at)?;
        state.end()
    }
}

impl<'de> Deserialize<'de> for CircuitArtifact {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: foundation_serialization::serde::Deserializer<'de>,
    {
        deserializer.deserialize_tuple_struct("CircuitArtifact", 3, CircuitArtifactVisitor)
    }
}

struct CircuitArtifactVisitor;

impl<'de> Visitor<'de> for CircuitArtifactVisitor {
    type Value = CircuitArtifact;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("circuit artifact tuple representation")
    }

    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let circuit_hash: [u8; 32] = expect_seq_field(&mut seq, "artifact.circuit_hash")?;
        let wasm_hash: [u8; 32] = expect_seq_field(&mut seq, "artifact.wasm_hash")?;
        let generated_at: u64 = expect_seq_field(&mut seq, "artifact.generated_at")?;
        Ok(CircuitArtifact {
            circuit_hash,
            wasm_hash,
            generated_at,
        })
    }
}

/// Compile a WASM workload, caching the resulting proving and verification keys.
pub fn compile_wasm(wasm: &[u8]) -> Result<Vec<u8>, SnarkError> {
    let compiled = compile_circuit(wasm)?;
    let artifact = CircuitArtifact::new(compiled.digest, digest(wasm));
    binary::encode(&artifact).map_err(|e| SnarkError::Encoding(e.to_string()))
}

/// Generate a proof for the workload using the preferred backend.
pub fn prove(wasm: &[u8], output: &[u8]) -> Result<ProofBundle, SnarkError> {
    let backend = default_backend();
    match prove_with_backend(wasm, output, backend) {
        Err(SnarkError::GpuUnavailable) if backend == SnarkBackend::Gpu => {
            prove_with_backend(wasm, output, SnarkBackend::Cpu)
        }
        other => other,
    }
}

/// Generate a proof using the requested backend.
pub fn prove_with_backend(
    wasm: &[u8],
    output: &[u8],
    backend: SnarkBackend,
) -> Result<ProofBundle, SnarkError> {
    eprintln!(
        "[SNARK] prove_with_backend ENTER (backend={:?}, wasm_len={})",
        backend,
        wasm.len()
    );

    eprintln!("[SNARK] Calling compile_circuit...");
    let compiled = compile_circuit(wasm)?;
    eprintln!("[SNARK] compile_circuit DONE");

    eprintln!("[SNARK] Deriving circuit inputs...");
    let inputs = CircuitInputs::derive(wasm, output);
    eprintln!("[SNARK] Circuit inputs derived");

    let artifact = CircuitArtifact::new(compiled.digest, inputs.wasm_hash);
    let circuit = ProgramCircuit::new(
        inputs.program_fe.clone(),
        inputs.output_fe.clone(),
        inputs.witness_fe.clone(),
    );
    eprintln!("[SNARK] Circuit created, calling run_prover...");
    let start = Instant::now();
    let proof = match run_prover(&compiled.params, circuit.clone(), backend) {
        Ok(proof) => proof,
        Err(err) => {
            eprintln!("[SNARK] run_prover FAILED: {:?}", err);
            #[cfg(feature = "telemetry")]
            record_prover_failure(backend);
            return Err(err);
        }
    };
    eprintln!("[SNARK] run_prover SUCCESS");
    let elapsed = start.elapsed();
    let latency_ms = u64::try_from(elapsed.as_millis())
        .unwrap_or(u64::MAX)
        .max(1);
    #[cfg(feature = "telemetry")]
    telemetry::sampled_observe_vec(
        &telemetry::SNARK_PROVER_LATENCY_SECONDS,
        &[backend.as_label()],
        elapsed.as_secs_f64(),
    );
    let (proof_inputs, proof_aux) = proof.inner();
    let public_inputs = proof_inputs.iter().map(field_to_bytes).collect::<Vec<_>>();
    let aux_assignments = proof_aux.iter().map(field_to_bytes).collect::<Vec<_>>();
    let encoded = encode_proof(backend, &public_inputs, &aux_assignments)?;
    Ok(ProofBundle {
        backend,
        circuit_hash: compiled.digest,
        program_commitment: inputs.wasm_hash,
        output_commitment: inputs.output_hash,
        witness_commitment: inputs.witness_hash,
        public_inputs,
        aux_assignments,
        encoded,
        latency_ms,
        artifact,
    })
}

/// Verify a proof against the provided workload and output commitment.
pub fn verify(bundle: &ProofBundle, wasm: &[u8], output: &[u8]) -> Result<bool, SnarkError> {
    let compiled = compile_circuit(wasm)?;
    if compiled.digest != bundle.circuit_hash {
        return Ok(false);
    }
    let inputs = CircuitInputs::derive(wasm, output);
    if bundle.program_commitment != inputs.wasm_hash
        || bundle.output_commitment != inputs.output_hash
        || bundle.witness_commitment != inputs.witness_hash
    {
        return Ok(false);
    }
    let proof = proof_from_bundle(bundle);
    #[cfg(feature = "telemetry")]
    let start = Instant::now();
    let verify_result = Groth16Bn256::verify(
        &compiled.verifier,
        &proof,
        &[inputs.program_fe.clone(), inputs.output_fe.clone()],
    );
    #[cfg(feature = "telemetry")]
    crate::telemetry::receipts::record_proof_verification_latency(start.elapsed());
    verify_result.map_err(SnarkError::from)
}

fn run_prover(
    params: &Parameters,
    circuit: ProgramCircuit,
    backend: SnarkBackend,
) -> Result<Proof, SnarkError> {
    eprintln!("[SNARK] run_prover ENTER (backend={:?})", backend);
    match backend {
        SnarkBackend::Cpu => {
            eprintln!("[SNARK] Calling Groth16Bn256::prove (CPU)...");
            let result = Groth16Bn256::prove(params, circuit, &mut ()).map_err(Into::into);
            eprintln!("[SNARK] Groth16Bn256::prove returned");
            result
        }
        SnarkBackend::Gpu => {
            eprintln!("[SNARK] Calling gpu_prove...");
            let result = gpu_prove(params, circuit);
            eprintln!("[SNARK] gpu_prove returned");
            result
        }
    }
}

#[cfg(feature = "gpu")]
fn gpu_prove(params: &Parameters, circuit: ProgramCircuit) -> Result<Proof, SnarkError> {
    Groth16Bn256::prove_gpu(params, circuit, &mut ()).map_err(Into::into)
}

#[cfg(not(feature = "gpu"))]
fn gpu_prove(_params: &Parameters, _circuit: ProgramCircuit) -> Result<Proof, SnarkError> {
    Err(SnarkError::GpuUnavailable)
}

fn compile_circuit(wasm: &[u8]) -> Result<Arc<CompiledCircuit>, SnarkError> {
    eprintln!("[SNARK] compile_circuit ENTER");
    let digest = digest(wasm);
    eprintln!("[SNARK] Digest computed, checking cache...");

    eprintln!("[SNARK] Acquiring cache lock...");
    if let Some(compiled) = CIRCUIT_CACHE.guard().get(&digest) {
        eprintln!("[SNARK] Cache HIT - returning cached circuit");
        return Ok(compiled.clone());
    }
    eprintln!("[SNARK] Cache MISS - need to setup circuit");

    eprintln!("[SNARK] Calling Groth16Bn256::setup...");
    let params = Arc::new(Groth16Bn256::setup(ProgramCircuit::blank(), &mut ())?);
    eprintln!("[SNARK] setup DONE, preparing verifying key...");

    let verifier = Arc::new(Groth16Bn256::prepare_verifying_key(&params));
    eprintln!("[SNARK] Verifying key prepared, caching entry...");

    let entry = Arc::new(CompiledCircuit {
        digest,
        params,
        verifier,
    });
    CIRCUIT_CACHE.guard().insert(digest, entry.clone());
    eprintln!("[SNARK] compile_circuit complete");
    Ok(entry)
}

fn proof_from_bundle(bundle: &ProofBundle) -> Proof {
    let public_inputs = bundle.public_inputs.iter().map(field_from_bytes).collect();
    let aux_assignments = bundle
        .aux_assignments
        .iter()
        .map(field_from_bytes)
        .collect();
    Proof::from_components(public_inputs, aux_assignments)
}

fn encode_proof(
    backend: SnarkBackend,
    public_inputs: &[[u8; 32]],
    aux_assignments: &[[u8; 32]],
) -> Result<Vec<u8>, SnarkError> {
    let mut payload =
        Vec::with_capacity(1 + 4 + public_inputs.len() * 32 + 4 + aux_assignments.len() * 32);
    payload.push(backend.tag());
    write_len(&mut payload, public_inputs.len())?;
    for input in public_inputs {
        payload.extend_from_slice(input);
    }
    write_len(&mut payload, aux_assignments.len())?;
    for assignment in aux_assignments {
        payload.extend_from_slice(assignment);
    }
    Ok(payload)
}

fn decode_proof(bytes: &[u8]) -> Result<EncodedProof, SnarkError> {
    if bytes.is_empty() {
        return Err(SnarkError::Encoding(
            "encoded proof missing discriminator".into(),
        ));
    }
    let mut cursor = 0usize;
    let backend = SnarkBackend::from_tag(bytes[cursor])?;
    cursor += 1;
    let (public_len, next) = read_len(bytes, cursor)?;
    cursor = next;
    let public_inputs = read_field_elements(bytes, &mut cursor, public_len)?;
    let (aux_len, next) = read_len(bytes, cursor)?;
    cursor = next;
    let aux_assignments = read_field_elements(bytes, &mut cursor, aux_len)?;
    if cursor != bytes.len() {
        return Err(SnarkError::Encoding(
            "trailing bytes in encoded proof".into(),
        ));
    }
    Ok(EncodedProof {
        backend,
        public_inputs,
        aux_assignments,
    })
}

struct EncodedProof {
    backend: SnarkBackend,
    public_inputs: Vec<[u8; 32]>,
    aux_assignments: Vec<[u8; 32]>,
}

fn expect_seq_field<'de, A, T>(seq: &mut A, field: &str) -> Result<T, A::Error>
where
    A: SeqAccess<'de>,
    T: Deserialize<'de>,
{
    seq.next_element()?.ok_or_else(|| {
        de::Error::custom(format!(
            "missing {field} while decoding proof bundle payload"
        ))
    })
}

fn read_len(bytes: &[u8], cursor: usize) -> Result<(usize, usize), SnarkError> {
    if bytes.len() < cursor + 4 {
        return Err(SnarkError::Encoding(
            "encoded proof missing length prefix".into(),
        ));
    }
    let value = u32::from_le_bytes(bytes[cursor..cursor + 4].try_into().expect("slice bounds"));
    Ok((value as usize, cursor + 4))
}

fn read_field_elements(
    bytes: &[u8],
    cursor: &mut usize,
    count: usize,
) -> Result<Vec<[u8; 32]>, SnarkError> {
    let byte_len = count
        .checked_mul(32)
        .ok_or_else(|| SnarkError::Encoding("encoded proof length overflow".into()))?;
    if bytes.len() < *cursor + byte_len {
        return Err(SnarkError::Encoding(
            "encoded proof truncated field assignments".into(),
        ));
    }
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        let mut field = [0u8; 32];
        field.copy_from_slice(&bytes[*cursor..*cursor + 32]);
        *cursor += 32;
        out.push(field);
    }
    Ok(out)
}

fn write_len(buffer: &mut Vec<u8>, len: usize) -> Result<(), SnarkError> {
    let value = u32::try_from(len)
        .map_err(|_| SnarkError::Encoding("field vector exceeds u32::MAX entries".into()))?;
    buffer.extend_from_slice(&value.to_le_bytes());
    Ok(())
}

fn default_backend() -> SnarkBackend {
    if let Ok(value) = std::env::var(SNARK_BACKEND_ENV) {
        if value.eq_ignore_ascii_case("gpu") {
            return SnarkBackend::Gpu;
        }
        if value.eq_ignore_ascii_case("cpu") {
            return SnarkBackend::Cpu;
        }
    }
    if gpu_available() {
        SnarkBackend::Gpu
    } else {
        SnarkBackend::Cpu
    }
}

fn gpu_available() -> bool {
    cfg!(feature = "gpu") || std::env::var("TB_GPU_MODEL").is_ok()
}

fn digest(input: &[u8]) -> [u8; 32] {
    let mut h = Hasher::new();
    h.update(input);
    *h.finalize().as_bytes()
}

fn field_from_bytes(bytes: &[u8; 32]) -> FieldElement {
    FieldElement::from(BigUint::from_bytes_be(bytes))
}

fn field_to_bytes(field: &FieldElement) -> [u8; 32] {
    let mut out = [0u8; 32];
    let bytes = field.clone_inner().to_bytes_be();
    let offset = out.len().saturating_sub(bytes.len());
    out[offset..].copy_from_slice(&bytes);
    out
}

fn now_ts() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(feature = "telemetry")]
fn record_prover_failure(backend: SnarkBackend) {
    telemetry::SNARK_PROVER_FAILURE_TOTAL
        .ensure_handle_for_label_values(&[backend.as_label()])
        .expect(crate::telemetry::LABEL_REGISTRATION_ERR)
        .inc();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compute_market::workloads;

    #[test]
    fn round_trip_proof_and_verify() {
        let wasm = b"demo-wasm";
        let output = [42u8; 32];
        let proof = prove_with_backend(wasm, &output, SnarkBackend::Cpu).unwrap();
        assert!(verify(&proof, wasm, &output).unwrap());
        assert!(proof.self_check());
        assert_eq!(proof.circuit_hash, digest(wasm));
    }

    #[test]
    fn invalid_output_rejected() {
        let wasm = b"demo-wasm";
        let output = [7u8; 32];
        let mut proof = prove_with_backend(wasm, &output, SnarkBackend::Cpu).unwrap();
        proof.output_commitment = [0u8; 32];
        assert!(!verify(&proof, wasm, &output).unwrap());
    }

    #[test]
    fn proof_bundle_binary_round_trip() {
        let wasm = b"binary-round-trip";
        let output = workloads::snark::run(wasm);
        let proof = prove_with_backend(wasm, &output, SnarkBackend::Cpu).unwrap();
        let blob = binary::encode(&proof).expect("encode bundle");
        let decoded: ProofBundle = binary::decode(&blob).expect("decode bundle");
        assert_eq!(decoded.backend, proof.backend);
        assert_eq!(decoded.circuit_hash, proof.circuit_hash);
        assert!(verify(&decoded, wasm, &output).unwrap());
    }
}
