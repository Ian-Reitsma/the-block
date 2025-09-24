# Remote Signer Security
> **Review (2025-09-23):** Validated for the dependency-sovereignty pivot; third-token references removed; align changes with the in-house roadmap.

This document outlines the recommended threat model and recovery steps for the
air-gapped remote signer.

## Threat Model
- Signer keys remain offline and are never exposed on networked hosts.
- PSBT files are transferred via QR codes or NFC to minimize attack surface.
- Multi-factor authentication using FIDO2/U2F tokens gates all signing
  requests.

## Recovery
- Keep encrypted backups of the master seed in multiple geographic locations.
- Rotate signing keys periodically; each rotation increments the
  `remote_signer_key_rotation_total` metric.
- If a signer device is lost, revoke it and re-issue using the governance
  portal before restoring from backups.