use std::collections::BTreeMap;

use iced::widget::canvas::{self, Cache, Event, Geometry, LineDash, Path, Stroke};
use iced::widget::{Canvas, container, row};
use iced::{Element, Length};
use iced::{Point, Rectangle, Renderer, Size, Theme, Vector, mouse};

use crate::charts::{Caches, ChartBasis, CommonChartData, Interaction, Message, round_to_tick};
use crate::data_providers::format_with_commas;

pub fn create_indicator_elem<'a>(
    chart_state: &'a CommonChartData,
    cache: &'a Caches,
    data_points: &'a BTreeMap<u64, (f32, f32)>,
    earliest: u64,
    latest: u64,
) -> Element<'a, Message> {
    let max_volume = {
        match chart_state.basis {
            ChartBasis::Time(_) => data_points
                .range(earliest..=latest)
                .map(|(_, (buy, sell))| buy.max(*sell))
                .max_by(|a, b| a.partial_cmp(b).unwrap())
                .unwrap_or(0.0),
            ChartBasis::Tick(_) => {
                let mut max_volume: f32 = 0.0;
                let earliest = earliest as usize;
                let latest = latest as usize;

                data_points
                    .iter()
                    .rev()
                    .enumerate()
                    .filter(|(index, _)| *index <= latest && *index >= earliest)
                    .for_each(|(_, (_, (buy_volume, sell_volume)))| {
                        max_volume = max_volume.max(buy_volume.max(*sell_volume));
                    });

                max_volume
            }
        }
    };

    let indi_chart = Canvas::new(VolumeIndicator {
        indicator_cache: &cache.main,
        crosshair_cache: &cache.crosshair,
        crosshair: chart_state.crosshair,
        x_max: chart_state.latest_x,
        scaling: chart_state.scaling,
        translation_x: chart_state.translation.x,
        basis: chart_state.basis,
        cell_width: chart_state.cell_width,
        data_points,
        chart_bounds: chart_state.bounds,
        max_volume,
    })
    .height(Length::Fill)
    .width(Length::Fill);

    let indi_labels = Canvas::new(super::IndicatorLabel {
        label_cache: &cache.y_labels,
        max: max_volume,
        min: 0.0,
        crosshair: chart_state.crosshair,
        chart_bounds: chart_state.bounds,
    })
    .height(Length::Fill)
    .width(Length::Fixed(60.0 + (chart_state.decimals as f32 * 2.0)));

    row![indi_chart, container(indi_labels),].into()
}

pub struct VolumeIndicator<'a> {
    pub indicator_cache: &'a Cache,
    pub crosshair_cache: &'a Cache,
    pub crosshair: bool,
    pub x_max: u64,
    pub max_volume: f32,
    pub scaling: f32,
    pub translation_x: f32,
    pub basis: ChartBasis,
    pub cell_width: f32,
    pub data_points: &'a BTreeMap<u64, (f32, f32)>,
    pub chart_bounds: Rectangle,
}

impl VolumeIndicator<'_> {
    fn visible_region(&self, size: Size) -> Rectangle {
        let width = size.width / self.scaling;
        let height = size.height / self.scaling;

        Rectangle {
            x: -self.translation_x - width / 2.0,
            y: 0.0,
            width,
            height,
        }
    }

    fn get_interval_range(&self, region: Rectangle) -> (u64, u64) {
        match self.basis {
            ChartBasis::Tick(_) => (
                self.x_to_interval(region.x + region.width),
                self.x_to_interval(region.x),
            ),
            ChartBasis::Time(interval) => (
                self.x_to_interval(region.x).saturating_sub(interval / 2),
                self.x_to_interval(region.x + region.width)
                    .saturating_add(interval / 2),
            ),
        }
    }

    fn x_to_interval(&self, x: f32) -> u64 {
        match self.basis {
            ChartBasis::Time(interval) => {
                if x <= 0.0 {
                    let diff = (-x / self.cell_width * interval as f32) as u64;
                    self.x_max.saturating_sub(diff)
                } else {
                    let diff = (x / self.cell_width * interval as f32) as u64;
                    self.x_max.saturating_add(diff)
                }
            }
            ChartBasis::Tick(_) => {
                let tick = -(x / self.cell_width);
                tick.round() as u64
            }
        }
    }

    fn interval_to_x(&self, value: u64) -> f32 {
        match self.basis {
            ChartBasis::Time(interval) => {
                if value <= self.x_max {
                    let diff = self.x_max - value;
                    -(diff as f32 / interval as f32) * self.cell_width
                } else {
                    let diff = value - self.x_max;
                    (diff as f32 / interval as f32) * self.cell_width
                }
            }
            ChartBasis::Tick(_) => -((value as f32) * self.cell_width),
        }
    }
}

