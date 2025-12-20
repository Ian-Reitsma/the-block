#![allow(clippy::module_name_repetitions)]

use core::fmt;
use std::ops::{Add, AddAssign, Mul, Sub};
use std::str::FromStr;

use foundation_bigint::BigUint;
use foundation_lazy::sync::Lazy;
#[cfg(feature = "gpu")]
use std::{sync::Arc, thread};
use thiserror::Error;

const MODULUS_DEC: &str =
    "21888242871839275222246405745257275088548364400416034343698204186575808495617";

static MODULUS: Lazy<BigUint> = Lazy::new(|| {
    BigUint::parse_bytes(MODULUS_DEC.as_bytes(), 10)
        .expect("valid BN254 modulus should parse from decimal")
});

fn modulus() -> &'static BigUint {
    &MODULUS
}

#[derive(Clone, PartialEq, Eq)]
pub struct FieldElement(BigUint);

impl FieldElement {
    pub fn zero() -> Self {
        Self(BigUint::zero())
    }

    pub fn one() -> Self {
        Self(BigUint::one())
    }

    pub fn from_u64(value: u64) -> Self {
        Self(BigUint::from(value)).reduce()
    }

    pub fn from_decimal_str(value: &str) -> Result<Self, Groth16Error> {
        BigUint::parse_bytes(value.as_bytes(), 10)
            .ok_or_else(|| Groth16Error::FieldConversion(format!("invalid field element: {value}")))
            .map(Self)
            .map(Self::reduce_inner)
    }

    pub fn inner(&self) -> &BigUint {
        &self.0
    }

    pub fn clone_inner(&self) -> BigUint {
        self.0.clone()
    }

    fn reduce(self) -> Self {
        Self::reduce_inner(self)
    }

    fn reduce_inner(value: Self) -> Self {
        let modulus = modulus();
        let reduced = value.0 % modulus;
        Self(reduced)
    }
}

impl fmt::Debug for FieldElement {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "FieldElement({})", self.0)
    }
}

impl FromStr for FieldElement {
    type Err = Groth16Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::from_decimal_str(value)
    }
}

impl From<BigUint> for FieldElement {
    fn from(value: BigUint) -> Self {
        Self(value).reduce()
    }
}

impl From<FieldElement> for BigUint {
    fn from(value: FieldElement) -> Self {
        value.0
    }
}

impl Add for FieldElement {
    type Output = FieldElement;

    fn add(self, rhs: Self) -> Self::Output {
        let modulus = modulus();
        let mut sum = self.0 + rhs.0;
        sum %= modulus;
        FieldElement(sum)
    }
}

impl Sub for FieldElement {
    type Output = FieldElement;

    fn sub(self, rhs: Self) -> Self::Output {
        let modulus = modulus();
        if self.0 >= rhs.0 {
            FieldElement(self.0 - rhs.0)
        } else {
            FieldElement(&self.0 + modulus - rhs.0)
        }
    }
}

impl Mul for FieldElement {
    type Output = FieldElement;

