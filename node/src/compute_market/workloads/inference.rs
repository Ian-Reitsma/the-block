use super::{blocktorch, hash_bytes, BlockTorchWorkloadMetadata, WorkloadRunOutput};
use crypto_suite::hashing::blake3::Hasher;
use foundation_serialization::{binary, Deserialize, Serialize};
use std::convert::TryInto;

const BLOCKTORCH_JOB_MAGIC: &[u8; 4] = b"BTRA";
const BLOCKTORCH_JOB_VERSION: u8 = 0;
const JOB_HEADER_LEN: usize = 4 + 1 + 32 + 4 + 4 + 4;

/// Describes the tensor layout + metadata emitted by the compute job.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(crate = "foundation_serialization::serde")]
pub struct InputTensorDescriptor {
    pub dtype: TensorDtype,
    pub input_shape: Vec<u32>,
    pub weight_shape: (u32, u32),
    #[serde(default)]
    pub strides: Vec<u32>,
    #[serde(default)]
    pub normalization: NormalizationPolicy,
    #[serde(default)]
    pub activation: Activation,
    #[serde(default)]
    pub attention_mask: Option<Vec<u8>>,
    #[serde(default)]
    pub padding_tokens: Option<Vec<u8>>,
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tensor_profile_epoch: Option<String>,
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub benchmark_commit: Option<String>,
}

impl Default for InputTensorDescriptor {
    fn default() -> Self {
        Self {
            dtype: TensorDtype::default(),
            input_shape: Vec::new(),
            weight_shape: (0, 0),
            strides: Vec::new(),
            normalization: NormalizationPolicy::default(),
            activation: Activation::default(),
            attention_mask: None,
            padding_tokens: None,
            tensor_profile_epoch: None,
            benchmark_commit: None,
        }
    }
}

impl InputTensorDescriptor {
    fn input_len(&self) -> usize {
        self.input_shape
            .iter()
            .copied()
            .map(|dim| dim as usize)
            .product()
    }

    fn weight_len(&self) -> usize {
        (self.weight_shape.0 as usize) * (self.weight_shape.1 as usize)
    }

    fn output_len(&self) -> usize {
        self.weight_shape.0 as usize
    }

    fn descriptor_bytes(&self) -> Vec<u8> {
        binary::encode(self).expect("descriptor serialization must succeed")
    }

