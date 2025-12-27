module Fee_v2


(* Placeholder types mirroring ECONOMICS.md *)

noeq type pct = {
  base_fee: nat;
  tip: nat
}

noeq type fee_decomp = {
  miner_fee: nat;
  treasury_fee: nat
}

(* Decompose a selector into fees -- stub implementation *)
let decompose (s:pct) : fee_decomp =
  { miner_fee = s.base_fee; treasury_fee = s.tip }

(* Lemma stubs -- admitted *)
let fee_split_sum (s:pct) : Lemma
  ((decompose s).miner_fee + (decompose s).treasury_fee == s.base_fee + s.tip) = admit ()

let inv_fee_01 (s:pct) : Lemma
  ((decompose s).miner_fee >= 0 /\ (decompose s).treasury_fee >= 0) = admit ()

let miner_le_total (s:pct) : Lemma
  ((decompose s).miner_fee <= s.base_fee + s.tip) = admit ()