impl canvas::Program<Message> for VolumeIndicator<'_> {
    type State = Interaction;

    fn update(
        &self,
        interaction: &mut Interaction,
        event: &Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        match event {
            Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                let message = match *interaction {
                    Interaction::None => {
                        if self.crosshair && cursor.is_over(bounds) {
                            Some(Message::CrosshairMoved)
                        } else {
                            None
                        }
                    }
                    _ => None,
                };

                let action =
                    message.map_or(canvas::Action::request_redraw(), canvas::Action::publish);

                Some(match interaction {
                    Interaction::None => action,
                    _ => action.and_capture(),
                })
            }
            _ => None,
        }
    }

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        theme: &Theme,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let center = Vector::new(bounds.width / 2.0, bounds.height / 2.0);

        let palette = theme.extended_palette();

        let max_volume = self.max_volume;

        let indicator = self.indicator_cache.draw(renderer, bounds.size(), |frame| {
            frame.translate(center);
            frame.scale(self.scaling);
            frame.translate(Vector::new(
                self.translation_x,
                (-bounds.height / self.scaling) / 2.0,
            ));

            let region = self.visible_region(frame.size());

            let (earliest, latest) = self.get_interval_range(region);

            match self.basis {
                ChartBasis::Time(_) => {
                    if latest < earliest {
                        return;
                    }

                    self.data_points.range(earliest..=latest).for_each(
                        |(timestamp, (buy_volume, sell_volume))| {
                            let x_position = self.interval_to_x(*timestamp);

                            if max_volume > 0.0 {
                                if *buy_volume != -1.0 {
                                    let buy_bar_height =
                                        (buy_volume / max_volume) * (bounds.height / self.scaling);
                                    let sell_bar_height =
                                        (sell_volume / max_volume) * (bounds.height / self.scaling);

                                    let bar_width = (self.cell_width / 2.0) * 0.9;

                                    frame.fill_rectangle(
                                        Point::new(
                                            x_position - bar_width,
                                            (region.y + region.height) - sell_bar_height,
                                        ),
                                        Size::new(bar_width, sell_bar_height),
                                        palette.danger.base.color,
                                    );

                                    frame.fill_rectangle(
                                        Point::new(
                                            x_position,
                                            (region.y + region.height) - buy_bar_height,
                                        ),
                                        Size::new(bar_width, buy_bar_height),
                                        palette.success.base.color,
                                    );
                                } else {
                                    let bar_height =
                                        (sell_volume / max_volume) * (bounds.height / self.scaling);

                                    let bar_width = self.cell_width * 0.9;

                                    frame.fill_rectangle(
                                        Point::new(
                                            x_position - (bar_width / 2.0),
                                            (bounds.height / self.scaling) - bar_height,
                                        ),
                                        Size::new(bar_width, bar_height),
                                        palette.secondary.strong.color,
                                    );
                                }
                            }
                        },
                    );
                }
                ChartBasis::Tick(_) => {
                    let earliest = earliest as usize;
                    let latest = latest as usize;

                    self.data_points
                        .iter()
                        .rev()
                        .enumerate()
                        .filter(|(index, _)| *index <= latest && *index >= earliest)
                        .for_each(|(index, (_, (buy_volume, sell_volume)))| {
                            let x_position = self.interval_to_x(index as u64);

                            if max_volume > 0.0 {
                                let buy_bar_height =
                                    (buy_volume / max_volume) * (bounds.height / self.scaling);
                                let sell_bar_height =
                                    (sell_volume / max_volume) * (bounds.height / self.scaling);

                                let bar_width = (self.cell_width / 2.0) * 0.9;

                                frame.fill_rectangle(
                                    Point::new(
                                        x_position - bar_width,
                                        (region.y + region.height) - sell_bar_height,
                                    ),
                                    Size::new(bar_width, sell_bar_height),
                                    palette.danger.base.color,
                                );

                                frame.fill_rectangle(
                                    Point::new(
                                        x_position,
                                        (region.y + region.height) - buy_bar_height,
                                    ),
                                    Size::new(bar_width, buy_bar_height),
                                    palette.success.base.color,
                                );
                            }
                        });
                }
            }
        });

        if self.crosshair {
            let crosshair = self.crosshair_cache.draw(renderer, bounds.size(), |frame| {
                let dashed_line = Stroke::with_color(
                    Stroke {
                        width: 1.0,
                        line_dash: LineDash {
                            segments: &[4.0, 4.0],
                            offset: 8,
                        },
                        ..Default::default()
                    },
                    palette
                        .secondary
                        .strong
                        .color
                        .scale_alpha(if palette.is_dark { 0.6 } else { 1.0 }),
                );

                if let Some(cursor_position) = cursor.position_in(self.chart_bounds) {
                    let region = self.visible_region(frame.size());

                    // Vertical time line
                    let earliest = self.x_to_interval(region.x) as f64;
                    let latest = self.x_to_interval(region.x + region.width) as f64;

                    let crosshair_ratio = f64::from(cursor_position.x / bounds.width);

                    let (rounded_interval, snap_ratio) = match self.basis {
                        ChartBasis::Time(timeframe) => {
                            let crosshair_millis = earliest + crosshair_ratio * (latest - earliest);

                            let rounded_timestamp =
                                (crosshair_millis / (timeframe as f64)).round() as u64 * timeframe;
                            let snap_ratio = ((rounded_timestamp as f64 - earliest)
                                / (latest - earliest))
                                as f32;

                            (rounded_timestamp, snap_ratio)
                        }
                        ChartBasis::Tick(_) => {
                            let chart_x_min = region.x;
                            let chart_x_max = region.x + region.width;

                            let crosshair_pos = chart_x_min + crosshair_ratio as f32 * region.width;

                            let cell_index = (crosshair_pos / self.cell_width).round() as i32;
                            let snapped_position = cell_index as f32 * self.cell_width;

                            let snap_ratio =
                                (snapped_position - chart_x_min) / (chart_x_max - chart_x_min);

                            let tick_value = self.x_to_interval(snapped_position);

                            (tick_value, snap_ratio)
                        }
                    };

                    frame.stroke(
                        &Path::line(
                            Point::new(snap_ratio * bounds.width, 0.0),
                            Point::new(snap_ratio * bounds.width, bounds.height),
                        ),
                        dashed_line,
                    );

                    if let Some((_, (buy_v, sell_v))) = match self.basis {
                        ChartBasis::Time(_) => self
                            .data_points
                            .iter()
                            .find(|(interval, _)| **interval == rounded_interval),
                        ChartBasis::Tick(_) => {
                            let index_from_end = rounded_interval as usize;

                            if index_from_end < self.data_points.len() {
                                self.data_points.iter().rev().nth(index_from_end)
                            } else {
                                None
                            }
                        }
                    } {
                        let mut tooltip_bg_height = 28.0;

                        let tooltip_text: String = if *buy_v != -1.0 {
                            format!(
                                "Buy Volume: {}\nSell Volume: {}",
                                format_with_commas(*buy_v),
                                format_with_commas(*sell_v),
                            )
                        } else {
                            tooltip_bg_height = 14.0;

                            format!("Volume: {}", format_with_commas(*sell_v),)
                        };

                        let text = canvas::Text {
                            content: tooltip_text,
                            position: Point::new(8.0, 2.0),
                            size: iced::Pixels(10.0),
                            color: palette.background.base.text,
                            ..canvas::Text::default()
                        };
                        frame.fill_text(text);

                        frame.fill_rectangle(
                            Point::new(4.0, 0.0),
                            Size::new(140.0, tooltip_bg_height),
                            palette.background.base.color,
                        );
                    }
                } else if let Some(cursor_position) = cursor.position_in(bounds) {
                    // Horizontal price line
                    let highest = max_volume;
                    let lowest = 0.0;

                    let crosshair_ratio = cursor_position.y / bounds.height;
                    let crosshair_price = highest + crosshair_ratio * (lowest - highest);

                    let rounded_price = round_to_tick(crosshair_price, 1.0);
                    let snap_ratio = (rounded_price - highest) / (lowest - highest);

                    frame.stroke(
                        &Path::line(
                            Point::new(0.0, snap_ratio * bounds.height),
                            Point::new(bounds.width, snap_ratio * bounds.height),
                        ),
                        dashed_line,
                    );
                }
            });

            vec![indicator, crosshair]
        } else {
            vec![indicator]
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
            Interaction::None if cursor.is_over(bounds) => {
                if self.crosshair {
                    mouse::Interaction::Crosshair
                } else {
                    mouse::Interaction::default()
                }
            }
            _ => mouse::Interaction::default(),
        }
    }
}