    fn mul(self, rhs: Self) -> Self::Output {
        let modulus = modulus();
        FieldElement((self.0 * rhs.0) % modulus)
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct Variable(pub(crate) usize);

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
struct ConstOne;

#[derive(Clone, Debug)]
struct LinearCombination {
    terms: Vec<(Variable, FieldElement)>,
    constant: FieldElement,
}

impl LinearCombination {
    fn new() -> Self {
        Self {
            terms: Vec::new(),
            constant: FieldElement::zero(),
        }
    }

    fn evaluate(&self, assignments: &[FieldElement]) -> FieldElement {
        let mut acc = self.constant.clone();
        for (var, coeff) in &self.terms {
            let idx = var.0;
            let value = assignments[idx].clone();
            acc = acc + coeff.clone() * value;
        }
        acc
    }
}

#[derive(Clone, Debug)]
struct Constraint {
    a: LinearCombination,
    b: LinearCombination,
    c: LinearCombination,
}

#[derive(Default, Clone, Debug)]
struct Shape {
    next_index: usize,
    input_indices: Vec<usize>,
    aux_indices: Vec<usize>,
    constraints: Vec<Constraint>,
}

#[derive(Clone, Debug)]
pub struct Parameters {
    shape: Shape,
}

#[derive(Clone, Debug)]
pub struct Proof {
    public_inputs: Vec<FieldElement>,
    aux_assignments: Vec<FieldElement>,
}

#[derive(Clone, Debug)]
pub struct PreparedVerifyingKey {
    shape: Shape,
}

#[derive(Debug, Error)]
pub enum SynthesisError {
    #[error("assignment missing")]
    AssignmentMissing,
    #[error("malformed constraint system")]
    MalformedConstraintSystem,
}

#[derive(Debug, Error)]
pub enum Groth16Error {
    #[error("synthesis failed: {0}")]
    Synthesis(#[from] SynthesisError),
    #[error("proof rejected: {0}")]
    Proof(String),
    #[error("field conversion failed: {0}")]
    FieldConversion(String),
}

pub trait ConstraintSystem {
    fn alloc<F, V>(&mut self, annotation: F, value: V) -> Result<Variable, SynthesisError>
    where
        F: FnOnce() -> String,
        V: FnOnce() -> Result<FieldElement, SynthesisError>;

    fn alloc_input<F, V>(&mut self, annotation: F, value: V) -> Result<Variable, SynthesisError>
    where
        F: FnOnce() -> String,
        V: FnOnce() -> Result<FieldElement, SynthesisError>;

    fn enforce<F>(
        &mut self,
        annotation: F,
        a: impl FnOnce(LinearCombinationBuilder) -> LinearCombinationBuilder,
        b: impl FnOnce(LinearCombinationBuilder) -> LinearCombinationBuilder,
        c: impl FnOnce(LinearCombinationBuilder) -> LinearCombinationBuilder,
    ) where
        F: FnOnce() -> String;

    fn one() -> Variable {
        Variable(0)
    }
}

#[derive(Clone, Debug)]
pub struct LinearCombinationBuilder(LinearCombination);

impl LinearCombinationBuilder {
    fn new() -> Self {
        Self(LinearCombination::new())
    }

    fn build(self) -> LinearCombination {
        self.0
    }
}

impl Add<Variable> for LinearCombinationBuilder {
    type Output = Self;

    fn add(mut self, rhs: Variable) -> Self::Output {
        self.0.terms.push((rhs, FieldElement::one()));
        self
    }
}

impl AddAssign<Variable> for LinearCombinationBuilder {
    fn add_assign(&mut self, rhs: Variable) {
        self.0.terms.push((rhs, FieldElement::one()));
    }
}

impl Add<(FieldElement, Variable)> for LinearCombinationBuilder {
    type Output = Self;

    fn add(mut self, rhs: (FieldElement, Variable)) -> Self::Output {
        self.0.terms.push((rhs.1, rhs.0));
        self
    }
}

impl AddAssign<(FieldElement, Variable)> for LinearCombinationBuilder {
    fn add_assign(&mut self, rhs: (FieldElement, Variable)) {
        self.0.terms.push((rhs.1, rhs.0));
    }
}

impl Add<ConstOne> for LinearCombinationBuilder {
    type Output = Self;

    fn add(mut self, _rhs: ConstOne) -> Self::Output {
        self.0.constant = self.0.constant.clone() + FieldElement::one();
        self
    }
}

impl AddAssign<ConstOne> for LinearCombinationBuilder {
    fn add_assign(&mut self, _rhs: ConstOne) {
        let new_const = self.0.constant.clone() + FieldElement::one();
        self.0.constant = new_const;
    }
}

impl Add<(FieldElement, ConstOne)> for LinearCombinationBuilder {
    type Output = Self;

    fn add(mut self, rhs: (FieldElement, ConstOne)) -> Self::Output {
        self.0.constant = self.0.constant.clone() + rhs.0;
        self
    }
}

impl AddAssign<(FieldElement, ConstOne)> for LinearCombinationBuilder {
    fn add_assign(&mut self, rhs: (FieldElement, ConstOne)) {
        let new_const = self.0.constant.clone() + rhs.0;
        self.0.constant = new_const;
    }
}

struct ShapeCS {
    shape: Shape,
}

impl ShapeCS {
    fn new() -> Self {
        Self {
            shape: Shape {
                next_index: 0,
                input_indices: Vec::new(),
                aux_indices: Vec::new(),
                constraints: Vec::new(),
            },
        }
    }
}

impl ConstraintSystem for ShapeCS {
    fn alloc<F, V>(&mut self, _annotation: F, _value: V) -> Result<Variable, SynthesisError>
    where
        F: FnOnce() -> String,
        V: FnOnce() -> Result<FieldElement, SynthesisError>,
    {
        self.shape.next_index += 1;
        let index = self.shape.next_index;
        self.shape.aux_indices.push(index);
        Ok(Variable(index))
    }

    fn alloc_input<F, V>(&mut self, _annotation: F, _value: V) -> Result<Variable, SynthesisError>
    where
        F: FnOnce() -> String,
        V: FnOnce() -> Result<FieldElement, SynthesisError>,
    {
        self.shape.next_index += 1;
        let index = self.shape.next_index;
        self.shape.input_indices.push(index);
        Ok(Variable(index))
    }

    fn enforce<F>(
        &mut self,
        _annotation: F,
        a: impl FnOnce(LinearCombinationBuilder) -> LinearCombinationBuilder,
        b: impl FnOnce(LinearCombinationBuilder) -> LinearCombinationBuilder,
        c: impl FnOnce(LinearCombinationBuilder) -> LinearCombinationBuilder,
    ) where
        F: FnOnce() -> String,
    {
        let lc_a = a(LinearCombinationBuilder::new()).build();
        let lc_b = b(LinearCombinationBuilder::new()).build();
        let lc_c = c(LinearCombinationBuilder::new()).build();
        self.shape.constraints.push(Constraint {
            a: lc_a,
            b: lc_b,
            c: lc_c,
        });
    }
}

struct AssignmentCS<'a> {
    shape: &'a Shape,
    values: Vec<FieldElement>,
    public_inputs: Vec<FieldElement>,
    aux_inputs: Vec<FieldElement>,
    next_input: usize,
    next_aux: usize,
    constraints: Vec<Constraint>,
}

impl<'a> AssignmentCS<'a> {
    fn new(shape: &'a Shape) -> Self {
        let mut values = vec![FieldElement::zero(); shape.next_index + 1];
        values[0] = FieldElement::one();
        Self {
            shape,
            values,
            public_inputs: Vec::new(),
            aux_inputs: Vec::new(),
            next_input: 0,
            next_aux: 0,
            constraints: Vec::new(),
        }
    }

    fn assignments(&self) -> Vec<FieldElement> {
        self.values.clone()
    }
}

impl ConstraintSystem for AssignmentCS<'_> {
    fn alloc<F, V>(&mut self, _annotation: F, value: V) -> Result<Variable, SynthesisError>
    where
        F: FnOnce() -> String,
        V: FnOnce() -> Result<FieldElement, SynthesisError>,
    {
        let index = self.shape.aux_indices[self.next_aux];
        self.next_aux += 1;
        let val = value()?;
        self.values[index] = val.clone();
        self.aux_inputs.push(val);
        Ok(Variable(index))
    }

    fn alloc_input<F, V>(&mut self, _annotation: F, value: V) -> Result<Variable, SynthesisError>
    where
        F: FnOnce() -> String,
        V: FnOnce() -> Result<FieldElement, SynthesisError>,
    {
        let index = self.shape.input_indices[self.next_input];
        self.next_input += 1;
        let val = value()?;
        self.values[index] = val.clone();
        self.public_inputs.push(val);
        Ok(Variable(index))
    }

    fn enforce<F>(
        &mut self,
        _annotation: F,
        a: impl FnOnce(LinearCombinationBuilder) -> LinearCombinationBuilder,
        b: impl FnOnce(LinearCombinationBuilder) -> LinearCombinationBuilder,
        c: impl FnOnce(LinearCombinationBuilder) -> LinearCombinationBuilder,
    ) where
        F: FnOnce() -> String,
    {
        let lc_a = a(LinearCombinationBuilder::new()).build();
        let lc_b = b(LinearCombinationBuilder::new()).build();
        let lc_c = c(LinearCombinationBuilder::new()).build();
        self.constraints.push(Constraint {
            a: lc_a,
            b: lc_b,
            c: lc_c,
        });
    }
}

pub trait Circuit: Clone {
    fn synthesize<CS: ConstraintSystem>(self, cs: &mut CS) -> Result<(), SynthesisError>;
}

pub trait BellmanCircuit<E>: Circuit {}

impl<T, E> BellmanCircuit<E> for T where T: Circuit {}

pub trait BellmanConstraintSystem<E>: ConstraintSystem {}

impl<T, E> BellmanConstraintSystem<E> for T where T: ConstraintSystem {}

#[derive(Clone, Debug)]
pub struct Bn256;

pub struct Groth16Bn256;

impl Groth16Bn256 {
    pub fn setup<C: Circuit, R>(_circuit: C, _rng: &mut R) -> Result<Parameters, Groth16Error> {
        let mut cs = ShapeCS::new();
        _circuit
            .clone()
            .synthesize(&mut cs)
            .map_err(Groth16Error::Synthesis)?;
        Ok(Parameters { shape: cs.shape })
    }

