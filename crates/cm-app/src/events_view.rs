//! Career and special-event browsing, with persistent, result-backed progress.
use std::path::PathBuf;
use std::time::SystemTime;

use ac_core::{
    content,
    events::{self, Catalog, Event, Progress, Series},
    launch, paths,
};
use gpui::{
    AnyElement, Context, Entity, EventEmitter, FontWeight, Render, ScrollStrategy,
    UniformListScrollHandle, Window, div, prelude::*, px, rgb, uniform_list,
};

use crate::{
    text_input::TextInput,
    theme,
    widgets::{button, chip, field, panel, picture, primary_button, section_title},
};

pub struct Running(pub bool);
impl EventEmitter<Running> for EventsView {}

pub struct EventsView {
    pub root: PathBuf,
    catalog: Catalog,
    progress: Progress,
    career: bool,
    series: usize,
    selected: usize,
    search: Entity<TextInput>,
    visible: Vec<usize>,
    filter: u8,
    difficulty: usize,
    restart_confirm: bool,
    show_config: bool,
    scroll: UniformListScrollHandle,
    loading: bool,
    pub running: bool,
    error: Option<String>,
    message: Option<String>,
    driver: String,
    nationality: String,
    proton: Option<PathBuf>,
}

fn medal(n: u8) -> &'static str {
    match n {
        1 => "Bronze",
        2 => "Silver",
        3 => "Gold",
        _ => "Not earned",
    }
}
fn medal_color(n: u8) -> gpui::Rgba {
    match n {
        1 => rgb(0xcd9360),
        2 => rgb(0xc6cbd4),
        3 => rgb(0xe7bc53),
        _ => theme::muted(),
    }
}
fn line(text: impl Into<gpui::SharedString>, width: f32) -> gpui::Div {
    div().w(px(width.max(80.))).text_sm().child(text.into())
}

impl EventsView {
    pub fn new(
        root: PathBuf,
        career: bool,
        driver: String,
        nationality: String,
        proton: Option<PathBuf>,
        cx: &mut Context<Self>,
    ) -> Self {
        let search = cx.new(|cx| TextInput::new(cx, "Search events, cars, tracks…", ""));
        cx.observe(&search, |this, _, cx| {
            this.refilter(cx);
            cx.notify();
        })
        .detach();
        let mut view = Self {
            root,
            catalog: Catalog::default(),
            progress: Progress::default(),
            career,
            series: 0,
            selected: 0,
            search,
            visible: Vec::new(),
            filter: 0,
            difficulty: 2,
            restart_confirm: false,
            show_config: false,
            scroll: UniformListScrollHandle::new(),
            loading: false,
            running: false,
            error: None,
            message: None,
            driver,
            nationality,
            proton,
        };
        view.reload(cx);
        view
    }

    pub fn configure(
        &mut self,
        career: bool,
        driver: String,
        nationality: String,
        proton: Option<PathBuf>,
        cx: &mut Context<Self>,
    ) {
        self.restart_confirm = false;
        self.career = career;
        self.driver = driver;
        self.nationality = nationality;
        self.proton = proton;
        self.refilter(cx);
        cx.notify();
    }

    pub fn set_running(&mut self, running: bool, cx: &mut Context<Self>) {
        self.running = running;
        cx.notify();
    }

    fn reload(&mut self, cx: &mut Context<Self>) {
        if self.running || self.loading {
            return;
        }
        self.loading = true;
        let root = self.root.clone();
        cx.spawn(async move |this, cx| {
            let (catalog, progress) = cx.background_executor().spawn(async move {
                (events::scan(&root), Progress::load(&root))
            }).await;
            let _ = this.update(cx, |this, cx| {
                this.catalog = catalog;
                match progress {
                    Ok(p) => { this.progress = p; this.error = None; }
                    Err(e) => this.error = Some(format!("Cannot load progress: {e:#}. Launching is disabled to protect the save.")),
                }
                this.loading = false;
                this.series = this.series.min(this.catalog.series.len().saturating_sub(1));
                this.refilter(cx);
                cx.notify();
            });
        }).detach();
    }

