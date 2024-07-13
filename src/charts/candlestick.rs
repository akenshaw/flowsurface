use std::collections::BTreeMap;
use iced::{
    alignment, mouse, widget::{button, canvas::{self, event::{self, Event}, stroke::Stroke, Cache, Canvas, Geometry, Path}}, Color, Element, Length, Point, Rectangle, Renderer, Size, Theme
};
use iced::widget::{Column, Row, Container, Text};
use crate::{market_data::Kline, Timeframe};

use super::{Chart, CommonChartData, Message, Interaction, AxisLabelXCanvas, AxisLabelYCanvas};
use super::{chart_button, calculate_price_step, calculate_time_step};

pub struct CandlestickChart {
    chart: CommonChartData,
    data_points: BTreeMap<i64, (f32, f32, f32, f32, f32, f32)>,
    timeframe: u16,
    mesh_cache: Cache,
}

impl Chart for CandlestickChart {
    type DataPoint = BTreeMap<i64, (f32, f32, f32, f32, f32, f32)>;

    fn get_common_data(&self) -> &CommonChartData {
        &self.chart
    }
    fn get_common_data_mut(&mut self) -> &mut CommonChartData {
        &mut self.chart
    }
}

impl CandlestickChart {
    const MIN_SCALING: f32 = 0.1;
    const MAX_SCALING: f32 = 2.0;

    pub fn new(klines: Vec<Kline>, timeframe: Timeframe) -> CandlestickChart {
        let mut klines_raw = BTreeMap::new();

        for kline in klines {
            let buy_volume = kline.taker_buy_base_asset_volume;
            let sell_volume = kline.volume - buy_volume;
            klines_raw.insert(kline.time as i64, (kline.open, kline.high, kline.low, kline.close, buy_volume, sell_volume));
        }

        let timeframe = match timeframe {
            Timeframe::M1 => 1,
            Timeframe::M3 => 3,
            Timeframe::M5 => 5,
            Timeframe::M15 => 15,
            Timeframe::M30 => 30,
        };

        CandlestickChart {
            chart: CommonChartData::default(),
            data_points: klines_raw,
            timeframe,
            mesh_cache: Cache::default(),
        }
    }

    pub fn insert_datapoint(&mut self, kline: &Kline) {
        let buy_volume: f32 = kline.taker_buy_base_asset_volume;
        let sell_volume: f32 = if buy_volume != -1.0 {
            kline.volume - buy_volume
        } else {
            kline.volume
        };

        self.data_points.insert(kline.time as i64, (kline.open, kline.high, kline.low, kline.close, buy_volume, sell_volume));

        self.render_start();
    }

    pub fn render_start(&mut self) {
        let (latest, earliest, highest, lowest) = self.calculate_range();

        if latest == 0 || highest == 0.0 {
            return;
        }

        let chart_state = &mut self.chart;

        if earliest != chart_state.x_min_time || latest != chart_state.x_max_time || lowest != chart_state.y_min_price || highest != chart_state.y_max_price {
            chart_state.x_labels_cache.clear();
            self.mesh_cache.clear();
        }

        chart_state.x_min_time = earliest;
        chart_state.x_max_time = latest;
        chart_state.y_min_price = lowest;
        chart_state.y_max_price = highest;

        chart_state.y_labels_cache.clear();
        chart_state.crosshair_cache.clear();

        chart_state.main_cache.clear();
    }

    fn calculate_range(&self) -> (i64, i64, f32, f32) {
        let chart = self.get_common_data();

        let latest: i64 = self.data_points.keys().last().map_or(0, |time| time - ((chart.translation.x*10000.0)*(self.timeframe as f32)) as i64);
        let earliest: i64 = latest - ((6400000.0*self.timeframe as f32) / (chart.scaling / (chart.bounds.width/800.0))) as i64;

        let (visible_klines, highest, lowest, avg_body_height, _, _) = self.data_points.iter()
            .filter(|(time, _)| {
                **time >= earliest && **time <= latest
            })
            .fold((vec![], f32::MIN, f32::MAX, 0.0f32, 0.0f32, None), |(mut klines, highest, lowest, total_body_height, max_vol, latest_kline), (time, kline)| {
                let body_height = (kline.0 - kline.3).abs();
                klines.push((*time, *kline));
                let total_body_height = match latest_kline {
                    Some(_) => total_body_height + body_height,
                    None => total_body_height,
                };
                (
                    klines,
                    highest.max(kline.1),
                    lowest.min(kline.2),
                    total_body_height,
                    max_vol.max(kline.4.max(kline.5)),
                    Some(kline)
                )
            });

        if visible_klines.is_empty() || visible_klines.len() == 1 {
            return (0, 0, 0.0, 0.0);
        }

        let avg_body_height = avg_body_height / (visible_klines.len() - 1) as f32;
        let (highest, lowest) = (highest + avg_body_height, lowest - avg_body_height);

        (latest, earliest, highest, lowest)
    }

