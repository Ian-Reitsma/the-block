#![forbid(unsafe_code)]

use crate::config::ReadAckPrivacyMode;
use crate::read_receipt::ReadAck;
use crate::ReadAckError;

/// Verify the privacy proof attached to a `ReadAck` according to the configured mode.
pub fn verify_ack(mode: ReadAckPrivacyMode, ack: &ReadAck) -> Result<(), ReadAckError> {
    match mode {
        ReadAckPrivacyMode::Enforce => {
            if ack.verify_privacy() {
                Ok(())
            } else {
                Err(ReadAckError::PrivacyProofRejected)
            }
        }
        ReadAckPrivacyMode::Observe => {
            if !ack.verify_privacy() {
                diagnostics::log::warn!("read_ack_privacy_verification_failed");
            }
            Ok(())
        }
        ReadAckPrivacyMode::Disabled => Ok(()),
    }
}
