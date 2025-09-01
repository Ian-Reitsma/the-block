#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, VecDeque};

use super::{trust_lines::TrustLedger, DexStore};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Side {
    Buy,
    Sell,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Order {
    pub id: u64,
    pub account: String,
    pub side: Side,
    pub amount: u64,
    pub price: u64,
    pub max_slippage_bps: u64,
}

#[derive(Default, Serialize, Deserialize, Debug, Clone)]
pub struct OrderBook {
    pub bids: BTreeMap<u64, VecDeque<Order>>, // price -> orders
    pub asks: BTreeMap<u64, VecDeque<Order>>, // price -> orders
    next_id: u64,
}

impl OrderBook {
    pub fn place(&mut self, mut order: Order) -> Result<Vec<(Order, Order, u64)>, &'static str> {
        let limit = match order.side {
            Side::Buy => order.price * (10_000 + order.max_slippage_bps) / 10_000,
            Side::Sell => {
                if order.max_slippage_bps > 10_000 {
                    return Err("slippage");
                }
                order.price * (10_000 - order.max_slippage_bps) / 10_000
            }
        };
        let best_opt = match order.side {
            Side::Buy => self.asks.keys().next().cloned(),
            Side::Sell => self.bids.keys().rev().next().cloned(),
        };
        if let Some(best) = best_opt {
            if (order.side == Side::Buy && best > limit)
                || (order.side == Side::Sell && best < limit)
            {
                return Err("slippage");
            }
        }

        order.id = self.next_id;
        self.next_id += 1;
        let mut trades = Vec::new();
        match order.side {
            Side::Buy => {
                while let Some((&price, queue)) = self.asks.iter_mut().next() {
                    if price > order.price || order.amount == 0 {
                        break;
                    }
                    if let Some(mut ask) = queue.pop_front() {
                        let qty = order.amount.min(ask.amount);
                        order.amount -= qty;
                        ask.amount -= qty;
                        trades.push((order.clone(), ask.clone(), qty));
                        if ask.amount > 0 {
                            queue.push_front(ask);
                        }
                        if order.amount == 0 {
                            break;
                        }
                    }
                    if queue.is_empty() {
                        self.asks.remove(&price);
                    }
                }
                if order.amount > 0 {
                    self.bids.entry(order.price).or_default().push_back(order);
                }
            }
            Side::Sell => {
                while let Some((&price, queue)) = self.bids.iter_mut().rev().next() {
                    if price < order.price || order.amount == 0 {
                        break;
                    }
                    if let Some(mut bid) = queue.pop_front() {
                        let qty = order.amount.min(bid.amount);
                        order.amount -= qty;
                        bid.amount -= qty;
                        trades.push((bid.clone(), order.clone(), qty));
                        if bid.amount > 0 {
                            queue.push_front(bid);
                        }
                        if order.amount == 0 {
                            break;
                        }
                    }
                    if queue.is_empty() {
                        self.bids.remove(&price);
                    }
                }
                if order.amount > 0 {
                    self.asks.entry(order.price).or_default().push_back(order);
                }
            }
        }
        Ok(trades)
    }

    /// Place an order and settle resulting trades against the provided trust ledger.
    pub fn place_and_settle(
        &mut self,
        order: Order,
        ledger: &mut TrustLedger,
    ) -> Result<Vec<(Order, Order, u64)>, &'static str> {
        self.place_settle_persist(order, ledger, None)
    }

    pub fn place_settle_persist(
        &mut self,
        order: Order,
        ledger: &mut TrustLedger,
        mut store: Option<&mut DexStore>,
    ) -> Result<Vec<(Order, Order, u64)>, &'static str> {
        let trades = self.place(order)?;
        for (buy, sell, qty) in &trades {
            let value = sell.price * *qty;
            ledger.adjust(&buy.account, &sell.account, value as i64);
            ledger.adjust(&sell.account, &buy.account, -(value as i64));
            if let Some(st) = store.as_deref_mut() {
                st.log_trade(&(buy.clone(), sell.clone(), *qty));
            }
        }
        if let Some(st) = store.as_deref_mut() {
            st.save_book(self);
        }
        Ok(trades)
    }
}