    fn current_series(&self) -> Option<&Series> {
        self.career
            .then(|| self.catalog.series.get(self.series))
            .flatten()
    }
    fn list(&self) -> &[Event] {
        if self.career {
            self.current_series()
                .map(|s| s.events.as_slice())
                .unwrap_or(&[])
        } else {
            &self.catalog.special
        }
    }
    fn current_event(&self) -> Option<&Event> {
        self.list().get(self.selected)
    }

    fn refilter(&mut self, cx: &Context<Self>) {
        self.restart_confirm = false;
        let q = self.search.read(cx).text().to_lowercase();
        self.visible = self
            .list()
            .iter()
            .enumerate()
            .filter(|(_, e)| {
                let record = self.progress.get(&e.id);
                let earned = record.medal;
                let completed = earned > 0
                    || (self.current_series().is_some_and(|s| s.championship())
                        && record.last_position.is_some());
                let matches = q.split_whitespace().all(|word| {
                    format!("{} {} {} {}", e.name, e.car, e.track, e.description)
                        .to_lowercase()
                        .contains(word)
                });
                matches
                    && match self.filter {
                        1 => !completed,
                        2 => completed,
                        3 => earned == 3,
                        _ => true,
                    }
            })
            .map(|(i, _)| i)
            .collect();
        if !self.visible.contains(&self.selected) {
            self.selected = self.visible.first().copied().unwrap_or(usize::MAX);
        }
    }

    pub fn pad(&mut self, name: &str, cx: &mut Context<Self>) {
        match name {
            "DPAD_UP" | "DPAD_DOWN" if !self.visible.is_empty() => {
                let p = self
                    .visible
                    .iter()
                    .position(|&i| i == self.selected)
                    .unwrap_or(0);
                let next = if name == "DPAD_UP" {
                    p.saturating_sub(1)
                } else {
                    (p + 1).min(self.visible.len() - 1)
                };
                self.selected = self.visible[next];
                self.scroll.scroll_to_item(next, ScrollStrategy::Center);
            }
            "DPAD_LEFT" | "DPAD_RIGHT" if self.career && !self.catalog.series.is_empty() => {
                self.series = if name == "DPAD_LEFT" {
                    self.series.saturating_sub(1)
                } else {
                    (self.series + 1).min(self.catalog.series.len() - 1)
                };
                self.selected = 0;
                self.refilter(cx);
            }
            "X" if !self.career => self.difficulty = (self.difficulty + 1) % 3,
            "START" | "A" => self.start(cx),
            _ => {}
        }
        cx.notify();
    }

