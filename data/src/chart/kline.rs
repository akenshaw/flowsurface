use std::collections::HashMap;

use exchange::Trade;
use ordered_float::OrderedFloat;
use serde::{Deserialize, Serialize};

use crate::util::round_to_tick;

#[derive(Debug, Clone, Default)]
pub struct GroupedTrades {
    pub buy_qty: f32,
    pub sell_qty: f32,
    pub first_time: u64,
    pub last_time: u64,
    pub buy_count: usize,
    pub sell_count: usize,
}

impl GroupedTrades {
    fn new(trade: &Trade) -> Self {
        Self {
            buy_qty: if trade.is_sell { 0.0 } else { trade.qty },
            sell_qty: if trade.is_sell { trade.qty } else { 0.0 },
            first_time: trade.time,
            last_time: trade.time,
            buy_count: if trade.is_sell { 0 } else { 1 },
            sell_count: if trade.is_sell { 1 } else { 0 },
        }
    }

    fn add_trade(&mut self, trade: &Trade) {
        if trade.is_sell {
            self.sell_qty += trade.qty;
            self.sell_count += 1;
        } else {
            self.buy_qty += trade.qty;
            self.buy_count += 1;
        }
        self.last_time = trade.time;
    }

    pub fn total_qty(&self) -> f32 {
        self.buy_qty + self.sell_qty
    }

    pub fn delta_qty(&self) -> f32 {
        self.buy_qty - self.sell_qty
    }
}

#[derive(Debug, Clone, Default)]
pub struct KlineTrades {
    pub trades: HashMap<OrderedFloat<f32>, GroupedTrades>,
    pub poc: Option<PointOfControl>,
}

impl KlineTrades {
    pub fn new() -> Self {
        Self {
            trades: HashMap::new(),
            poc: None,
        }
    }

    pub fn first_trade_t(&self) -> Option<u64> {
        self.trades.values().map(|group| group.first_time).min()
    }

    pub fn last_trade_t(&self) -> Option<u64> {
        self.trades.values().map(|group| group.last_time).max()
    }

    pub fn add_trade_at_price_level(&mut self, trade: &Trade, tick_size: f32) {
        let price_level = OrderedFloat(round_to_tick(trade.price, tick_size));

        if let Some(group) = self.trades.get_mut(&price_level) {
            group.add_trade(trade);
        } else {
            self.trades.insert(price_level, GroupedTrades::new(trade));
        }
    }

    pub fn max_qty_by<F>(&self, highest: OrderedFloat<f32>, lowest: OrderedFloat<f32>, f: F) -> f32
    where
        F: Fn(f32, f32) -> f32,
    {
        let mut max_qty: f32 = 0.0;
        for (price, group) in &self.trades {
            if price >= &lowest && price <= &highest {
                max_qty = max_qty.max(f(group.buy_qty, group.sell_qty));
            }
        }
        max_qty
    }

    pub fn calculate_poc(&mut self) {
        if self.trades.is_empty() {
            return;
        }

        let mut max_volume = 0.0;
        let mut poc_price = 0.0;

        for (price, group) in &self.trades {
            let total_volume = group.total_qty();
            if total_volume > max_volume {
                max_volume = total_volume;
                poc_price = price.0;
            }
        }

        self.poc = Some(PointOfControl {
            price: poc_price,
            volume: max_volume,
            status: NPoc::default(),
        });
    }

    pub fn set_poc_status(&mut self, status: NPoc) {
        if let Some(poc) = &mut self.poc {
            poc.status = status;
        }
    }

    pub fn poc_price(&self) -> Option<f32> {
        self.poc.map(|poc| poc.price)
    }

    pub fn clear(&mut self) {
        self.trades.clear();
        self.poc = None;
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize, Serialize)]
pub enum KlineChartKind {
    #[default]
    Candles,
    Footprint {
        clusters: ClusterKind,
        studies: Vec<FootprintStudy>,
    },
}

impl KlineChartKind {
    pub fn min_scaling(&self) -> f32 {
        match self {
            KlineChartKind::Footprint { .. } => 0.4,
            KlineChartKind::Candles => 0.6,
        }
    }

    pub fn max_scaling(&self) -> f32 {
        match self {
            KlineChartKind::Footprint { .. } => 1.2,
            KlineChartKind::Candles => 2.5,
        }
    }

    pub fn max_cell_width(&self) -> f32 {
        match self {
            KlineChartKind::Footprint { .. } => 360.0,
            KlineChartKind::Candles => 16.0,
        }
    }

    pub fn min_cell_width(&self) -> f32 {
        match self {
            KlineChartKind::Footprint { .. } => 80.0,
            KlineChartKind::Candles => 1.0,
        }
    }

    pub fn max_cell_height(&self) -> f32 {
        match self {
            KlineChartKind::Footprint { .. } => 90.0,
            KlineChartKind::Candles => 8.0,
        }
    }

    pub fn min_cell_height(&self) -> f32 {
        match self {
            KlineChartKind::Footprint { .. } => 1.0,
            KlineChartKind::Candles => 0.001,
        }
    }

    pub fn default_cell_width(&self) -> f32 {
        match self {
            KlineChartKind::Footprint { .. } => 80.0,
            KlineChartKind::Candles => 4.0,
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Default, Deserialize, Serialize)]
pub enum ClusterKind {
    #[default]
    BidAsk,
    VolumeProfile,
    DeltaProfile,
}

impl ClusterKind {
    pub const ALL: [ClusterKind; 3] = [
        ClusterKind::BidAsk,
        ClusterKind::VolumeProfile,
        ClusterKind::DeltaProfile,
    ];
}

impl std::fmt::Display for ClusterKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ClusterKind::BidAsk => write!(f, "Bid/Ask"),
            ClusterKind::VolumeProfile => write!(f, "Volume Profile"),
            ClusterKind::DeltaProfile => write!(f, "Delta Profile"),
        }
    }
}

#[derive(Debug, Default, Copy, Clone, PartialEq, Deserialize, Serialize)]
pub struct Config {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub enum FootprintStudy {
    NPoC {
        lookback: usize,
    },
    Imbalance {
        threshold: usize,
        color_scale: Option<usize>,
        ignore_zeros: bool,
    },
}

impl FootprintStudy {
    pub fn is_same_type(&self, other: &Self) -> bool {
        matches!(
            (self, other),
            (FootprintStudy::NPoC { .. }, FootprintStudy::NPoC { .. })
                | (
                    FootprintStudy::Imbalance { .. },
                    FootprintStudy::Imbalance { .. }
                )
        )
    }
}

impl FootprintStudy {
    pub const ALL: [FootprintStudy; 2] = [
        FootprintStudy::NPoC { lookback: 80 },
        FootprintStudy::Imbalance {
            threshold: 200,
            color_scale: Some(400),
            ignore_zeros: true,
        },
    ];
}

impl std::fmt::Display for FootprintStudy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FootprintStudy::NPoC { .. } => write!(f, "Naked Point of Control"),
            FootprintStudy::Imbalance { .. } => write!(f, "Imbalance"),
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct PointOfControl {
    pub price: f32,
    pub volume: f32,
    pub status: NPoc,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum NPoc {
    #[default]
    None,
    Naked,
    Filled {
        at: u64,
    },
}

impl NPoc {
    pub fn filled(&mut self, at: u64) {
        *self = NPoc::Filled { at };
    }

    pub fn unfilled(&mut self) {
        *self = NPoc::Naked;
    }
}
