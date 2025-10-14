use foundation_bigint::BigUint;

fn dec(input: &str) -> BigUint {
    BigUint::parse_bytes(input.as_bytes(), 10).expect("invalid decimal literal")
}

fn hex(input: &str) -> BigUint {
    BigUint::parse_bytes(input.as_bytes(), 16).expect("invalid hex literal")
}

fn decimal_product(lhs: &str, rhs: &str) -> String {
    let lhs_digits: Vec<u32> = lhs.bytes().map(|b| (b - b'0') as u32).rev().collect();
    let rhs_digits: Vec<u32> = rhs.bytes().map(|b| (b - b'0') as u32).rev().collect();
    if lhs_digits.is_empty() || rhs_digits.is_empty() {
        return "0".to_string();
    }
    let mut output = vec![0u32; lhs_digits.len() + rhs_digits.len() + 1];
    for (i, &ld) in lhs_digits.iter().enumerate() {
        let mut carry = 0u32;
        for (j, &rd) in rhs_digits.iter().enumerate() {
            let idx = i + j;
            let total = output[idx] + ld * rd + carry;
            output[idx] = total % 10;
            carry = total / 10;
        }
        let mut idx = i + rhs_digits.len();
        while carry > 0 {
            let total = output[idx] + carry;
            output[idx] = total % 10;
            carry = total / 10;
            idx += 1;
        }
    }
    while output.len() > 1 && output.last() == Some(&0) {
        output.pop();
    }
    output
        .into_iter()
        .rev()
        .map(|d| char::from(b'0' + d as u8))
        .collect()
}

fn modpow_u128(mut base: u128, mut exp: u128, modulus: u128) -> u128 {
    if modulus == 0 {
        return 0;
    }
    base %= modulus;
    let mut acc = 1u128;
    while exp > 0 {
        if exp & 1 == 1 {
            acc = (acc * base) % modulus;
        }
        exp >>= 1;
        if exp == 0 {
            break;
        }
        base = (base * base) % modulus;
    }
    acc
}

#[test]
fn addition_matches_expected() {
    let lhs = dec("123456789012345678901234567890");
    let rhs = dec("987654321098765432109876543210");
    let sum = &lhs + &rhs;
    let expected = dec("1111111110111111111011111111100");
    assert_eq!(sum, expected);
}

#[test]
fn subtraction_matches_expected() {
    let lhs = dec("987654321098765432109876543210");
    let rhs = dec("123456789012345678901234567890");
    let difference = &lhs - &rhs;
    let expected = dec("864197532086419753208641975320");
    assert_eq!(difference, expected);
}

#[test]
fn multiplication_matches_expected() {
    let lhs = dec("12345678901234567890");
    let rhs = dec("31415926535897932384");
    let product = &lhs * &rhs;
    let expected = dec(&decimal_product(
        "12345678901234567890",
        "31415926535897932384",
    ));
    assert_eq!(product, expected);
}

#[test]
fn shifting_round_trips() {
    let value = hex("deadbeefcafebabe1122334455667788");
    let shifted = (&value << 37) >> 37;
    assert_eq!(shifted, value);
}

#[test]
fn parse_rejects_invalid_digits() {
    assert!(BigUint::parse_bytes(b"123xz", 16).is_none());
    assert!(BigUint::parse_bytes(b"hello", 10).is_none());
}

#[test]
fn zero_and_one_helpers_match_literals() {
    let zero = BigUint::zero();
    let one = BigUint::one();
    assert!(zero.is_zero());
    assert_eq!(one, BigUint::from(1u8));
    assert_eq!(zero + one.clone(), one);
}

#[test]
fn modpow_matches_reference() {
    let base = dec("123456789012345678901234567890");
    let exponent = dec("65537");
    let modulus = dec("1000000007");
    let result = base.modpow(&exponent, &modulus);
    let expected = BigUint::from(modpow_u128(
        123456789012345678901234567890u128,
        65537,
        1_000_000_007,
    ));
    assert_eq!(result, expected);
}

#[test]
fn modulo_matches_decimal_scan() {
    let value = dec("314159265358979323846264338327950288419716939937510");
    let modulus = dec("4294967291");
    let remainder = value.clone() % &modulus;
    let expected = {
        let mut acc = 0u128;
        let m = 4_294_967_291u128;
        for byte in "314159265358979323846264338327950288419716939937510".bytes() {
            let digit = (byte - b'0') as u128;
            acc = (acc * 10 + digit) % m;
        }
        BigUint::from(acc)
    };
    assert_eq!(remainder, expected);
}
