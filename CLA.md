# The Block Contributor License Agreement (CLA) v1.0

This CLA applies to contributions made to the maintainers of The Block (the "Project") in the `the-block-core` repository. By submitting a contribution, you agree to the terms below. If you are contributing on behalf of an entity, you represent that you have authority to bind that entity and that the entity accepts these terms.

## 1. License Grant (Inbound = Outbound)
- You grant the Project maintainers a perpetual, worldwide, royalty-free, irrevocable license to use, reproduce, modify, publicly perform, publicly display, sublicense, distribute, and relicense your contribution and any necessary derivative works.
- The Project will continue to license `the-block-core` under Apache 2.0 (or Apache/MIT dual where already in use). The maintainers may also offer your contribution under additional terms for proprietary products (e.g., `the-block-enterprise`) without further consent, while keeping the open-core under a permissive license.

## 2. Patent License
- You grant the Project maintainers and downstream users a perpetual, worldwide, royalty-free, irrevocable (except as stated here) patent license to make, have made, use, offer to sell, sell, import, and otherwise transfer your contribution. If you initiate a patent claim against the Project related to your contribution, this patent license terminates.

## 3. Representations
- You have the legal right to submit the contribution and to grant these licenses; no third-party agreement or policy (including employer policies) prohibits you from doing so.
- Your contribution is your original work (or you have identified third-party components and their licenses in the contribution).
- You will not knowingly include material that is malicious or intentionally deceptive.

## 4. Developer Certificate of Origin (DCO) Sign-off
- Every commit must include a `Signed-off-by: Your Name <email>` line (use `git commit -s`). The sign-off certifies compliance with the DCO and this CLA.
- If contributing on behalf of an entity, include the entity name in the sign-off (e.g., `Signed-off-by: Jane Doe <jane@company.com> (ExampleCorp)`).

## 5. Acceptance Process
- Before your first contribution is merged, you must indicate agreement to this CLA (via the Project's CLA portal or a pull-request comment stating "I agree to The Block CLA v1.0") and ensure your commits carry the sign-off line. Contributions without a recorded agreement and sign-off will not be merged.
- CLA compliance is enforced in CI (`scripts/check_cla.sh`, `node/tests/cla_check.rs`). Maintainers will keep a ledger of contributors who have agreed to this CLA tied to their sign-off identity.

## 6. Miscellaneous
- This CLA does not transfer trademark rights. Use of Project marks is governed separately.
- This CLA does not create an employment relationship. Contributions are provided "as-is" without warranties.

If you cannot agree to these terms, do not submit contributions to this repository.
