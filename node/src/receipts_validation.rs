//! Receipt validation and limits
//!
//! Provides validation functions and constants to prevent DoS attacks
//! and ensure receipt data integrity.

use crate::receipts::Receipt;

/// Maximum number of receipts allowed per block (DoS protection)
pub const MAX_RECEIPTS_PER_BLOCK: usize = 10_000;

/// Maximum total serialized receipt bytes per block (10MB limit)
pub const MAX_RECEIPT_BYTES_PER_BLOCK: usize = 10_000_000;

/// Maximum length for string fields (contract_id, provider, etc.)
pub const MAX_STRING_FIELD_LENGTH: usize = 256;

/// Minimum payment amount to emit a receipt (spam protection)
pub const MIN_PAYMENT_FOR_RECEIPT_CT: u64 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationError {
    TooManyReceipts {
        count: usize,
        max: usize,
    },
    ReceiptsTooLarge {
        bytes: usize,
        max: usize,
    },
    BlockHeightMismatch {
        receipt_height: u64,
        block_height: u64,
    },
    EmptyStringField {
        field: &'static str,
    },
    StringFieldTooLong {
        field: &'static str,
        length: usize,
        max: usize,
    },
    ZeroValue {
        field: &'static str,
    },
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ValidationError::TooManyReceipts { count, max } => {
                write!(f, "Too many receipts in block: {} (max: {})", count, max)
            }
            ValidationError::ReceiptsTooLarge { bytes, max } => {
                write!(f, "Receipts too large: {} bytes (max: {})", bytes, max)
            }
            ValidationError::BlockHeightMismatch {
                receipt_height,
                block_height,
            } => {
                write!(
                    f,
                    "Receipt block height {} doesn't match block height {}",
                    receipt_height, block_height
                )
            }
            ValidationError::EmptyStringField { field } => {
                write!(f, "Empty string field: {}", field)
            }
            ValidationError::StringFieldTooLong { field, length, max } => {
                write!(
                    f,
                    "String field {} too long: {} chars (max: {})",
                    field, length, max
                )
            }
            ValidationError::ZeroValue { field } => {
                write!(f, "Zero value in field: {}", field)
            }
        }
    }
}

impl std::error::Error for ValidationError {}

/// Validate a single receipt's fields
pub fn validate_receipt(receipt: &Receipt, block_height: u64) -> Result<(), ValidationError> {
    // Check block height matches
    if receipt.block_height() != block_height {
        return Err(ValidationError::BlockHeightMismatch {
            receipt_height: receipt.block_height(),
            block_height,
        });
    }

    // Validate based on receipt type
    match receipt {
        Receipt::Storage(r) => {
            validate_string_field("contract_id", &r.contract_id)?;
            validate_string_field("provider", &r.provider)?;

            if r.bytes == 0 {
                return Err(ValidationError::ZeroValue { field: "bytes" });
            }
            if r.price_ct == 0 {
                return Err(ValidationError::ZeroValue { field: "price_ct" });
            }
        }
        Receipt::Compute(r) => {
            validate_string_field("job_id", &r.job_id)?;
            validate_string_field("provider", &r.provider)?;

            if r.compute_units == 0 {
                return Err(ValidationError::ZeroValue {
                    field: "compute_units",
                });
            }
            if r.payment_ct == 0 {
                return Err(ValidationError::ZeroValue {
                    field: "payment_ct",
                });
            }
        }
        Receipt::Energy(r) => {
            validate_string_field("contract_id", &r.contract_id)?;
            validate_string_field("provider", &r.provider)?;

            if r.energy_units == 0 {
                return Err(ValidationError::ZeroValue {
                    field: "energy_units",
                });
            }
            if r.price_ct == 0 {
                return Err(ValidationError::ZeroValue { field: "price_ct" });
            }
        }
        Receipt::Ad(r) => {
            validate_string_field("campaign_id", &r.campaign_id)?;
            validate_string_field("publisher", &r.publisher)?;

            if r.impressions == 0 {
                return Err(ValidationError::ZeroValue {
                    field: "impressions",
                });
            }
            if r.spend_ct == 0 {
                return Err(ValidationError::ZeroValue { field: "spend_ct" });
            }
        }
    }

    Ok(())
}

