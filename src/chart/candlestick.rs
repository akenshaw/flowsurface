use std::collections::hash_map::Entry;
use std::collections::{BTreeMap, HashMap};

use data::UserTimezone;
use data::chart::ChartLayout;
use data::chart::indicators::{CandlestickIndicator, Indicator};
use iced::theme::palette::Extended;
use iced::widget::canvas::{LineDash, Path, Stroke};
use iced::widget::container;
use iced::widget::{
    canvas::{self, Event, Geometry},
    column,
};
use iced::{Element, Length, Point, Rectangle, Renderer, Size, Theme, Vector, mouse};

use data::aggr::{ticks::TickAggr, time::TimeSeries};
use exchange::fetcher::{FetchRange, RequestHandler};
use exchange::{Kline, OpenInterest as OIData, TickerInfo, Timeframe, Trade};

use super::scale::PriceInfoLabel;
use super::{
    Action, Basis, Caches, Chart, ChartConstants, ChartData, CommonChartData, Interaction, Message,
    indicator,
};
use super::{canvas_interaction, count_decimals, request_fetch, update_chart, view_chart};

impl Chart for CandlestickChart {
    fn get_common_data(&self) -> &CommonChartData {
        &self.chart
    }

    fn get_common_data_mut(&mut self) -> &mut CommonChartData {
        &mut self.chart
    }

    fn update_chart(&mut self, message: &Message) {
        update_chart(self, message);
        self.render_start();
    }

    fn canvas_interaction(
        &self,
        interaction: &mut Interaction,
        event: &Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        canvas_interaction(self, interaction, event, bounds, cursor)
    }

    fn view_indicators<I: Indicator>(&self, indicators: &[I]) -> Option<Element<Message>> {
        self.view_indicators(indicators)
    }

    fn get_visible_timerange(&self) -> (u64, u64) {
        let chart = self.get_common_data();

        let visible_region = chart.visible_region(chart.bounds.size());

        let earliest = chart.x_to_interval(visible_region.x);
        let latest = chart.x_to_interval(visible_region.x + visible_region.width);

        (earliest, latest)
    }

    /// used for x-axis label generation
    fn get_interval_keys(&self) -> Vec<u64> {
        match &self.data_source {
            ChartData::TimeBased(_) => {
                // timeseries data keys are already ordered unlike tick based data,
                // currently we can just guesstimate them from the visible range instead
                // `TimeBased` match arms on this trait method exists just for modularity
                // timeseries.data_points.keys().cloned().collect()
                vec![]
            }
            ChartData::TickBased(tick_aggr) => tick_aggr
                .data_points
                .iter()
                .map(|dp| dp.start_timestamp)
                .collect(),
        }
    }

    fn is_empty(&self) -> bool {
        match &self.data_source {
            ChartData::TimeBased(timeseries) => timeseries.data_points.is_empty(),
            ChartData::TickBased(tick_aggr) => tick_aggr.data_points.is_empty(),
        }
    }
}

impl ChartConstants for CandlestickChart {
    const MIN_SCALING: f32 = 0.6;
    const MAX_SCALING: f32 = 2.5;

    const MAX_CELL_WIDTH: f32 = 16.0;
    const MIN_CELL_WIDTH: f32 = 1.0;

    const MAX_CELL_HEIGHT: f32 = 8.0;
    const MIN_CELL_HEIGHT: f32 = 0.001;

    const DEFAULT_CELL_WIDTH: f32 = 4.0;
}

#[allow(dead_code)]
enum IndicatorData {
    Volume(Caches, BTreeMap<u64, (f32, f32)>),
    OpenInterest(Caches, BTreeMap<u64, f32>),
}

impl IndicatorData {
    fn clear_cache(&mut self) {
        match self {
            IndicatorData::Volume(caches, _) | IndicatorData::OpenInterest(caches, _) => {
                caches.clear_all();
            }
        }
    }
}

pub struct CandlestickChart {
    chart: CommonChartData,
    data_source: ChartData,
    indicators: HashMap<CandlestickIndicator, IndicatorData>,
    raw_trades: Vec<Trade>,
    request_handler: RequestHandler,
}

