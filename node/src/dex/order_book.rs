#![forbid(unsafe_code)]

use std::collections::{BTreeMap, VecDeque};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Buy,
    Sell,
}

#[derive(Debug, Clone)]
pub struct Order {
    pub id: u64,
    pub account: String,
    pub side: Side,
    pub amount: u64,
    pub price: u64,
}

#[derive(Default)]
pub struct OrderBook {
    pub bids: BTreeMap<u64, VecDeque<Order>>, // price -> orders
    pub asks: BTreeMap<u64, VecDeque<Order>>, // price -> orders
    next_id: u64,
}

impl OrderBook {
    pub fn place(&mut self, mut order: Order) -> Vec<(Order, Order, u64)> {
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
        trades
    }
}