    fn start(&mut self, cx: &mut Context<Self>) {
        if self.running || self.loading || self.error.is_some() {
            return;
        }
        let Some(event) = self.current_event().cloned() else {
            return;
        };
        let series = self.current_series().cloned();
        if series.as_ref().is_some_and(|s| !s.unlocked(&self.progress)) {
            self.message = Some("Complete the required series first.".into());
            cx.notify();
            return;
        }
        if series.as_ref().is_some_and(|s| {
            s.championship() && self.progress.get(&event.id).last_position.is_some()
        }) {
            self.message = Some("This championship round is already scored. Use Restart championship to run it again.".into());
            cx.notify();
            return;
        }
        let root = self.root.clone();
        if let Err(e) = events::prepare(
            &root,
            &event,
            series.as_ref(),
            self.difficulty,
            &self.driver,
            &self.nationality,
        ) {
            self.message = Some(format!("Cannot launch: {e:#}"));
            cx.notify();
            return;
        }
        let result_path = paths::documents_ac_dir(&root).map(|d| d.join("out/race_out.json"));
        let previous = result_path
            .as_ref()
            .and_then(|p| std::fs::metadata(p).ok())
            .and_then(|m| m.modified().ok());
        let began = SystemTime::now();
        match launch::start(
            &root,
            &launch::LaunchOptions {
                proton: self.proton.clone(),
            },
        ) {
            Err(e) => {
                self.message = Some(format!("Cannot start game: {e:#}"));
                cx.notify();
            }
            Ok(mut child) => {
                self.running = true;
                self.message = Some(format!("Starting {}…", event.name));
                cx.emit(Running(true));
                cx.notify();
                cx.spawn(async move |this, cx| {
                    let exit = cx
                        .background_executor()
                        .spawn(async move { child.wait() })
                        .await;
                    let result = (|| -> anyhow::Result<serde_json::Value> {
                        let path = result_path
                            .ok_or_else(|| anyhow::anyhow!("Cannot locate race results"))?;
                        let changed = std::fs::metadata(&path)?.modified()?;
                        anyhow::ensure!(
                            Some(changed) != previous && changed >= began,
                            "No new race result; progress was not changed"
                        );
                        let data: serde_json::Value =
                            serde_json::from_slice(&std::fs::read(&path)?)?;
                        let expected_track = if event.layout.is_empty() {
                            event.track.clone()
                        } else {
                            format!("{}-{}", event.track, event.layout)
                        };
                        anyhow::ensure!(
                            data["track"].as_str() == Some(expected_track.as_str()),
                            "Result track does not match this event"
                        );
                        anyhow::ensure!(
                            data["players"][0]["car"].as_str() == Some(event.car.as_str()),
                            "Result car does not match this event"
                        );
                        Ok(data)
                    })();
                    let _ = this.update(cx, |this, cx| {
                        this.running = false;
                        cx.emit(Running(false));
                        this.message = Some(match exit {
                            Ok(code) if code.success() => match result {
                                Ok(data) => {
                                    let mut next = this.progress.clone();
                                    match next.record(&event, series.as_ref(), &data).and_then(
                                        |msg| {
                                            next.save(&root)?;
                                            Ok(msg)
                                        },
                                    ) {
                                        Ok(msg) => {
                                            this.progress = next;
                                            msg
                                        }
                                        Err(e) => format!("Progress not saved: {e:#}"),
                                    }
                                }
                                Err(e) => format!("Game closed. {e:#}"),
                            },
                            Ok(code) => format!("Game exited with {code}; progress unchanged."),
                            Err(e) => format!("Cannot wait for game: {e}"),
                        });
                        this.refilter(cx);
                        cx.notify();
                    });
                })
                .detach();
            }
        }
    }

    fn restart(&mut self, cx: &mut Context<Self>) {
        if self.running || self.error.is_some() {
            return;
        }
        let Some(series) = self.current_series().cloned() else {
            return;
        };
        if !self.restart_confirm {
            self.restart_confirm = true;
            self.message = Some("Restart clears this championship run’s points and round results. Press Confirm restart to continue.".into());
            cx.notify();
            return;
        }
        self.restart_confirm = false;
        let id = self.current_series().unwrap().id.clone();
        let ids: Vec<_> = series.events.iter().map(|e| e.id.clone()).collect();
        let mut next = self.progress.clone();
        next.championship_points.remove(&id);
        // Preserve previously earned completion/unlocks, reset only this run's rounds.
        for key in ids {
            next.events.remove(&key);
        }
        match next.save(&self.root) {
            Ok(()) => {
                self.progress = next;
                self.message =
                    Some("Championship restarted. Previous series unlocks are preserved.".into());
            }
            Err(e) => self.message = Some(format!("Cannot restart: {e:#}")),
        }
        self.refilter(cx);
        cx.notify();
    }

    fn overview(&self) -> String {
        let all: Vec<&Event> = if self.career {
            self.catalog
                .series
                .iter()
                .flat_map(|s| s.events.iter())
                .collect()
        } else {
            self.catalog.special.iter().collect()
        };
        let mut counts = [0usize; 3];
        for e in &all {
            let m = self.progress.get(&e.id).medal;
            if (1..=3).contains(&m) {
                counts[m as usize - 1] += 1;
            }
        }
        if self.career {
            format!(
                "{} / {} series complete  ·  {} events  ·  Gold {}  Silver {}  Bronze {}",
                self.catalog
                    .series
                    .iter()
                    .filter(|s| s.completed(&self.progress))
                    .count(),
                self.catalog.series.len(),
                all.len(),
                counts[2],
                counts[1],
                counts[0]
            )
        } else {
            format!(
                "{} events  ·  Gold {}  Silver {}  Bronze {}  ·  {} not earned",
                all.len(),
                counts[2],
                counts[1],
                counts[0],
                all.len() - counts.iter().sum::<usize>()
            )
        }
    }