impl CandlestickChart {
    pub fn new(
        layout: ChartLayout,
        basis: Basis,
        klines_raw: &[Kline],
        raw_trades: Vec<Trade>,
        tick_size: f32,
        enabled_indicators: &[CandlestickIndicator],
        ticker_info: Option<TickerInfo>,
    ) -> CandlestickChart {
        match basis {
            Basis::Time(interval) => {
                let timeseries = TimeSeries::new(interval.into(), tick_size, &[], klines_raw);

                let base_price_y = timeseries.get_base_price();
                let latest_x = timeseries.get_latest_timestamp().unwrap_or(0);
                let (scale_high, scale_low) = timeseries.get_price_scale(60);
                let volume_data = timeseries.get_volume_data();

                let y_ticks = (scale_high - scale_low) / tick_size;

                CandlestickChart {
                    chart: CommonChartData {
                        cell_width: Self::DEFAULT_CELL_WIDTH,
                        cell_height: 200.0 / y_ticks,
                        base_price_y,
                        latest_x,
                        tick_size,
                        crosshair: layout.crosshair,
                        indicators_split: layout.indicators_split,
                        decimals: count_decimals(tick_size),
                        ticker_info,
                        basis: super::Basis::Time(interval),
                        ..Default::default()
                    },
                    data_source: ChartData::TimeBased(timeseries),
                    indicators: {
                        enabled_indicators
                            .iter()
                            .map(|indicator| {
                                (
                                    *indicator,
                                    match indicator {
                                        CandlestickIndicator::Volume => IndicatorData::Volume(
                                            Caches::default(),
                                            volume_data.clone(),
                                        ),
                                        CandlestickIndicator::OpenInterest => {
                                            IndicatorData::OpenInterest(
                                                Caches::default(),
                                                BTreeMap::new(),
                                            )
                                        }
                                    },
                                )
                            })
                            .collect()
                    },
                    raw_trades,
                    request_handler: RequestHandler::new(),
                }
            }
            Basis::Tick(interval) => {
                let tick_aggr = TickAggr::new(interval, tick_size, &raw_trades);
                let volume_data = tick_aggr.get_volume_data();

                CandlestickChart {
                    chart: CommonChartData {
                        cell_width: Self::DEFAULT_CELL_WIDTH,
                        cell_height: Self::MAX_CELL_HEIGHT,
                        tick_size,
                        decimals: count_decimals(tick_size),
                        crosshair: layout.crosshair,
                        indicators_split: layout.indicators_split,
                        ticker_info,
                        basis,
                        ..Default::default()
                    },
                    data_source: ChartData::TickBased(tick_aggr),
                    indicators: {
                        enabled_indicators
                            .iter()
                            .map(|indicator| {
                                (
                                    *indicator,
                                    match indicator {
                                        CandlestickIndicator::Volume => IndicatorData::Volume(
                                            Caches::default(),
                                            volume_data.clone(),
                                        ),
                                        CandlestickIndicator::OpenInterest => {
                                            IndicatorData::OpenInterest(
                                                Caches::default(),
                                                BTreeMap::new(),
                                            )
                                        }
                                    },
                                )
                            })
                            .collect()
                    },
                    raw_trades,
                    request_handler: RequestHandler::new(),
                }
            }
        }
    }

    pub fn get_tick_size(&self) -> f32 {
        self.chart.tick_size
    }

    pub fn update_latest_kline(&mut self, kline: &Kline) -> Action {
        if let ChartData::TimeBased(ref mut timeseries) = self.data_source {
            timeseries.insert_klines(&[kline.to_owned()]);

            if let Some(IndicatorData::Volume(_, data)) =
                self.indicators.get_mut(&CandlestickIndicator::Volume)
            {
                data.insert(kline.time, (kline.volume.0, kline.volume.1));
            };

            let chart = self.get_common_data_mut();

            if (kline.time) > chart.latest_x {
                chart.latest_x = kline.time;
            }

            chart.last_price = Some(PriceInfoLabel::new(kline.close, kline.open));

            self.render_start();
            return self.get_missing_data_task();
        }

        Action::None
    }

