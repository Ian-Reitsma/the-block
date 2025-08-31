#![forbid(unsafe_code)]

#[derive(Debug, Clone)]
pub enum Op {
    Push(Vec<u8>),
    Dup,
    Equal,
    CheckSig,
    True,
}

#[derive(Debug, Clone)]
pub struct Script(pub Vec<Op>);

pub fn execute(sig: &Script, pk: &Script) -> Result<Vec<u8>, String> {
    let mut stack: Vec<Vec<u8>> = Vec::new();
    for op in sig.0.iter().chain(pk.0.iter()) {
        match op {
            Op::Push(data) => stack.push(data.clone()),
            Op::Dup => {
                let v = stack.last().cloned().ok_or("stack underflow")?;
                stack.push(v);
            }
            Op::Equal => {
                let a = stack.pop().ok_or("stack underflow")?;
                let b = stack.pop().ok_or("stack underflow")?;
                stack.push(if a == b { vec![1] } else { vec![0] });
            }
            Op::True => stack.push(vec![1]),
            Op::CheckSig => {
                let _sig = stack.pop().ok_or("stack underflow")?;
                let _pk = stack.pop().ok_or("stack underflow")?;
                // Placeholder signature check
                stack.push(vec![1]);
            }
        }
    }
    Ok(stack.into_iter().map(|v| v[0]).collect())
}
