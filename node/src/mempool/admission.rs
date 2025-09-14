use crate::{accounts::AccountValidation, Account, SignedTransaction, TxAdmissionError};

/// Validate a transaction against the sender's account rules.
pub fn validate_account(
    acc: &mut Account,
    tx: &SignedTransaction,
) -> Result<(), TxAdmissionError> {
    acc.validate_tx(tx)
}
