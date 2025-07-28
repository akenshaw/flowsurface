use std::collections::HashMap;

use data::{
    UserTimezone,
    aggr::time::TimeSeries,
    chart::{Basis, comparison::Config, kline::KlineDataPoint},
};
use exchange::{Ticker, TickerInfo};
use iced::{
    Element,
    widget::{canvas::Cache, text},
};

use crate::chart::Action;

#[derive(Clone, Debug)]
pub enum Message {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Zoom(u16);

impl Zoom {
    pub fn increment(self) -> Self {
        Self(self.0.saturating_add(1).min(10))
    }

    pub fn decrement(self) -> Self {
        Self(self.0.saturating_sub(1).max(1))
    }
}

impl Default for Zoom {
    fn default() -> Self {
        Self(2)
    }
}

pub struct ComparisonChart {
    pub cache: Cache,
    pub config: Config,
    pub tickers: Vec<TickerInfo>,
    pub data: HashMap<Ticker, TimeSeries<KlineDataPoint>>,
    pub interval: exchange::Timeframe,
    pub last_update: std::time::Instant,
    pub zoom: Zoom,
}

impl ComparisonChart {
    pub fn new(config: Option<Config>, tickers: Vec<TickerInfo>, interval: Basis) -> Self {
        let interval = match interval {
            Basis::Time(tf) => tf,
            _ => exchange::Timeframe::M5,
        };

        let config = config.unwrap_or_else(|| Config {});
        let data = tickers
            .iter()
            .map(|t_info| {
                (
                    t_info.ticker,
                    TimeSeries::<KlineDataPoint>::new(interval, t_info.min_ticksize, &[], &[]),
                )
            })
            .collect();

        Self {
            cache: Cache::default(),
            config,
            tickers,
            data,
            interval,
            last_update: std::time::Instant::now(),
            zoom: Zoom::default(),
        }
    }

    pub fn insert_klines(
        &mut self,
        ticker: Ticker,
        klines: &[exchange::Kline],
        timeframe: exchange::Timeframe,
    ) {
        if let Some(series) = self.data.get_mut(&ticker) {
            series.insert_klines(klines);
        }
    }

    pub fn view(&self, _timezone: UserTimezone) -> Element<Message> {
        iced::widget::center(text("Comparison Chart")).into()
    }

    pub fn invalidate(&mut self, now: Option<std::time::Instant>) -> Option<Action> {
        self.cache.clear();
        if let Some(now) = now {
            self.last_update = now;
        }

        None
    }
}