    fn decode_input(&self, payload: &[u8]) -> Option<Vec<f32>> {
        match self.normalization {
            NormalizationPolicy::BytesToFloat => {
                if payload.len() != self.input_len() {
                    return None;
                }
                Some(payload.iter().map(|b| *b as f32 / 255.0).collect())
            }
            NormalizationPolicy::Identity => {
                if payload.len() % 4 != 0 {
                    return None;
                }
                let slices = payload.len() / 4;
                if slices != self.input_len() {
                    return None;
                }
                let mut output = Vec::with_capacity(slices);
                for chunk in payload.chunks_exact(4) {
                    let arr: [u8; 4] = chunk.try_into().ok()?;
                    output.push(f32::from_le_bytes(arr));
                }
                Some(output)
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(crate = "foundation_serialization::serde")]
pub enum TensorDtype {
    #[serde(rename = "F32")]
    F32,
}

impl Default for TensorDtype {
    fn default() -> Self {
        TensorDtype::F32
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(crate = "foundation_serialization::serde")]
pub enum NormalizationPolicy {
    #[serde(rename = "BYTES_TO_FLOAT")]
    BytesToFloat,
    #[serde(rename = "IDENTITY")]
    Identity,
}

impl Default for NormalizationPolicy {
    fn default() -> Self {
        NormalizationPolicy::BytesToFloat
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(crate = "foundation_serialization::serde")]
pub enum Activation {
    #[serde(rename = "LINEAR")]
    Linear,
    #[serde(rename = "RELU")]
    ReLU,
    #[serde(rename = "SOFTMAX")]
    Softmax,
}

impl Default for Activation {
    fn default() -> Self {
        Activation::Linear
    }
}

/// Payload for a BlockTorch inference job.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(crate = "foundation_serialization::serde")]
pub struct BlockTorchInference {
    pub artifact: Vec<u8>,
    pub input: Vec<u8>,
    pub descriptor: InputTensorDescriptor,
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub job_kernel_digest: Option<[u8; 32]>,
}

impl BlockTorchInference {
    pub fn new(artifact: Vec<u8>, input: Vec<u8>, descriptor: InputTensorDescriptor) -> Self {
        Self {
            artifact,
            input,
            descriptor,
            job_kernel_digest: None,
        }
    }

    pub fn from_job_blob(blob: &[u8]) -> Option<Self> {
        if blob.len() < JOB_HEADER_LEN {
            return None;
        }
        if &blob[0..4] != BLOCKTORCH_JOB_MAGIC {
            return None;
        }
        if blob[4] != BLOCKTORCH_JOB_VERSION {
            return None;
        }
        let kernel_digest: [u8; 32] = blob[5..37].try_into().ok()?;
        let artifact_len = read_u32_le(&blob[37..41])? as usize;
        let input_len = read_u32_le(&blob[41..45])? as usize;
        let descriptor_len = read_u32_le(&blob[45..49])? as usize;
        let mut offset = JOB_HEADER_LEN;
        let descriptor_end = offset.checked_add(descriptor_len)?;
        if descriptor_end > blob.len() {
            return None;
        }
        let descriptor_bytes = &blob[offset..descriptor_end];
        let descriptor = binary::decode::<InputTensorDescriptor>(descriptor_bytes).ok()?;
        offset = descriptor_end;
        let artifact_end = offset.checked_add(artifact_len)?;
        if artifact_end > blob.len() {
            return None;
        }
        let artifact = blob[offset..artifact_end].to_vec();
        offset = artifact_end;
        let input_end = offset.checked_add(input_len)?;
        if input_end > blob.len() {
            return None;
        }
        let input = blob[offset..input_end].to_vec();
        Some(Self {
            artifact,
            input,
            descriptor,
            job_kernel_digest: Some(kernel_digest),
        })
    }

    pub fn to_job_blob(&self) -> Option<Vec<u8>> {
        let descriptor_bytes = self.descriptor.descriptor_bytes();
        let artifact_len = self.artifact.len();
        let input_len = self.input.len();
        let descriptor_len = descriptor_bytes.len();
        if artifact_len > u32::MAX as usize
            || input_len > u32::MAX as usize
            || descriptor_len > u32::MAX as usize
        {
            return None;
        }
        let kernel_digest = self
            .job_kernel_digest
            .unwrap_or_else(blocktorch::kernel_bundle_digest);
        let mut blob =
            Vec::with_capacity(JOB_HEADER_LEN + artifact_len + input_len + descriptor_len);
        blob.extend_from_slice(BLOCKTORCH_JOB_MAGIC);
        blob.push(BLOCKTORCH_JOB_VERSION);
        blob.extend_from_slice(&kernel_digest);
        blob.extend_from_slice(&(artifact_len as u32).to_le_bytes());
        blob.extend_from_slice(&(input_len as u32).to_le_bytes());
        blob.extend_from_slice(&(descriptor_len as u32).to_le_bytes());
        blob.extend_from_slice(&descriptor_bytes);
        blob.extend_from_slice(&self.artifact);
        blob.extend_from_slice(&self.input);
        Some(blob)
    }

    fn descriptor_digest(&self) -> [u8; 32] {
        hash_bytes(&self.descriptor.descriptor_bytes())
    }

    fn benchmark_commit(&self) -> Option<String> {
        self.descriptor
            .benchmark_commit
            .clone()
            .or_else(blocktorch::runtime_benchmark_commit)
    }

    fn tensor_profile_epoch(&self) -> Option<String> {
        self.descriptor.tensor_profile_epoch.clone()
    }

    pub fn metadata(&self, output_digest: [u8; 32]) -> BlockTorchWorkloadMetadata {
        BlockTorchWorkloadMetadata {
            kernel_digest: blocktorch::kernel_bundle_digest(),
            descriptor_digest: self.descriptor_digest(),
            benchmark_commit: self.benchmark_commit(),
            tensor_profile_epoch: self.tensor_profile_epoch(),
            output_digest,
        }
    }
}

/// Execute the BlockTorch inference workload via deterministic CPU kernels.
pub fn run(payload: &BlockTorchInference) -> WorkloadRunOutput {
    let output_tensor = run_inference(payload);
    let output_digest = hash_output(&output_tensor);
    WorkloadRunOutput {
        output: output_digest,
        blocktorch: Some(payload.metadata(output_digest)),
    }
}

fn run_inference(payload: &BlockTorchInference) -> Vec<f32> {
    let output_len = payload.descriptor.output_len();
    if output_len == 0 {
        return Vec::new();
    }
    let mut output = vec![0.0f32; output_len];
    cpu_inference(payload, &mut output);
    output
}

fn cpu_inference(payload: &BlockTorchInference, output: &mut [f32]) {
    let descriptor = &payload.descriptor;
    let rows = descriptor.weight_shape.0 as usize;
    let cols = descriptor.weight_shape.1 as usize;
    let weight_len = descriptor.weight_len();
    if rows == 0 || cols == 0 || output.len() != rows {
        return;
    }

    let input = match descriptor.decode_input(&payload.input) {
        Some(mut values) => {
            if let Some(mask) = descriptor.attention_mask.as_ref() {
                if mask.len() == values.len() {
                    values
                        .iter_mut()
                        .zip(mask.iter())
                        .for_each(|(value, mask_val)| {
                            if *mask_val == 0 {
                                *value = 0.0;
                            }
                        });
                }
            }
            values
        }
        None => return,
    };

    if input.len() < cols {
        return;
    }

    let weights = match bytes_to_f32(&payload.artifact) {
        Some(values) => values,
        None => return,
    };

    if weights.len() < weight_len {
        return;
    }

    for r in 0..rows {
        let base = r * cols;
        let mut acc = 0.0f32;
        for c in 0..cols {
            acc += weights[base + c] * input[c];
        }
        output[r] = acc;
    }

    match descriptor.activation {
        Activation::ReLU => {
            for value in output.iter_mut() {
                if *value < 0.0 {
                    *value = 0.0;
                }
            }
        }
        Activation::Softmax => {
            let max = output.iter().copied().fold(f32::NEG_INFINITY, f32::max);
            let mut sum_exp = 0.0f32;
            for value in output.iter_mut() {
                *value = (*value - max).exp();
                sum_exp += *value;
            }
            if sum_exp != 0.0 {
                for value in output.iter_mut() {
                    *value /= sum_exp;
                }
            }
        }
        Activation::Linear => {}
    }
}

fn bytes_to_f32(bytes: &[u8]) -> Option<Vec<f32>> {
    if bytes.len() % 4 != 0 {
        return None;
    }
    let mut result = Vec::with_capacity(bytes.len() / 4);
    for chunk in bytes.chunks_exact(4) {
        let arr: [u8; 4] = chunk.try_into().ok()?;
        result.push(f32::from_le_bytes(arr));
    }
    Some(result)
}

fn hash_output(values: &[f32]) -> [u8; 32] {
    let mut h = Hasher::new();
    for value in values {
        h.update(&value.to_le_bytes());
    }
    *h.finalize().as_bytes()
}

fn read_u32_le(bytes: &[u8]) -> Option<u32> {
    if bytes.len() < 4 {
        return None;
    }
    let arr: [u8; 4] = bytes[..4].try_into().ok()?;
    Some(u32::from_le_bytes(arr))
}