/// Validate a string field is non-empty and within max length
fn validate_string_field(field_name: &'static str, value: &str) -> Result<(), ValidationError> {
    if value.is_empty() {
        return Err(ValidationError::EmptyStringField { field: field_name });
    }

    if value.len() > MAX_STRING_FIELD_LENGTH {
        return Err(ValidationError::StringFieldTooLong {
            field: field_name,
            length: value.len(),
            max: MAX_STRING_FIELD_LENGTH,
        });
    }

    Ok(())
}

/// Validate receipt count doesn't exceed maximum
pub fn validate_receipt_count(count: usize) -> Result<(), ValidationError> {
    if count > MAX_RECEIPTS_PER_BLOCK {
        return Err(ValidationError::TooManyReceipts {
            count,
            max: MAX_RECEIPTS_PER_BLOCK,
        });
    }
    Ok(())
}

/// Validate total receipt size doesn't exceed maximum
pub fn validate_receipt_size(bytes: usize) -> Result<(), ValidationError> {
    if bytes > MAX_RECEIPT_BYTES_PER_BLOCK {
        return Err(ValidationError::ReceiptsTooLarge {
            bytes,
            max: MAX_RECEIPT_BYTES_PER_BLOCK,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::receipts::StorageReceipt;

    #[test]
    fn valid_storage_receipt_passes() {
        let receipt = Receipt::Storage(StorageReceipt {
            contract_id: "contract_123".into(),
            provider: "provider_456".into(),
            bytes: 1000,
            price_ct: 500,
            block_height: 100,
            provider_escrow: 10000,
        });

        assert!(validate_receipt(&receipt, 100).is_ok());
    }

    #[test]
    fn empty_contract_id_fails() {
        let receipt = Receipt::Storage(StorageReceipt {
            contract_id: "".into(),
            provider: "provider_456".into(),
            bytes: 1000,
            price_ct: 500,
            block_height: 100,
            provider_escrow: 10000,
        });

        assert!(matches!(
            validate_receipt(&receipt, 100),
            Err(ValidationError::EmptyStringField {
                field: "contract_id"
            })
        ));
    }

    #[test]
    fn zero_bytes_fails() {
        let receipt = Receipt::Storage(StorageReceipt {
            contract_id: "contract_123".into(),
            provider: "provider_456".into(),
            bytes: 0,
            price_ct: 500,
            block_height: 100,
            provider_escrow: 10000,
        });

        assert!(matches!(
            validate_receipt(&receipt, 100),
            Err(ValidationError::ZeroValue { field: "bytes" })
        ));
    }

    #[test]
    fn block_height_mismatch_fails() {
        let receipt = Receipt::Storage(StorageReceipt {
            contract_id: "contract_123".into(),
            provider: "provider_456".into(),
            bytes: 1000,
            price_ct: 500,
            block_height: 100,
            provider_escrow: 10000,
        });

        assert!(matches!(
            validate_receipt(&receipt, 101),
            Err(ValidationError::BlockHeightMismatch { .. })
        ));
    }

    #[test]
    fn too_many_receipts_fails() {
        let result = validate_receipt_count(15_000);
        assert!(matches!(
            result,
            Err(ValidationError::TooManyReceipts {
                count: 15_000,
                max: 10_000
            })
        ));
    }

    #[test]
    fn receipts_too_large_fails() {
        let result = validate_receipt_size(15_000_000);
        assert!(matches!(
            result,
            Err(ValidationError::ReceiptsTooLarge {
                bytes: 15_000_000,
                max: 10_000_000
            })
        ));
    }

    #[test]
    fn string_too_long_fails() {
        let long_string = "a".repeat(300);
        let receipt = Receipt::Storage(StorageReceipt {
            contract_id: long_string,
            provider: "provider_456".into(),
            bytes: 1000,
            price_ct: 500,
            block_height: 100,
            provider_escrow: 10000,
        });

        assert!(matches!(
            validate_receipt(&receipt, 100),
            Err(ValidationError::StringFieldTooLong {
                field: "contract_id",
                ..
            })
        ));
    }
}