    pub fn prove<C: Circuit, R>(
        params: &Parameters,
        circuit: C,
        _rng: &mut R,
    ) -> Result<Proof, Groth16Error> {
        let mut cs = AssignmentCS::new(&params.shape);
        circuit
            .synthesize(&mut cs)
            .map_err(Groth16Error::Synthesis)?;

        if cs.public_inputs.len() != params.shape.input_indices.len() {
            return Err(Groth16Error::Proof("public input length mismatch".into()));
        }
        if cs.aux_inputs.len() != params.shape.aux_indices.len() {
            return Err(Groth16Error::Proof(
                "auxiliary input length mismatch".into(),
            ));
        }

        let assignments = cs.assignments();
        for constraint in &cs.constraints {
            let a = constraint.a.evaluate(&assignments);
            let b = constraint.b.evaluate(&assignments);
            let c = constraint.c.evaluate(&assignments);
            if a.clone() * b.clone() != c {
                return Err(Groth16Error::Proof("constraint unsatisfied".into()));
            }
        }

        Ok(Proof {
            public_inputs: cs.public_inputs,
            aux_assignments: cs.aux_inputs,
        })
    }

    #[cfg(feature = "gpu")]
    pub fn prove_gpu<C: Circuit, R>(
        params: &Parameters,
        circuit: C,
        _rng: &mut R,
    ) -> Result<Proof, Groth16Error> {
        let mut cs = AssignmentCS::new(&params.shape);
        circuit
            .synthesize(&mut cs)
            .map_err(Groth16Error::Synthesis)?;

        if cs.public_inputs.len() != params.shape.input_indices.len() {
            return Err(Groth16Error::Proof("public input length mismatch".into()));
        }
        if cs.aux_inputs.len() != params.shape.aux_indices.len() {
            return Err(Groth16Error::Proof(
                "auxiliary input length mismatch".into(),
            ));
        }

        let assignments = cs.assignments();
        verify_constraints_parallel(&cs.constraints, &assignments)?;

        Ok(Proof {
            public_inputs: cs.public_inputs,
            aux_assignments: cs.aux_inputs,
        })
    }

