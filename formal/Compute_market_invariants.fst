module Compute_market_invariants

noeq type offer = {
  provider_bond:int;
  consumer_bond:int
}

noeq type job = {
  provider_bond:int;
  consumer_bond:int
}

noeq type state = {
  offers_provider:int;
  offers_consumer:int;
  jobs_provider:int;
  jobs_consumer:int;
  provider_balance:int;
  consumer_balance:int
}

let total_bonds (s:state) =
  s.offers_provider + s.offers_consumer +
  s.jobs_provider + s.jobs_consumer +
  s.provider_balance + s.consumer_balance

let post_offer (s:state) (o:offer) : state =
  {
    offers_provider = s.offers_provider + o.provider_bond;
    offers_consumer = s.offers_consumer + o.consumer_bond;
    jobs_provider = s.jobs_provider;
    jobs_consumer = s.jobs_consumer;
    provider_balance = s.provider_balance - o.provider_bond;
    consumer_balance = s.consumer_balance - o.consumer_bond
  }

let accept_offer (s:state) (o:offer) : state =
  {
    offers_provider = s.offers_provider - o.provider_bond;
    offers_consumer = s.offers_consumer - o.consumer_bond;
    jobs_provider = s.jobs_provider + o.provider_bond;
    jobs_consumer = s.jobs_consumer + o.consumer_bond;
    provider_balance = s.provider_balance;
    consumer_balance = s.consumer_balance
  }

let finalize_job (s:state) (j:job) : state =
  {
    offers_provider = s.offers_provider;
    offers_consumer = s.offers_consumer;
    jobs_provider = s.jobs_provider - j.provider_bond;
    jobs_consumer = s.jobs_consumer - j.consumer_bond;
    provider_balance = s.provider_balance + j.provider_bond;
    consumer_balance = s.consumer_balance + j.consumer_bond
  }

val post_offer_preserves:
  s:state -> o:offer -> Lemma (requires True)
    (ensures total_bonds (post_offer s o) == total_bonds s)
let post_offer_preserves _ _ = ()

val accept_offer_preserves:
  s:state -> o:offer -> Lemma (requires True)
    (ensures total_bonds (accept_offer s o) == total_bonds s)
let accept_offer_preserves _ _ = ()

val finalize_job_preserves:
  s:state -> j:job -> Lemma (requires True)
    (ensures total_bonds (finalize_job s j) == total_bonds s)
let finalize_job_preserves _ _ = ()