    fn details(&self, width: f32, cx: &mut Context<Self>) -> AnyElement {
        let Some(e) = self.current_event() else {
            return div()
                .p_4()
                .text_color(theme::muted())
                .child("No events match your filter.")
                .into_any_element();
        };
        let p = self.progress.get(&e.id);
        let mut body = div().flex().flex_col().gap_3().p_4().w(px(width));
        if let Some(s) = self.current_series() {
            let counts = s.medal_counts(&self.progress);
            let goal = if s.championship() {
                format!(
                    "Championship goal: {} points{}",
                    s.goal_points,
                    if s.goal_ranking > 0 {
                        format!(" / rank {} or better", s.goal_ranking)
                    } else {
                        String::new()
                    }
                )
            } else {
                format!(
                    "Series goals: Bronze {} · Silver {} · Gold {}",
                    s.goal_medals[0], s.goal_medals[1], s.goal_medals[2]
                )
            };
            let req = s
                .requires
                .iter()
                .map(|id| {
                    self.catalog
                        .series
                        .iter()
                        .find(|a| &a.id == id)
                        .map(|s| s.name.clone())
                        .unwrap_or_else(|| id.clone())
                })
                .collect::<Vec<_>>()
                .join(", ");
            body = body.child(
                panel()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(line(s.name.clone(), width - 58.).font_weight(FontWeight::BOLD))
                    .when(self.show_config, |d| {
                        d.child(line(s.description.clone(), width - 58.))
                    })
                    .child(line(goal, width - 58.).text_color(theme::accent()))
                    .child(line(
                        format!(
                            "Earned: Bronze {} · Silver {} · Gold {}{}",
                            counts[0],
                            counts[1],
                            counts[2],
                            if s.completed(&self.progress) {
                                "  ·  Complete"
                            } else {
                                ""
                            }
                        ),
                        width - 58.,
                    ))
                    .when(!req.is_empty(), |d| {
                        d.child(
                            line(
                                format!(
                                    "Requires {}: {req}",
                                    if s.requires_any { "any" } else { "all" }
                                ),
                                width - 58.,
                            )
                            .text_color(
                                if s.unlocked(&self.progress) {
                                    theme::muted()
                                } else {
                                    theme::error()
                                },
                            ),
                        )
                    }),
            );
            if s.championship() {
                let mut standings = panel()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(section_title("Championship standings"));
                standings = standings.child(line(
                    format!(
                        "Scoring: {}",
                        s.points
                            .iter()
                            .map(u32::to_string)
                            .collect::<Vec<_>>()
                            .join(" / ")
                    ),
                    width - 58.,
                ));
                for (i, (name, points)) in s.standings(&self.progress).into_iter().enumerate() {
                    standings = standings.child(line(
                        format!(
                            "{}. {} — {points} pts",
                            i + 1,
                            if name == "player" {
                                self.driver.clone()
                            } else {
                                s.opponents
                                    .iter()
                                    .find(|o| o.id == name)
                                    .map(|o| o.name.clone())
                                    .unwrap_or(name)
                            }
                        ),
                        width - 58.,
                    ));
                }
                body = body.child(standings);
            }
        }
        if !e.goals.is_empty() {
            let mut goals = panel()
                .flex()
                .flex_col()
                .gap_2()
                .child(section_title("Medal targets"));
            for (i, g) in e.goals.iter().enumerate() {
                let n = (i + 1) as u8;
                goals = goals.child(
                    div()
                        .flex()
                        .justify_between()
                        .gap_2()
                        .child(
                            line(
                                format!("{}  ·  {}", medal(n), events::goal_label(g)),
                                width - 155.,
                            )
                            .text_color(medal_color(n)),
                        )
                        .child(
                            div()
                                .text_sm()
                                .text_color(if p.medal >= n {
                                    theme::ok()
                                } else {
                                    theme::muted()
                                })
                                .child(if p.medal >= n { "Earned" } else { "Not earned" }),
                        ),
                );
            }
            if e.goals.first().is_some_and(|g| g.kind == "AI") {
                goals = goals.child(
                    div().flex().gap_1().children(
                        (0..e.goals.len().min(3))
                            .map(|i| {
                                chip(
                                    ("event-difficulty", i),
                                    format!("{} · {}%", medal(i as u8 + 1), e.goals[i].objective),
                                    self.difficulty == i,
                                )
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.difficulty = i;
                                    cx.notify();
                                }))
                                .into_any_element()
                            })
                            .collect::<Vec<_>>(),
                    ),
                );
            }
            body = body.child(goals);
        }
        body = body.child(
            panel()
                .flex()
                .flex_col()
                .gap_1()
                .child(section_title("Your progress"))
                .child(field("Best medal", medal(p.medal)))
                .child(field("Attempts", p.attempts.to_string()))
                .child(field(
                    "Best score",
                    p.best_score
                        .map(|score| {
                            e.goals
                                .first()
                                .map(|g| {
                                    events::goal_label(&events::Goal {
                                        kind: g.kind.clone(),
                                        objective: score,
                                    })
                                })
                                .unwrap_or_else(|| score.to_string())
                        })
                        .unwrap_or_else(|| "—".into()),
                ))
                .when_some(p.last_position, |d, pos| {
                    d.child(field("Last finish", format!("Position {pos}")))
                }),
        );
        let car = content::load_car(&self.root.join("content/cars").join(&e.car));
        let track = content::load_track(&self.root.join("content/tracks").join(&e.track));
        body = body
            .child(picture(
                e.preview
                    .clone()
                    .or_else(|| car.preview().map(|p| p.to_path_buf())),
                180.,
            ))
            .child(
                line(e.name.clone(), width - 32.)
                    .text_xl()
                    .font_weight(FontWeight::BOLD),
            )
            .child(line(e.description.clone(), width - 32.).text_color(theme::muted()))
            .child(
                panel()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(field("Car", car.name))
                    .child(field(
                        "Track",
                        format!(
                                "{}{}",
                                track
                                    .layouts
                                    .iter()
                                    .find(|layout| layout.layout_id.as_deref().unwrap_or("")
                                        == e.layout)
                                    .map(|layout| layout.name.clone())
                                    .unwrap_or_else(|| track.id.clone()),
                                if e.layout.is_empty() {
                                    String::new()
                                } else {
                                    format!(" / {}", e.layout)
                                }
                            ),
                    ))
                    .child(field("Weather", e.weather.clone()))
                    .child(field(
                        "Temperature",
                        format!(
                            "Air {}°C · Road {}°C",
                            e.ambient
                                .map(|v| v.to_string())
                                .unwrap_or_else(|| "—".into()),
                            e.road.map(|v| v.to_string()).unwrap_or_else(|| "—".into())
                        ),
                    ))
                    .child(field(
                        "Rules",
                        format!(
                            "Penalties {} · Fixed setup {}",
                            if e.penalties { "on" } else { "off" },
                            if e.fixed_setup { "on" } else { "off" }
                        ),
                    ))
                    .child(field("AI strength", format!("{}%", e.ai_level))),
            );
        let mut sessions = panel()
            .flex()
            .flex_col()
            .gap_1()
            .child(section_title("Session schedule"));
        for s in &e.sessions {
            sessions = sessions.child(line(
                format!(
                    "{} · {}{}{}",
                    s.name,
                    events::mode_label(s.kind),
                    if s.laps > 0 {
                        format!(" · {} laps", s.laps)
                    } else {
                        String::new()
                    },
                    if s.minutes > 0 {
                        format!(" · {} min", s.minutes)
                    } else {
                        String::new()
                    }
                ),
                width - 58.,
            ));
        }
        body = body.child(sessions);
        body = body.child(
            button(
                "event-config-toggle",
                if self.show_config {
                    "Hide full configuration"
                } else {
                    "Show full configuration & opponents"
                },
            )
            .on_click(cx.listener(|this, _, _, cx| {
                this.show_config = !this.show_config;
                cx.notify();
            })),
        );
        // Preserve every authored detail, including opponents, assists and track settings.
        for section in e.ini.sections().iter().filter(|_| self.show_config) {
            if section.name == "EVENT" {
                continue;
            }
            let mut config = panel()
                .flex()
                .flex_col()
                .gap_1()
                .child(section_title(section.name.clone()));
            for (key, value) in section.entries() {
                config = config
                    .child(line(format!("{key}: {value}"), width - 58.).text_color(theme::muted()));
            }
            body = body.child(config);
        }
        body.into_any_element()
    }
}