    fn get_missing_data_task(&mut self) -> Action {
        match &self.data_source {
            ChartData::TimeBased(timeseries) => {
                let timeframe = timeseries.interval.to_milliseconds();

                let (visible_earliest, visible_latest) = self.get_visible_timerange();
                let (kline_earliest, kline_latest) = timeseries.get_kline_timerange();
                let earliest = visible_earliest - (visible_latest - visible_earliest);

                // priority 1, basic kline data fetch
                if visible_earliest < kline_earliest {
                    let range = FetchRange::Kline(earliest, kline_earliest);
                    if let Some(action) = request_fetch(&mut self.request_handler, range) {
                        return action;
                    }
                }

                // priority 2, Open Interest data
                for data in self.indicators.values() {
                    if let IndicatorData::OpenInterest(_, _) = data {
                        if timeframe >= Timeframe::M5.to_milliseconds()
                            && self.chart.ticker_info.is_some_and(|t| t.is_perps())
                        {
                            let (oi_earliest, oi_latest) = self.get_oi_timerange(kline_latest);

                            if visible_earliest < oi_earliest {
                                let range = FetchRange::OpenInterest(earliest, oi_earliest);
                                if let Some(action) =
                                    request_fetch(&mut self.request_handler, range)
                                {
                                    return action;
                                }
                            }

                            if oi_latest < kline_latest {
                                let range =
                                    FetchRange::OpenInterest(oi_latest.max(earliest), kline_latest);
                                if let Some(action) =
                                    request_fetch(&mut self.request_handler, range)
                                {
                                    return action;
                                }
                            }
                        }
                    }
                }

                // priority 3, missing klines & integrity check
                if let Some(missing_keys) =
                    timeseries.check_integrity(kline_earliest, kline_latest, timeframe)
                {
                    let latest = missing_keys.iter().max().unwrap_or(&visible_latest) + timeframe;
                    let earliest =
                        missing_keys.iter().min().unwrap_or(&visible_earliest) - timeframe;

                    let range = FetchRange::Kline(earliest, latest);
                    if let Some(action) = request_fetch(&mut self.request_handler, range) {
                        return action;
                    }
                }
            }
            ChartData::TickBased(_) => {
                // TODO: implement trade fetch
            }
        }

        Action::None
    }

    pub fn insert_new_klines(&mut self, req_id: uuid::Uuid, klines_raw: &[Kline]) {
        match self.data_source {
            ChartData::TimeBased(ref mut timeseries) => {
                timeseries.insert_klines(klines_raw);

                if let Some(IndicatorData::Volume(_, data)) =
                    self.indicators.get_mut(&CandlestickIndicator::Volume)
                {
                    data.extend(
                        klines_raw
                            .iter()
                            .map(|kline| (kline.time, (kline.volume.0, kline.volume.1))),
                    );
                };

                if klines_raw.is_empty() {
                    self.request_handler
                        .mark_failed(req_id, "No data received".to_string());
                } else {
                    self.request_handler.mark_completed(req_id);
                }
            }
            ChartData::TickBased(_) => {}
        }

        self.render_start();
    }

    pub fn insert_open_interest(&mut self, req_id: Option<uuid::Uuid>, oi_data: &[OIData]) {
        if let Some(req_id) = req_id {
            if oi_data.is_empty() {
                self.request_handler
                    .mark_failed(req_id, "No data received".to_string());
            } else {
                self.request_handler.mark_completed(req_id);
            }
        }

        if let Some(IndicatorData::OpenInterest(_, data)) =
            self.indicators.get_mut(&CandlestickIndicator::OpenInterest)
        {
            data.extend(oi_data.iter().map(|oi| (oi.time, oi.value)));
        };
    }

    pub fn insert_trades_buffer(&mut self, trades_buffer: &[Trade]) {
        self.raw_trades.extend_from_slice(trades_buffer);

        if let ChartData::TickBased(ref mut tick_aggr) = self.data_source {
            let old_dp_len = tick_aggr.data_points.len();

            tick_aggr.insert_trades(trades_buffer);

            if let Some(IndicatorData::Volume(_, data)) =
                self.indicators.get_mut(&CandlestickIndicator::Volume)
            {
                let start_idx = old_dp_len.saturating_sub(1);
                for (idx, dp) in tick_aggr.data_points.iter().enumerate().skip(start_idx) {
                    data.insert(idx as u64, (dp.volume_buy, dp.volume_sell));
                }
            }

            if let Some(last_dp) = tick_aggr.data_points.last() {
                self.chart.last_price =
                    Some(PriceInfoLabel::new(last_dp.close_price, last_dp.open_price));
            } else {
                self.chart.last_price = None;
            }

            self.render_start();
        }
    }

    pub fn set_tick_basis(&mut self, tick_basis: u64) {
        self.chart.basis = Basis::Tick(tick_basis);

        let new_tick_aggr = TickAggr::new(tick_basis, self.chart.tick_size, &self.raw_trades);

        if let Some(indicator) = self.indicators.get_mut(&CandlestickIndicator::Volume) {
            *indicator = IndicatorData::Volume(Caches::default(), new_tick_aggr.get_volume_data());
        }

        self.data_source = ChartData::TickBased(new_tick_aggr);

        self.render_start();
    }

    pub fn get_raw_trades(&self) -> Vec<Trade> {
        self.raw_trades.clone()
    }

    fn get_oi_timerange(&self, latest_kline: u64) -> (u64, u64) {
        let mut from_time = latest_kline;
        let mut to_time = u64::MIN;

        if let Some(IndicatorData::OpenInterest(_, data)) =
            self.indicators.get(&CandlestickIndicator::OpenInterest)
        {
            data.iter().for_each(|(time, _)| {
                from_time = from_time.min(*time);
                to_time = to_time.max(*time);
            });
        };

        (from_time, to_time)
    }

