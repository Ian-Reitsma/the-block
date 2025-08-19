module Fee

open FStar.Mul

(* Placeholder types mirroring ECONOMICS.md *)

noeq type fee_selector = {
  base_fee: nat;
  tip: nat
}

noeq type fee_decomp = {
  miner_fee: nat;
  treasury_fee: nat
}

(* Decompose a selector into fees -- stub implementation *)
let decompose (s:fee_selector) : fee_decomp =
  { miner_fee = s.base_fee; treasury_fee = s.tip }

(* Lemma stubs -- admitted *)
let fee_split_sum (s:fee_selector) : Lemma (decompose s).miner_fee + (decompose s).treasury_fee == s.base_fee + s.tip = admit ()

let inv_fee_01 (s:fee_selector) : Lemma (decompose s).miner_fee >= 0 /\ (decompose s).treasury_fee >= 0 = admit ()