impl Render for EventsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let main_w = f32::from(window.viewport_size().width) - crate::app::SIDEBAR_W;
        let list_w = 310.;
        let detail_w = (main_w - list_w).max(300.);
        let title = if self.career {
            "Career"
        } else {
            "Special Events"
        };
        let selected = self.selected;
        let list = uniform_list(
            "events-list",
            self.visible.len(),
            cx.processor(move |this, range: std::ops::Range<usize>, _, cx| {
                range
                    .filter_map(|row| {
                        let i = *this.visible.get(row)?;
                        let e = this.list().get(i)?;
                        let p = this.progress.get(&e.id);
                        Some(
                            div()
                                .id(("event-row", i))
                                .h(px(70.))
                                .px_3()
                                .py_2()
                                .flex()
                                .flex_col()
                                .gap_1()
                                .cursor_pointer()
                                .border_b_1()
                                .border_color(theme::border())
                                .when(i == selected, |d| d.bg(theme::selected()))
                                .hover(|s| s.bg(theme::panel_hover()))
                                .child(
                                    div()
                                        .w(px(280.))
                                        .text_sm()
                                        .whitespace_nowrap()
                                        .text_ellipsis()
                                        .overflow_hidden()
                                        .child(e.name.clone()),
                                )
                                .child(
                                    div()
                                        .w(px(280.))
                                        .text_xs()
                                        .whitespace_nowrap()
                                        .text_ellipsis()
                                        .overflow_hidden()
                                        .text_color(medal_color(p.medal))
                                        .child(format!(
                                            "{} · {}",
                                            e.sessions
                                                .last()
                                                .map(|s| events::mode_label(s.kind))
                                                .unwrap_or("Event"),
                                            if this
                                                .current_series()
                                                .is_some_and(|s| s.championship())
                                            {
                                                p.last_position
                                                    .map(|pos| format!("Finished P{pos}"))
                                                    .unwrap_or_else(|| "Not completed".into())
                                            } else {
                                                medal(p.medal).into()
                                            }
                                        )),
                                )
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.selected = i;
                                    cx.notify();
                                })),
                        )
                    })
                    .collect()
            }),
        )
        .track_scroll(self.scroll.clone())
        .flex_1()
        .min_h_0();
        let mut left = div()
            .flex()
            .flex_col()
            .w(px(list_w))
            .flex_shrink_0()
            .border_r_1()
            .border_color(theme::border());
        if self.career {
            let mut series_list = div()
                .id("career-series")
                .flex()
                .flex_col()
                .max_h(px(180.))
                .overflow_y_scroll()
                .flex_shrink_0();
            for (i, s) in self.catalog.series.iter().enumerate() {
                series_list = series_list.child(
                    div()
                        .id(("career-series", i))
                        .px_3()
                        .py_2()
                        .cursor_pointer()
                        .text_sm()
                        .when(self.series == i, |d| d.bg(theme::selected()))
                        .hover(|d| d.bg(theme::panel_hover()))
                        .child(
                            div()
                                .w(px(280.))
                                .whitespace_nowrap()
                                .text_ellipsis()
                                .overflow_hidden()
                                .child(format!(
                                    "{} {}",
                                    if s.completed(&self.progress) {
                                        "✓"
                                    } else if !s.unlocked(&self.progress) {
                                        "Locked ·"
                                    } else {
                                        ""
                                    },
                                    s.name
                                )),
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.series = i;
                            this.selected = 0;
                            this.refilter(cx);
                            cx.notify();
                        })),
                );
            }
            left = left
                .child(div().px_3().py_2().child(section_title("Series")))
                .child(series_list);
        }
        left = left
            .child(div().p_2().flex().child(self.search.clone()))
            .child(
                div().px_2().pb_2().flex().flex_wrap().gap_1().children(
                    ["All", "Unearned", "Earned", "Gold"]
                        .iter()
                        .enumerate()
                        .map(|(i, label)| {
                            chip(("event-filter", i), *label, self.filter == i as u8)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.filter = i as u8;
                                    this.refilter(cx);
                                    cx.notify();
                                }))
                                .into_any_element()
                        })
                        .collect::<Vec<_>>(),
                ),
            )
            .child(
                div()
                    .px_3()
                    .py_1()
                    .text_xs()
                    .text_color(theme::muted())
                    .child(format!(
                        "{} / {} events",
                        self.visible.len(),
                        self.list().len()
                    )),
            )
            .child(list);
        let event = self.current_event();
        let blocked = self.running
            || self.loading
            || self.error.is_some()
            || event.is_none()
            || self
                .current_series()
                .is_some_and(|s| !s.unlocked(&self.progress));
        let unavailable = event
            .and_then(|e| events::available(&self.root, e, self.current_series()).err())
            .map(|e| format!("{e:#}"));
        let already_scored = self.current_series().is_some_and(|s| s.championship())
            && event.is_some_and(|e| self.progress.get(&e.id).last_position.is_some());
        let disabled = blocked || unavailable.is_some() || already_scored;
        let footer = div()
            .flex()
            .items_center()
            .gap_3()
            .p_3()
            .flex_shrink_0()
            .border_t_1()
            .border_color(theme::border())
            .child(
                primary_button(
                    "event-start",
                    if self.running {
                        "Game running…"
                    } else {
                        "Start event"
                    },
                )
                .when(disabled, |d| d.opacity(0.4))
                .when(!disabled, |d| {
                    d.on_click(cx.listener(|this, _, _, cx| this.start(cx)))
                }),
            )
            .child(line(
                if already_scored {
                    "Round scored. Restart championship to replay.".into()
                } else if let Some(e) = unavailable {
                    e
                } else if self
                    .current_series()
                    .is_some_and(|s| !s.unlocked(&self.progress))
                {
                    "Complete the prerequisite series to unlock.".into()
                } else {
                    "D-pad: select · A / Start: launch · X: difficulty".into()
                },
                (detail_w - 230.).max(160.),
            ));
        div()
            .flex()
            .flex_col()
            .size_full()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .p_4()
                    .flex_shrink_0()
                    .border_b_1()
                    .border_color(theme::border())
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(div().text_xl().font_weight(FontWeight::BOLD).child(title))
                            .child(line(self.overview(), main_w - 230.).text_color(theme::muted())),
                    )
                    .child(
                        button("events-refresh", "Refresh")
                            .on_click(cx.listener(|this, _, _, cx| this.reload(cx))),
                    ),
            )
            .child(
                line(events::import_notice(), main_w - 32.)
                    .px_4()
                    .py_1()
                    .text_color(theme::muted())
                    .text_xs(),
            )
            .when_some(self.error.clone(), |d, e| {
                d.child(line(e, main_w - 32.).p_3().text_color(theme::error()))
            })
            .when_some(self.message.clone(), |d, m| {
                d.child(line(m, main_w - 32.).p_3().text_color(theme::accent()))
            })
            .when(!self.catalog.errors.is_empty(), |d| {
                d.child(
                    line(self.catalog.errors.join(" · "), main_w - 32.)
                        .p_3()
                        .text_color(theme::error()),
                )
            })
            .when(self.loading, |d| {
                d.child(div().p_4().child("Loading events and progress…"))
            })
            .child(
                div().flex().flex_1().min_h_0().child(left).child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .min_w_0()
                        .child(
                            div()
                                .id("event-details")
                                .flex_1()
                                .min_h_0()
                                .overflow_y_scroll()
                                .child(self.details(detail_w, cx)),
                        )
                        .when(
                            self.current_series().is_some_and(|s| s.championship()),
                            |d| {
                                d.child(
                                    div().px_3().py_2().child(
                                        button(
                                            "championship-restart",
                                            if self.restart_confirm {
                                                "Confirm restart"
                                            } else {
                                                "Restart championship run"
                                            },
                                        )
                                        .on_click(cx.listener(|this, _, _, cx| this.restart(cx))),
                                    ),
                                )
                            },
                        )
                        .child(footer),
                ),
            )
    }
}