    pub fn update(&mut self, message: &Message) {
        match message {
            Message::Translated(translation) => {
                let chart = self.get_common_data_mut();

                if chart.autoscale {
                    chart.translation.x = translation.x;
                } else {
                    chart.translation = *translation;
                }
                chart.crosshair_position = Point::new(0.0, 0.0);

                self.render_start();
            },
            Message::Scaled(scaling, translation) => {
                let chart = self.get_common_data_mut();

                chart.scaling = *scaling;
                
                if let Some(translation) = translation {
                    if chart.autoscale {
                        chart.translation.x = translation.x;
                    } else {
                        chart.translation = *translation;
                    }
                }
                chart.crosshair_position = Point::new(0.0, 0.0);

                self.render_start();
            },
            Message::ChartBounds(bounds) => {
                self.chart.bounds = *bounds;
            },
            Message::AutoscaleToggle => {
                self.chart.autoscale = !self.chart.autoscale;
            },
            Message::CrosshairToggle => {
                self.chart.crosshair = !self.chart.crosshair;
            },
            Message::CrosshairMoved(position) => {
                let chart = self.get_common_data_mut();

                chart.crosshair_position = *position;
                if chart.crosshair {
                    chart.crosshair_cache.clear();
                    chart.y_crosshair_cache.clear();
                    chart.x_crosshair_cache.clear();
                }
            },
            _ => {}
        }
    }

    pub fn view(&self) -> Element<Message> {
        let chart = Canvas::new(self)
            .width(Length::FillPortion(10))
            .height(Length::FillPortion(10));

        let chart_state = self.get_common_data();
    
        let axis_labels_x = Canvas::new(
            AxisLabelXCanvas { 
                labels_cache: &chart_state.x_labels_cache, 
                min: chart_state.x_min_time, 
                max: chart_state.x_max_time, 
                crosshair_cache: &chart_state.x_crosshair_cache, 
                crosshair_position: chart_state.crosshair_position, 
                crosshair: chart_state.crosshair,
                timeframe: self.timeframe
            })
            .width(Length::FillPortion(10))
            .height(Length::Fixed(26.0));
    
        let axis_labels_y = Canvas::new(
            AxisLabelYCanvas { 
                labels_cache: &chart_state.y_labels_cache, 
                y_croshair_cache: &chart_state.y_crosshair_cache, 
                min: chart_state.y_min_price,
                max: chart_state.y_max_price,
                crosshair_position: chart_state.crosshair_position, 
                crosshair: chart_state.crosshair
            })
            .width(Length::Fixed(60.0))
            .height(Length::FillPortion(10));

        let autoscale_button = button(
            Text::new("A")
                .size(12)
                .horizontal_alignment(alignment::Horizontal::Center)
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .on_press(Message::AutoscaleToggle)
            .style(|_theme: &Theme, _status: iced::widget::button::Status| chart_button(_theme, _status, chart_state.autoscale));
        let crosshair_button = button(
            Text::new("+")
                .size(12)
                .horizontal_alignment(alignment::Horizontal::Center)
            ) 
            .width(Length::Fill)
            .height(Length::Fill)
            .on_press(Message::CrosshairToggle)
            .style(|_theme: &Theme, _status: iced::widget::button::Status| chart_button(_theme, _status, chart_state.crosshair));
    
        let chart_controls = Container::new(
            Row::new()
                .push(autoscale_button)
                .push(crosshair_button).spacing(2)
            ).padding([0, 2, 0, 2])
            .width(Length::Fixed(60.0))
            .height(Length::Fixed(26.0));

        let chart_and_y_labels = Row::new()
            .push(chart)
            .push(axis_labels_y);
    
        let bottom_row = Row::new()
            .push(axis_labels_x)
            .push(chart_controls);
    
        let content = Column::new()
            .push(chart_and_y_labels)
            .push(bottom_row)
            .spacing(0)
            .padding(5);
    
        content.into()
    }
}

impl canvas::Program<Message> for CandlestickChart {
    type State = Interaction;

