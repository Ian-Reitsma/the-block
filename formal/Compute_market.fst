module Compute_market


noeq type offer = {
  provider_bond:nat;
  consumer_bond:nat
}

let min_bond : nat = 1

val valid_offer : offer -> Tot bool
let valid_offer o = o.provider_bond >= min_bond && o.consumer_bond >= min_bond

val bonds_ge_min : o:offer -> Lemma (requires valid_offer o)
  (ensures o.provider_bond >= min_bond /\ o.consumer_bond >= min_bond)
let bonds_ge_min o = ()