    fn render_start(&mut self) {
        let chart_state = &mut self.chart;

        if chart_state.autoscale {
            chart_state.translation = Vector::new(
                0.5 * (chart_state.bounds.width / chart_state.scaling)
                    - (8.0 * chart_state.cell_width / chart_state.scaling),
                self.data_source
                    .get_latest_price_range_y_midpoint(chart_state),
            );
        }

        chart_state.cache.clear_all();

        self.indicators.iter_mut().for_each(|(_, data)| {
            data.clear_cache();
        });
    }

    pub fn get_chart_layout(&self) -> ChartLayout {
        self.chart.get_chart_layout()
    }

    pub fn toggle_indicator(&mut self, indicator: CandlestickIndicator) {
        match self.indicators.entry(indicator) {
            Entry::Occupied(entry) => {
                entry.remove();
            }
            Entry::Vacant(entry) => {
                let data = match indicator {
                    CandlestickIndicator::Volume => match &self.data_source {
                        ChartData::TimeBased(timeseries) => {
                            let volume_data = timeseries
                                .data_points
                                .iter()
                                .map(|(time, dp)| (*time, (dp.kline.volume.0, dp.kline.volume.1)))
                                .collect();

                            IndicatorData::Volume(Caches::default(), volume_data)
                        }
                        ChartData::TickBased(tick_aggr) => {
                            let volume_data = tick_aggr
                                .data_points
                                .iter()
                                .enumerate()
                                .map(|(idx, dp)| (idx as u64, (dp.volume_buy, dp.volume_sell)))
                                .collect();

                            IndicatorData::Volume(Caches::default(), volume_data)
                        }
                    },
                    CandlestickIndicator::OpenInterest => {
                        IndicatorData::OpenInterest(Caches::default(), BTreeMap::new())
                    }
                };
                entry.insert(data);

                if self.chart.indicators_split.is_none() {
                    self.chart.indicators_split = Some(0.8);
                }
            }
        }

        if self.indicators.is_empty() {
            self.chart.indicators_split = None;
        }
    }

    pub fn view_indicators<I: Indicator>(&self, enabled: &[I]) -> Option<Element<Message>> {
        let chart_state = self.get_common_data();

        let visible_region = chart_state.visible_region(chart_state.bounds.size());
        let (earliest, latest) = chart_state.get_interval_range(visible_region);

        let mut indicators: iced::widget::Column<'_, Message> = column![];

        for indicator in I::get_enabled(
            enabled,
            chart_state.ticker_info.map(|info| info.get_market_type()),
        ) {
            if let Some(candlestick_indicator) =
                indicator.as_any().downcast_ref::<CandlestickIndicator>()
            {
                match candlestick_indicator {
                    CandlestickIndicator::Volume => {
                        if let Some(IndicatorData::Volume(cache, data)) =
                            self.indicators.get(&CandlestickIndicator::Volume)
                        {
                            indicators = indicators.push(indicator::volume::create_indicator_elem(
                                chart_state,
                                cache,
                                data,
                                earliest,
                                latest,
                            ));
                        }
                    }
                    CandlestickIndicator::OpenInterest => {
                        if let Some(IndicatorData::OpenInterest(cache, data)) =
                            self.indicators.get(&CandlestickIndicator::OpenInterest)
                        {
                            indicators =
                                indicators.push(indicator::open_interest::create_indicator_elem(
                                    chart_state,
                                    cache,
                                    data,
                                    earliest,
                                    latest,
                                ));
                        }
                    }
                }
            }
        }

        Some(
            container(indicators)
                .width(Length::FillPortion(10))
                .height(Length::Fill)
                .into(),
        )
    }

    pub fn update(&mut self, message: &Message) {
        self.update_chart(message);
    }

    pub fn view<'a, I: Indicator>(
        &'a self,
        indicators: &'a [I],
        timezone: UserTimezone,
    ) -> Element<'a, Message> {
        view_chart(self, indicators, timezone)
    }
}

impl canvas::Program<Message> for CandlestickChart {
    type State = Interaction;

    fn update(
        &self,
        interaction: &mut Interaction,
        event: &Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        self.canvas_interaction(interaction, event, bounds, cursor)
    }

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        theme: &Theme,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let chart = self.get_common_data();

        if chart.bounds.width == 0.0 {
            return vec![];
        }

        let center = Vector::new(bounds.width / 2.0, bounds.height / 2.0);
        let bounds_size = bounds.size();