    fn update(
        &self,
        interaction: &mut Interaction,
        event: Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> (event::Status, Option<Message>) {  
        let chart_state = self.get_common_data();

        if bounds != chart_state.bounds {
            return (event::Status::Ignored, Some(Message::ChartBounds(bounds)));
        } 
        
        if let Event::Mouse(mouse::Event::ButtonReleased(_)) = event {
            *interaction = Interaction::None;
        }

        let Some(cursor_position) = cursor.position_in(bounds) else {
            return (event::Status::Ignored, 
                if chart_state.crosshair {
                    Some(Message::CrosshairMoved(Point::new(0.0, 0.0)))
                } else {
                    None
                }
                );
        };

        match event {
            Event::Mouse(mouse_event) => match mouse_event {
                mouse::Event::ButtonPressed(button) => {
                    let message = match button {
                        mouse::Button::Right => {
                            *interaction = Interaction::Drawing;
                            None
                        }
                        mouse::Button::Left => {
                            *interaction = Interaction::Panning {
                                translation: chart_state.translation,
                                start: cursor_position,
                            };
                            None
                        }
                        _ => None,
                    };

                    (event::Status::Captured, message)
                }
                mouse::Event::CursorMoved { .. } => {
                    let message = match *interaction {
                        Interaction::Drawing => None,
                        Interaction::Erasing => None,
                        Interaction::Panning { translation, start } => {
                            Some(Message::Translated(
                                translation
                                    + (cursor_position - start)
                                        * (1.0 / chart_state.scaling),
                            ))
                        }
                        Interaction::None => 
                            if chart_state.crosshair && cursor.is_over(bounds) {
                                Some(Message::CrosshairMoved(cursor_position))
                            } else {
                                None
                            },
                    };

                    let event_status = match interaction {
                        Interaction::None => event::Status::Ignored,
                        _ => event::Status::Captured,
                    };

                    (event_status, message)
                }
                mouse::Event::WheelScrolled { delta } => match delta {
                    mouse::ScrollDelta::Lines { y, .. }
                    | mouse::ScrollDelta::Pixels { y, .. } => {
                        if y < 0.0 && chart_state.scaling > Self::MIN_SCALING
                            || y > 0.0 && chart_state.scaling < Self::MAX_SCALING
                        {
                            //let old_scaling = self.scaling;

                            let scaling = (chart_state.scaling * (1.0 + y / 30.0))
                                .clamp(
                                    Self::MIN_SCALING,  // 0.1
                                    Self::MAX_SCALING,  // 2.0
                                );

                            //let translation =
                            //    if let Some(cursor_to_center) =
                            //        cursor.position_from(bounds.center())
                            //    {
                            //        let factor = scaling - old_scaling;

                            //        Some(
                            //            self.translation
                            //                - Vector::new(
                            //                    cursor_to_center.x * factor
                            //                        / (old_scaling
                            //                            * old_scaling),
                            //                    cursor_to_center.y * factor
                            //                        / (old_scaling
                            //                            * old_scaling),
                            //                ),
                            //        )
                            //    } else {
                            //        None
                            //    };

                            (
                                event::Status::Captured,
                                Some(Message::Scaled(scaling, None)),
                            )
                        } else {
                            (event::Status::Captured, None)
                        }
                    }
                },
                _ => (event::Status::Ignored, None),
            },
            _ => (event::Status::Ignored, None),
        }
    }
    
    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Vec<Geometry> {    
        let chart = self.get_common_data();

        let latest: i64 = self.data_points.keys().last().map_or(0, |time| time - ((chart.translation.x*10000.0)*(self.timeframe as f32)) as i64);
        let earliest: i64 = latest - ((6400000.0*self.timeframe as f32) / (chart.scaling / (bounds.width/800.0))) as i64;

        let (visible_klines, highest, lowest, avg_body_height, max_volume, _) = self.data_points.iter()
            .filter(|(time, _)| {
                **time >= earliest && **time <= latest
            })
            .fold((vec![], f32::MIN, f32::MAX, 0.0f32, 0.0f32, None), |(mut klines, highest, lowest, total_body_height, max_vol, latest_kline), (time, kline)| {
                let body_height = (kline.0 - kline.3).abs();
                klines.push((*time, *kline));
                let total_body_height = match latest_kline {
                    Some(_) => total_body_height + body_height,
                    None => total_body_height,
                };
                (
                    klines,
                    highest.max(kline.1),
                    lowest.min(kline.2),
                    total_body_height,
                    max_vol.max(kline.4.max(kline.5)),
                    Some(kline)
                )
            });

        if visible_klines.is_empty() || visible_klines.len() == 1 {
            return vec![];
        }

        let avg_body_height = avg_body_height / (visible_klines.len() - 1) as f32;
        let (highest, lowest) = (highest + avg_body_height, lowest - avg_body_height);
        let y_range = highest - lowest;

        let volume_area_height = bounds.height / 8.0; 
        let candlesticks_area_height = bounds.height - volume_area_height;

        let y_labels_can_fit = (bounds.height / 32.0) as i32;
        let (step, rounded_lowest) = calculate_price_step(highest, lowest, y_labels_can_fit);

        let x_labels_can_fit = (bounds.width / 90.0) as i32;
        let (time_step, rounded_earliest) = calculate_time_step(earliest, latest, x_labels_can_fit, self.timeframe);

        let background = self.mesh_cache.draw(renderer, bounds.size(), |frame| {
            frame.with_save(|frame| {
                let mut time = rounded_earliest;

                while time <= latest {                    
                    let x_position = ((time - earliest) as f64 / (latest - earliest) as f64) * bounds.width as f64;

                    if x_position >= 0.0 && x_position <= bounds.width as f64 {
                        let line = Path::line(
                            Point::new(x_position as f32, 0.0), 
                            Point::new(x_position as f32, bounds.height)
                        );
                        frame.stroke(&line, Stroke::default().with_color(Color::from_rgba8(27, 27, 27, 1.0)).with_width(1.0))
                    };
                    
                    time += time_step;
                }
            });
            
            frame.with_save(|frame| {
                let mut y = rounded_lowest;

                while y <= highest {
                    let y_position = candlesticks_area_height - ((y - lowest) / y_range * candlesticks_area_height);
                    let line = Path::line(
                        Point::new(0.0, y_position), 
                        Point::new(bounds.width, y_position)
                    );
                    frame.stroke(&line, Stroke::default().with_color(Color::from_rgba8(27, 27, 27, 1.0)).with_width(1.0));
                    y += step;
                }
            });
        });

        let candlesticks = chart.main_cache.draw(renderer, bounds.size(), |frame| {
            for (time, (open, high, low, close, buy_volume, sell_volume)) in visible_klines {
                let x_position: f64 = ((time - earliest) as f64 / (latest - earliest) as f64) * bounds.width as f64;
                
                let y_open = candlesticks_area_height - ((open - lowest) / y_range * candlesticks_area_height);
                let y_high = candlesticks_area_height - ((high - lowest) / y_range * candlesticks_area_height);
                let y_low = candlesticks_area_height - ((low - lowest) / y_range * candlesticks_area_height);
                let y_close = candlesticks_area_height - ((close - lowest) / y_range * candlesticks_area_height);
                
                let color = if close >= open { Color::from_rgb8(81, 205, 160) } else { Color::from_rgb8(192, 80, 77) };

                let body = Path::rectangle(
                    Point::new(x_position as f32 - (2.0 * chart.scaling), y_open.min(y_close)), 
                    Size::new(4.0 * chart.scaling, (y_open - y_close).abs())
                );                    
                frame.fill(&body, color);
                
                let wick = Path::line(
                    Point::new(x_position as f32, y_high), 
                    Point::new(x_position as f32, y_low)
                );
                frame.stroke(&wick, Stroke::default().with_color(color).with_width(1.0));

                if buy_volume != -1.0 {
                    let buy_bar_height = (buy_volume / max_volume) * volume_area_height;
                    let sell_bar_height = (sell_volume / max_volume) * volume_area_height;
                    
                    let buy_bar = Path::rectangle(
                        Point::new(x_position as f32, bounds.height - buy_bar_height), 
                        Size::new(2.0 * chart.scaling, buy_bar_height)
                    );
                    frame.fill(&buy_bar, Color::from_rgb8(81, 205, 160)); 
                    
                    let sell_bar = Path::rectangle(
                        Point::new(x_position as f32 - (2.0 * chart.scaling), bounds.height - sell_bar_height), 
                        Size::new(2.0 * chart.scaling, sell_bar_height)
                    );
                    frame.fill(&sell_bar, Color::from_rgb8(192, 80, 77)); 
                } else {
                    let bar_height = ((sell_volume) / max_volume) * volume_area_height;
                    
                    let bar = Path::rectangle(
                        Point::new(x_position as f32 - (2.0 * chart.scaling), bounds.height - bar_height), 
                        Size::new(4.0 * chart.scaling, bar_height)
                    );
                    let color = if close >= open { Color::from_rgba8(81, 205, 160, 0.8) } else { Color::from_rgba8(192, 80, 77, 0.8) };

                    frame.fill(&bar, color);
                }
            }
        });

        if chart.crosshair {
            let crosshair = chart.crosshair_cache.draw(renderer, bounds.size(), |frame| {
                if let Some(cursor_position) = cursor.position_in(bounds) {
                    let line = Path::line(
                        Point::new(0.0, cursor_position.y), 
                        Point::new(bounds.width, cursor_position.y)
                    );
                    frame.stroke(&line, Stroke::default().with_color(Color::from_rgba8(200, 200, 200, 0.6)).with_width(1.0));

                    let crosshair_ratio = cursor_position.x as f64 / bounds.width as f64;
                    let crosshair_millis = earliest as f64 + crosshair_ratio * (latest - earliest) as f64;
                    let rounded_timestamp = (crosshair_millis / (self.timeframe as f64 * 60.0 * 1000.0)).round() as i64 * self.timeframe as i64 * 60 * 1000;

                    let snap_ratio = (rounded_timestamp as f64 - earliest as f64) / (latest as f64 - earliest as f64);
                    let snap_x = snap_ratio * bounds.width as f64;

                    let line = Path::line(
                        Point::new(snap_x as f32, 0.0), 
                        Point::new(snap_x as f32, bounds.height)
                    );
                    frame.stroke(&line, Stroke::default().with_color(Color::from_rgba8(200, 200, 200, 0.6)).with_width(1.0));

                    if let Some((_, kline)) = self.data_points.iter()
                        .find(|(time, _)| **time == rounded_timestamp as i64) {

                        
                        let tooltip_text: String = if kline.4 != -1.0 {
                            format!(
                                "O: {} H: {} L: {} C: {}\nBuyV: {:.0} SellV: {:.0}",
                                kline.0, kline.1, kline.2, kline.3, kline.4, kline.5
                            )
                        } else {
                            format!(
                                "O: {} H: {} L: {} C: {}\nVolume: {:.0}",
                                kline.0, kline.1, kline.2, kline.3, kline.5
                            )
                        };

                        let text = canvas::Text {
                            content: tooltip_text,
                            position: Point::new(10.0, 10.0),
                            size: iced::Pixels(12.0),
                            color: Color::from_rgba8(120, 120, 120, 1.0),
                            ..canvas::Text::default()
                        };
                        frame.fill_text(text);
                    }
                }
            });

            vec![background, crosshair, candlesticks]
        }   else {
            vec![background, candlesticks]
        }
    }

    fn mouse_interaction(
        &self,
        interaction: &Interaction,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        match interaction {
            Interaction::Drawing => mouse::Interaction::Crosshair,
            Interaction::Erasing => mouse::Interaction::Crosshair,
            Interaction::Panning { .. } => mouse::Interaction::Grabbing,
            Interaction::None if cursor.is_over(bounds) => {
                if self.chart.crosshair {
                    mouse::Interaction::Crosshair
                } else {
                    mouse::Interaction::default()
                }
            }
            Interaction::None => { mouse::Interaction::default() }
        }
    }
}