    pub fn prepare_verifying_key(params: &Parameters) -> PreparedVerifyingKey {
        PreparedVerifyingKey {
            shape: params.shape.clone(),
        }
    }

    pub fn verify(
        pvk: &PreparedVerifyingKey,
        proof: &Proof,
        inputs: &[FieldElement],
    ) -> Result<bool, Groth16Error> {
        if inputs.len() != pvk.shape.input_indices.len() {
            return Err(Groth16Error::Proof("public input length mismatch".into()));
        }
        if inputs != proof.public_inputs.as_slice() {
            return Ok(false);
        }

        if proof.aux_assignments.len() != pvk.shape.aux_indices.len() {
            return Err(Groth16Error::Proof(
                "auxiliary input length mismatch".into(),
            ));
        }

        let mut assignments = vec![FieldElement::zero(); pvk.shape.next_index + 1];
        assignments[0] = FieldElement::one();
        for (idx, value) in pvk
            .shape
            .input_indices
            .iter()
            .zip(proof.public_inputs.iter())
        {
            assignments[*idx] = value.clone();
        }
        for (idx, value) in pvk
            .shape
            .aux_indices
            .iter()
            .zip(proof.aux_assignments.iter())
        {
            assignments[*idx] = value.clone();
        }

        for constraint in &pvk.shape.constraints {
            let a = constraint.a.evaluate(&assignments);
            let b = constraint.b.evaluate(&assignments);
            let c = constraint.c.evaluate(&assignments);
            if a.clone() * b.clone() != c {
                return Ok(false);
            }
        }

        Ok(true)
    }
}

impl Proof {
    pub fn inner(&self) -> (&[FieldElement], &[FieldElement]) {
        (&self.public_inputs, &self.aux_assignments)
    }

    pub fn from_components(
        public_inputs: Vec<FieldElement>,
        aux_assignments: Vec<FieldElement>,
    ) -> Self {
        Self {
            public_inputs,
            aux_assignments,
        }
    }
}

#[cfg(feature = "gpu")]
fn verify_constraints_parallel(
    constraints: &[Constraint],
    assignments: &[FieldElement],
) -> Result<(), Groth16Error> {
    if constraints.is_empty() {
        return Ok(());
    }
    let workers = thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
        .max(1);
    let chunk_size = constraints.len().div_ceil(workers);
    let assignments = Arc::new(assignments.to_vec());
    let shared = Arc::new(constraints.to_vec());
    let mut handles = Vec::new();
    for (idx, start) in (0..shared.len()).step_by(chunk_size).enumerate() {
        let assignments = Arc::clone(&assignments);
        let constraints = Arc::clone(&shared);
        let end = ((idx + 1) * chunk_size).min(constraints.len());
        handles.push(thread::spawn(move || {
            for constraint in &constraints[start..end] {
                if !constraint_satisfied(constraint, &assignments) {
                    return Err(Groth16Error::Proof("constraint unsatisfied".into()));
                }
            }
            Ok(())
        }));
    }
    for handle in handles {
        handle
            .join()
            .map_err(|_| Groth16Error::Proof("constraint worker panicked".into()))??;
    }
    Ok(())
}

#[cfg(feature = "gpu")]
fn constraint_satisfied(constraint: &Constraint, assignments: &[FieldElement]) -> bool {
    let a = constraint.a.evaluate(assignments);
    let b = constraint.b.evaluate(assignments);
    let c = constraint.c.evaluate(assignments);
    a.clone() * b.clone() == c
}