        let candle_width = chart.cell_width * 0.8;

        let palette = theme.extended_palette();

        let candlesticks = chart.cache.main.draw(renderer, bounds_size, |frame| {
            frame.translate(center);
            frame.scale(chart.scaling);
            frame.translate(chart.translation);

            let region = chart.visible_region(frame.size());

            let (earliest, latest) = chart.get_interval_range(region);
            let price_to_y = |price: f32| chart.price_to_y(price);

            match &self.data_source {
                ChartData::TickBased(tick_aggr) => {
                    let earliest = earliest as usize;
                    let latest = latest as usize;

                    tick_aggr
                        .data_points
                        .iter()
                        .rev()
                        .enumerate()
                        .filter(|(index, _)| *index <= latest && *index >= earliest)
                        .for_each(|(index, tick_aggr)| {
                            let x_position = chart.interval_to_x(index as u64);

                            let kline = Kline {
                                time: tick_aggr.start_timestamp,
                                open: tick_aggr.open_price,
                                high: tick_aggr.high_price,
                                low: tick_aggr.low_price,
                                close: tick_aggr.close_price,
                                volume: (tick_aggr.volume_buy, tick_aggr.volume_sell),
                            };

                            draw_data_point(
                                frame,
                                price_to_y,
                                candle_width,
                                palette,
                                x_position,
                                &kline,
                            );
                        });
                }
                ChartData::TimeBased(timeseries) => {
                    if latest < earliest {
                        return;
                    }

                    timeseries
                        .data_points
                        .range(earliest..=latest)
                        .for_each(|(timestamp, dp)| {
                            let x_position = chart.interval_to_x(*timestamp);

                            draw_data_point(
                                frame,
                                price_to_y,
                                candle_width,
                                palette,
                                x_position,
                                &dp.kline,
                            );
                        });
                }
            }

            // last price line
            if let Some(price) = &chart.last_price {
                let (mut y_pos, line_color) = price.get_with_color(palette);
                y_pos = chart.price_to_y(y_pos);

                let marker_line = Stroke::with_color(
                    Stroke {
                        width: 1.0,
                        line_dash: LineDash {
                            segments: &[2.0, 2.0],
                            offset: 4,
                        },
                        ..Default::default()
                    },
                    line_color.scale_alpha(0.5),
                );

                frame.stroke(
                    &Path::line(
                        Point::new(0.0, y_pos),
                        Point::new(region.x + region.width, y_pos),
                    ),
                    marker_line,
                );
            };
        });

        if chart.crosshair {
            let crosshair = chart.cache.crosshair.draw(renderer, bounds_size, |frame| {
                if let Some(cursor_position) = cursor.position_in(bounds) {
                    let (_, rounded_aggregation) =
                        chart.draw_crosshair(frame, theme, bounds_size, cursor_position);

                    CommonChartData::draw_crosshair_tooltip(
                        &self.data_source,
                        frame,
                        palette,
                        rounded_aggregation,
                    );
                }
            });

            vec![candlesticks, crosshair]
        } else {
            vec![candlesticks]
        }
    }

    fn mouse_interaction(
        &self,
        interaction: &Interaction,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        match interaction {
            Interaction::Panning { .. } => mouse::Interaction::Grabbing,
            Interaction::Zoomin { .. } => mouse::Interaction::ZoomIn,
            Interaction::None => {
                if cursor.is_over(bounds) && self.chart.crosshair {
                    return mouse::Interaction::Crosshair;
                }
                mouse::Interaction::default()
            }
        }
    }
}

fn draw_data_point(
    frame: &mut canvas::Frame,
    price_to_y: impl Fn(f32) -> f32,
    candle_width: f32,
    palette: &Extended,
    x_position: f32,
    kline: &Kline,
) {
    let y_open = price_to_y(kline.open);
    let y_high = price_to_y(kline.high);
    let y_low = price_to_y(kline.low);
    let y_close = price_to_y(kline.close);

    let body_color = if kline.close >= kline.open {
        palette.success.base.color
    } else {
        palette.danger.base.color
    };
    frame.fill_rectangle(
        Point::new(x_position - (candle_width / 2.0), y_open.min(y_close)),
        Size::new(candle_width, (y_open - y_close).abs()),
        body_color,
    );

    let wick_color = if kline.close >= kline.open {
        palette.success.base.color
    } else {
        palette.danger.base.color
    };
    frame.fill_rectangle(
        Point::new(x_position - (candle_width / 8.0), y_high),
        Size::new(candle_width / 4.0, (y_high - y_low).abs()),
        wick_color,
    );
}
