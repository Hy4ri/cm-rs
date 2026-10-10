//! The main window: Quick Drive, Cars, Tracks, Install and Settings pages.

use std::ops::Range;
use std::path::PathBuf;
use std::sync::Arc;

use ac_core::content::{self, Car, TrackEntry, Weather};
use ac_core::race::{self, Conditions, Mode, QuickDrive};
use ac_core::settings::Settings;
use ac_core::{acd, install, launch, paths};
use gpui::{
    ClickEvent, Context, Entity, FontWeight, ObjectFit, PathPromptOptions, SharedString, Window,
    div, img, prelude::*, px, uniform_list,
};

use crate::text_input::TextInput;
use crate::theme;
use crate::widgets::{button, chip, field, panel, primary_button, section_title};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    QuickDrive,
    Cars,
    Tracks,
    Install,
    Settings,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ModeKind {
    Practice,
    Hotlap,
    Race,
}

#[derive(Default)]
struct Content {
    cars: Vec<Car>,
    tracks: Vec<TrackEntry>,
    weather: Vec<Weather>,
    car_search: Vec<String>,
    track_search: Vec<String>,
}

impl Content {
    fn scan(root: &std::path::Path) -> Self {
        let cars = content::scan_cars(root);
        let tracks = content::track_entries(&content::scan_tracks(root));
        Content {
            car_search: cars.iter().map(|c| c.search_text()).collect(),
            track_search: tracks.iter().map(|t| t.search_text()).collect(),
            weather: content::scan_weather(root),
            cars,
            tracks,
        }
    }
}

struct InstallState {
    archive: PathBuf,
    items: Vec<install::Item>,
}

pub struct CmApp {
    settings: Settings,
    ac_root: Option<PathBuf>,
    content: Arc<Content>,
    loading: bool,
    page: Page,

    car: Option<usize>,
    skin: usize,
    track: Option<usize>,
    car_filter: Entity<TextInput>,
    track_filter: Entity<TextInput>,
    cars_view: Vec<usize>,
    tracks_view: Vec<usize>,

    mode: ModeKind,
    practice_minutes: u32,
    laps: u32,
    opponents: u32,
    ai_level: u32,
    start_position: u32,
    time_seconds: u32,
    ambient: i32,
    weather_id: String,
    penalties: bool,

    install: Option<InstallState>,
    busy: bool,
    status: Option<(String, bool)>,
    game_running: bool,

    in_root: Entity<TextInput>,
    in_driver: Entity<TextInput>,
    in_nation: Entity<TextInput>,
    in_proton: Entity<TextInput>,
}

fn fmt_time(seconds: u32) -> String {
    format!("{:02}:{:02}", seconds / 3600, seconds % 3600 / 60)
}

impl CmApp {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let settings = Settings::load();
        let ac_root = settings
            .ac_root
            .clone()
            .filter(|p| paths::is_ac_root(p))
            .or_else(paths::find_ac_root);

        let car_filter = cx.new(|cx| TextInput::new(cx, "Search cars…", ""));
        let track_filter = cx.new(|cx| TextInput::new(cx, "Search tracks…", ""));
        cx.observe(&car_filter, |this, _, cx| this.refilter(cx)).detach();
        cx.observe(&track_filter, |this, _, cx| this.refilter(cx)).detach();

        let root_text = ac_root.as_ref().map(|p| p.display().to_string()).unwrap_or_default();
        let in_root = cx.new(|cx| TextInput::new(cx, "Assetto Corsa folder", root_text));
        let in_driver = cx.new(|cx| TextInput::new(cx, "Driver name", settings.driver_name.clone()));
        let in_nation = cx.new(|cx| TextInput::new(cx, "ITA", settings.nationality.clone()));
        let proton_text = settings.proton.as_ref().map(|p| p.display().to_string()).unwrap_or_default();
        let in_proton = cx.new(|cx| TextInput::new(cx, "Auto-detect", proton_text));

        let last = settings.last_quick_drive.clone().unwrap_or_default();
        let (mode, practice_minutes, laps, opponents, ai_level, start_position) = match last.mode {
            Mode::Practice { minutes } => (ModeKind::Practice, minutes, 5, 7, 95, 8),
            Mode::Hotlap => (ModeKind::Hotlap, 0, 5, 7, 95, 8),
            Mode::Race { laps, opponents, ai_level, starting_position } => {
                (ModeKind::Race, 0, laps, opponents, ai_level, starting_position)
            }
        };
        let c = if last.conditions.weather_id.is_empty() { Conditions::default() } else { last.conditions.clone() };

        let mut this = Self {
            page: if ac_root.is_some() { Page::QuickDrive } else { Page::Settings },
            settings,
            ac_root,
            content: Arc::default(),
            loading: false,
            car: None,
            skin: 0,
            track: None,
            car_filter,
            track_filter,
            cars_view: Vec::new(),
            tracks_view: Vec::new(),
            mode,
            practice_minutes,
            laps,
            opponents,
            ai_level,
            start_position,
            time_seconds: c.time_seconds,
            ambient: c.ambient_c as i32,
            weather_id: c.weather_id,
            penalties: last.penalties || last.car_id.is_empty(),
            install: None,
            busy: false,
            status: None,
            game_running: false,
            in_root,
            in_driver,
            in_nation,
            in_proton,
        };
        if this.ac_root.is_none() {
            this.status = Some(("Assetto Corsa wasn't found. Set its folder in Settings.".into(), true));
        }
        this.reload(cx);
        this
    }

    // ---------- state ----------

    fn set_status(&mut self, text: impl Into<String>, error: bool, cx: &mut Context<Self>) {
        self.status = Some((text.into(), error));
        cx.notify();
    }

    fn reload(&mut self, cx: &mut Context<Self>) {
        let Some(root) = self.ac_root.clone() else { return };
        self.loading = true;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let content = cx.background_executor().spawn(async move { Content::scan(&root) }).await;
            this.update(cx, |this, cx| {
                this.content = Arc::new(content);
                this.loading = false;
                this.restore_selection();
                this.refilter(cx);
                let n = (this.content.cars.len(), this.content.tracks.len());
                this.set_status(format!("Loaded {} cars and {} track layouts", n.0, n.1), false, cx);
            })
            .ok();
        })
        .detach();
    }

    fn restore_selection(&mut self) {
        let last = self.settings.last_quick_drive.clone().unwrap_or_default();
        let c = &self.content;
        self.car = c.cars.iter().position(|x| x.id == last.car_id).or((!c.cars.is_empty()).then_some(0));
        self.skin = self
            .car
            .and_then(|i| c.cars[i].skins.iter().position(|s| s.id == last.skin_id))
            .unwrap_or(0);
        self.track = c
            .tracks
            .iter()
            .position(|t| t.track_id == last.track_id && t.layout.layout_id == last.layout_id)
            .or((!c.tracks.is_empty()).then_some(0));
        if !c.weather.iter().any(|w| w.id == self.weather_id) {
            if let Some(w) = c.weather.first() {
                self.weather_id = w.id.clone();
            }
        }
    }

    fn refilter(&mut self, cx: &mut Context<Self>) {
        let car_q = self.car_filter.read(cx).text().to_lowercase();
        let track_q = self.track_filter.read(cx).text().to_lowercase();
        let matches = |hay: &str, q: &str| q.split_whitespace().all(|w| hay.contains(w));
        self.cars_view = (0..self.content.cars.len()).filter(|&i| matches(&self.content.car_search[i], &car_q)).collect();
        self.tracks_view = (0..self.content.tracks.len()).filter(|&i| matches(&self.content.track_search[i], &track_q)).collect();
        cx.notify();
    }

    fn selected_car(&self) -> Option<&Car> {
        self.car.and_then(|i| self.content.cars.get(i))
    }

    fn selected_track(&self) -> Option<&TrackEntry> {
        self.track.and_then(|i| self.content.tracks.get(i))
    }

    fn quick_drive(&self, cx: &Context<Self>) -> Option<QuickDrive> {
        let car = self.selected_car()?;
        let track = self.selected_track()?;
        let skin_id = car.skins.get(self.skin).map(|s| s.id.clone()).unwrap_or_default();
        let mode = match self.mode {
            ModeKind::Practice => Mode::Practice { minutes: self.practice_minutes },
            ModeKind::Hotlap => Mode::Hotlap,
            ModeKind::Race => Mode::Race {
                laps: self.laps,
                opponents: self.opponents,
                ai_level: self.ai_level,
                starting_position: self.start_position,
            },
        };
        let mut q = QuickDrive {
            car_id: car.id.clone(),
            skin_id: skin_id.clone(),
            track_id: track.track_id.clone(),
            layout_id: track.layout.layout_id.clone(),
            driver_name: self.in_driver.read(cx).text(),
            nationality: self.in_nation.read(cx).text().to_uppercase(),
            mode,
            conditions: Conditions {
                time_seconds: self.time_seconds,
                ambient_c: self.ambient as f64,
                weather_id: self.weather_id.clone(),
                time_multiplier: 1.0,
            },
            penalties: self.penalties,
            fixed_setup: false,
            use_mph: false,
            opponents: Vec::new(),
        };
        if self.mode == ModeKind::Race {
            let skins: Vec<String> = car.skins.iter().map(|s| s.id.clone()).collect();
            q.opponents = race::pick_opponents(&car.id, &skin_id, &skins, self.opponents);
        }
        Some(q)
    }

    // ---------- actions ----------

    fn go(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        let (Some(root), Some(q)) = (self.ac_root.clone(), self.quick_drive(cx)) else {
            return self.set_status("Pick a car and a track first.", true, cx);
        };
        if self.mode == ModeKind::Race {
            if let Some(t) = self.selected_track().and_then(|t| t.layout.pitboxes) {
                if self.opponents + 1 > t {
                    return self.set_status(format!("This track has only {t} pit boxes; lower the opponent count."), true, cx);
                }
            }
        }
        self.settings.last_quick_drive = Some(q.clone());
        let _ = self.settings.save();

        if let Err(e) = launch::write_race_ini(&root, &q) {
            return self.set_status(format!("Couldn't write race.ini: {e:#}"), true, cx);
        }
        let opts = launch::LaunchOptions { proton: self.settings.proton.clone() };
        match launch::start(&root, &opts) {
            Ok(mut child) => {
                self.game_running = true;
                self.set_status("Assetto Corsa is starting…", false, cx);
                cx.spawn(async move |this, cx| {
                    let code = cx.background_executor().spawn(async move { child.wait() }).await;
                    this.update(cx, |this, cx| {
                        this.game_running = false;
                        match code {
                            Ok(c) if c.success() => this.set_status("Game closed.", false, cx),
                            Ok(c) => this.set_status(format!("Game exited with {c}. Is Steam running?"), true, cx),
                            Err(e) => this.set_status(format!("Lost track of the game: {e}"), true, cx),
                        }
                    })
                    .ok();
                })
                .detach();
            }
            Err(e) => self.set_status(format!("{e:#}"), true, cx),
        }
    }

    fn save_settings(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        let root_text = self.in_root.read(cx).text();
        let root = PathBuf::from(root_text.trim());
        if !paths::is_ac_root(&root) {
            return self.set_status("That folder doesn't contain acs.exe and content/cars.", true, cx);
        }
        let proton = self.in_proton.read(cx).text();
        self.settings.ac_root = Some(root.clone());
        self.settings.driver_name = self.in_driver.read(cx).text();
        self.settings.nationality = self.in_nation.read(cx).text().to_uppercase();
        self.settings.proton = (!proton.trim().is_empty()).then(|| PathBuf::from(proton.trim()));
        if let Err(e) = self.settings.save() {
            return self.set_status(format!("Couldn't save settings: {e:#}"), true, cx);
        }
        let changed = self.ac_root.as_ref() != Some(&root);
        self.ac_root = Some(root);
        self.set_status("Settings saved.", false, cx);
        if changed {
            self.reload(cx);
        }
    }

    fn browse_root(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Select Assetto Corsa folder".into()),
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = rx.await {
                if let Some(p) = paths.into_iter().next() {
                    this.update(cx, |this, cx| {
                        this.in_root.update(cx, |i, cx| i.set_text(p.display().to_string(), cx));
                    })
                    .ok();
                }
            }
        })
        .detach();
    }

    fn detect_root(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        match paths::find_ac_root() {
            Some(p) => {
                self.in_root.update(cx, |i, cx| i.set_text(p.display().to_string(), cx));
                self.set_status(format!("Found {}", p.display()), false, cx);
            }
            None => self.set_status("Couldn't find Assetto Corsa in any Steam library.", true, cx),
        }
    }

    fn choose_archive(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = self.ac_root.clone() else { return };
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Select a mod archive (.zip)".into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = rx.await else { return };
            let Some(archive) = paths.into_iter().next() else { return };
            let a = archive.clone();
            let plan = cx.background_executor().spawn(async move { install::plan_zip(&a, &root) }).await;
            this.update(cx, |this, cx| match plan {
                Ok(items) => {
                    this.install = Some(InstallState { archive, items });
                    this.status = None;
                    cx.notify();
                }
                Err(e) => {
                    this.install = None;
                    this.set_status(format!("{e:#}"), true, cx);
                }
            })
            .ok();
        })
        .detach();
    }

    fn do_install(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        let Some(state) = self.install.take() else { return };
        self.busy = true;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let InstallState { archive, items } = state;
            let n = items.len();
            let result = cx.background_executor().spawn(async move { install::install_zip(&archive, &items) }).await;
            this.update(cx, |this, cx| {
                this.busy = false;
                match result {
                    Ok(files) => {
                        this.set_status(format!("Installed {n} item(s), {files} files."), false, cx);
                        this.reload(cx);
                    }
                    Err(e) => this.set_status(format!("Install failed: {e:#}"), true, cx),
                }
            })
            .ok();
        })
        .detach();
    }

    fn unpack_acd(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        let Some(car) = self.selected_car() else { return };
        let out = car.dir.join("data");
        if out.exists() {
            return self.set_status("This car already has a data folder; not overwriting it.", true, cx);
        }
        match acd::unpack(&car.dir.join("data.acd"), &out) {
            Ok(n) => self.set_status(format!("Unpacked {n} files to {}", out.display()), false, cx),
            Err(e) => self.set_status(format!("Unpack failed: {e:#}"), true, cx),
        }
    }

    fn pack_acd(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        let Some(car) = self.selected_car() else { return };
        let acd_path = car.dir.join("data.acd");
        if acd_path.exists() {
            let backup = car.dir.join("data.acd~cm-rs-backup");
            if let Err(e) = std::fs::copy(&acd_path, &backup) {
                return self.set_status(format!("Couldn't back up data.acd: {e}"), true, cx);
            }
        }
        match acd::pack(&car.dir.join("data"), &acd_path) {
            Ok(n) => self.set_status(format!("Packed {n} files into data.acd (old one kept as data.acd~cm-rs-backup)"), false, cx),
            Err(e) => self.set_status(format!("Pack failed: {e:#}"), true, cx),
        }
    }

    // ---------- views ----------

    fn nav_item(&self, page: Page, label: &'static str, cx: &mut Context<Self>) -> impl IntoElement {
        let active = self.page == page;
        div()
            .id(label)
            .px_4()
            .py_2()
            .text_sm()
            .cursor_pointer()
            .border_l_2()
            .when(active, |d| d.border_color(theme::accent()).bg(theme::panel()).text_color(theme::text()))
            .when(!active, |d| d.border_color(theme::sidebar()).text_color(theme::muted()).hover(|s| s.text_color(theme::text())))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.page = page;
                cx.notify();
            }))
            .child(label)
    }

    fn car_list(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .w(px(320.))
            .flex_shrink_0()
            .h_full()
            .border_r_1()
            .border_color(theme::border())
            .child(div().flex().w_full().p_2().child(self.car_filter.clone()))
            .child(
                uniform_list(
                    "car-list",
                    self.cars_view.len(),
                    cx.processor(|this, range: Range<usize>, _window, cx| {
                        range
                            .map(|ix| {
                                let car_ix = this.cars_view[ix];
                                let car = &this.content.cars[car_ix];
                                let selected = this.car == Some(car_ix);
                                let sub = [car.brand.clone(), car.class.clone()]
                                    .into_iter()
                                    .flatten()
                                    .collect::<Vec<_>>()
                                    .join(" · ");
                                div()
                                    .id(ix)
                                    .w_full()
                                    .h(px(52.))
                                    .px_2()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .cursor_pointer()
                                    .when(selected, |d| d.bg(theme::selected()))
                                    .when(!selected, |d| d.hover(|s| s.bg(theme::panel_hover())))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.car = Some(car_ix);
                                        this.skin = 0;
                                        cx.notify();
                                    }))
                                    .child(
                                        div().w(px(76.)).h(px(43.)).flex_shrink_0().bg(theme::panel()).rounded_sm().overflow_hidden().when_some(
                                            car.preview().map(|p| p.to_path_buf()),
                                            |d, p| d.child(img(p).size_full().object_fit(ObjectFit::Cover)),
                                        ),
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .flex_col()
                                            .min_w_0()
                                            .child(div().text_sm().truncate().child(car.name.clone()))
                                            .child(div().text_xs().text_color(theme::muted()).truncate().child(sub)),
                                    )
                            })
                            .collect()
                    }),
                )
                .flex_1(),
            )
    }

    fn track_list(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .w(px(300.))
            .flex_shrink_0()
            .h_full()
            .border_r_1()
            .border_color(theme::border())
            .child(div().flex().w_full().p_2().child(self.track_filter.clone()))
            .child(
                uniform_list(
                    "track-list",
                    self.tracks_view.len(),
                    cx.processor(|this, range: Range<usize>, _window, cx| {
                        range
                            .map(|ix| {
                                let t_ix = this.tracks_view[ix];
                                let t = &this.content.tracks[t_ix];
                                let selected = this.track == Some(t_ix);
                                let sub = [t.layout.country.clone(), t.layout.length.clone().map(|l| format!("{l} m"))]
                                    .into_iter()
                                    .flatten()
                                    .collect::<Vec<_>>()
                                    .join(" · ");
                                div()
                                    .id(ix)
                                    .w_full()
                                    .h(px(52.))
                                    .px_2()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .cursor_pointer()
                                    .when(selected, |d| d.bg(theme::selected()))
                                    .when(!selected, |d| d.hover(|s| s.bg(theme::panel_hover())))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.track = Some(t_ix);
                                        cx.notify();
                                    }))
                                    .child(
                                        div().w(px(43.)).h(px(43.)).flex_shrink_0().when_some(t.layout.outline.clone(), |d, p| {
                                            d.child(img(p).size_full().object_fit(ObjectFit::Contain))
                                        }),
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .flex_col()
                                            .min_w_0()
                                            .child(div().text_sm().truncate().child(t.layout.name.clone()))
                                            .child(div().text_xs().text_color(theme::muted()).truncate().child(sub)),
                                    )
                            })
                            .collect()
                    }),
                )
                .flex_1(),
            )
    }

    fn stepper(
        &self,
        id: &'static str,
        label: &'static str,
        value: String,
        step: fn(&mut CmApp, i32),
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        div()
            .flex()
            .items_center()
            .gap_2()
            .child(div().w(px(140.)).text_sm().text_color(theme::muted()).child(label))
            .child(button((id, 0usize), "−").on_click(cx.listener(move |this, _, _, cx| {
                step(this, -1);
                cx.notify();
            })))
            .child(div().w(px(170.)).text_center().text_sm().child(value))
            .child(button((id, 1usize), "+").on_click(cx.listener(move |this, _, _, cx| {
                step(this, 1);
                cx.notify();
            })))
    }

    fn quick_drive_page(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let car = self.selected_car();
        let skin = car.and_then(|c| c.skins.get(self.skin));
        let track = self.selected_track();
        let skin_count = car.map(|c| c.skins.len()).unwrap_or(0);

        let car_block = panel()
            .flex()
            .flex_col()
            .gap_2()
            .child(div().text_lg().font_weight(FontWeight::BOLD).child(car.map(|c| c.name.clone()).unwrap_or_else(|| "No car".into())))
            .child(
                div().w_full().h(px(220.)).bg(theme::bg()).rounded_sm().overflow_hidden().when_some(
                    skin.and_then(|s| s.preview.clone()),
                    |d, p| d.child(img(p).size_full().object_fit(ObjectFit::Contain)),
                ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(button("skin-prev", "◀").on_click(cx.listener(move |this, _, _, cx| {
                        if skin_count > 0 {
                            this.skin = (this.skin + skin_count - 1) % skin_count;
                            cx.notify();
                        }
                    })))
                    .child(
                        div()
                            .flex_1()
                            .text_center()
                            .text_sm()
                            .child(skin.map(|s| format!("{} ({}/{})", s.name, self.skin + 1, skin_count)).unwrap_or_else(|| "No skins".into())),
                    )
                    .child(button("skin-next", "▶").on_click(cx.listener(move |this, _, _, cx| {
                        if skin_count > 0 {
                            this.skin = (this.skin + 1) % skin_count;
                            cx.notify();
                        }
                    }))),
            );

        let track_block = panel()
            .flex()
            .gap_3()
            .child(
                div().w(px(160.)).h(px(90.)).flex_shrink_0().bg(theme::bg()).rounded_sm().overflow_hidden().when_some(
                    track.and_then(|t| t.layout.preview.clone()),
                    |d, p| d.child(img(p).size_full().object_fit(ObjectFit::Cover)),
                ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(div().font_weight(FontWeight::BOLD).child(track.map(|t| t.layout.name.clone()).unwrap_or_else(|| "No track".into())))
                    .when_some(track, |d, t| {
                        d.child(div().text_sm().text_color(theme::muted()).child(
                            [t.layout.city.clone(), t.layout.country.clone()].into_iter().flatten().collect::<Vec<_>>().join(", "),
                        ))
                        .child(div().text_sm().text_color(theme::muted()).child(format!(
                            "{} · {} pit boxes",
                            t.layout.length.clone().map(|l| format!("{l} m")).unwrap_or_else(|| "length ?".into()),
                            t.layout.pitboxes.map(|p| p.to_string()).unwrap_or_else(|| "?".into())
                        )))
                    }),
            );

        let mut mode_block = panel()
            .flex()
            .flex_col()
            .gap_2()
            .child(section_title("Mode"))
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(chip("mode-practice", "Practice", self.mode == ModeKind::Practice).on_click(cx.listener(|this, _, _, cx| {
                        this.mode = ModeKind::Practice;
                        cx.notify();
                    })))
                    .child(chip("mode-hotlap", "Hotlap", self.mode == ModeKind::Hotlap).on_click(cx.listener(|this, _, _, cx| {
                        this.mode = ModeKind::Hotlap;
                        cx.notify();
                    })))
                    .child(chip("mode-race", "Quick race", self.mode == ModeKind::Race).on_click(cx.listener(|this, _, _, cx| {
                        this.mode = ModeKind::Race;
                        cx.notify();
                    }))),
            );
        match self.mode {
            ModeKind::Practice => {
                let label = if self.practice_minutes == 0 { "Unlimited".to_string() } else { format!("{} min", self.practice_minutes) };
                mode_block = mode_block.child(self.stepper("minutes", "Duration", label, |t, d| {
                    t.practice_minutes = (t.practice_minutes as i32 + d * 5).clamp(0, 240) as u32
                }, cx));
            }
            ModeKind::Hotlap => {}
            ModeKind::Race => {
                mode_block = mode_block
                    .child(self.stepper("laps", "Laps", self.laps.to_string(), |t, d| t.laps = (t.laps as i32 + d).clamp(1, 200) as u32, cx))
                    .child(self.stepper("opponents", "Opponents", self.opponents.to_string(), |t, d| {
                        t.opponents = (t.opponents as i32 + d).clamp(1, 40) as u32;
                        t.start_position = t.start_position.min(t.opponents + 1);
                    }, cx))
                    .child(self.stepper("ai", "AI strength", format!("{}%", self.ai_level), |t, d| {
                        t.ai_level = (t.ai_level as i32 + d).clamp(70, 100) as u32
                    }, cx))
                    .child(self.stepper("grid", "Start position", self.start_position.to_string(), |t, d| {
                        t.start_position = (t.start_position as i32 + d).clamp(1, t.opponents as i32 + 1) as u32
                    }, cx));
            }
        }
        mode_block = mode_block.child(
            div().flex().items_center().gap_2().child(div().w(px(140.)).text_sm().text_color(theme::muted()).child("Penalties")).child(
                chip("penalties", if self.penalties { "On" } else { "Off" }, self.penalties).on_click(cx.listener(|this, _, _, cx| {
                    this.penalties = !this.penalties;
                    cx.notify();
                })),
            ),
        );

        let road = race::road_temperature(self.time_seconds as f64, self.ambient as f64, 1.0);
        let conditions = panel()
            .flex()
            .flex_col()
            .gap_2()
            .child(section_title("Conditions"))
            .child(self.stepper("time", "Time", fmt_time(self.time_seconds), |t, d| {
                t.time_seconds = (t.time_seconds as i32 + d * 1800).clamp(race::TIME_MIN as i32, race::TIME_MAX as i32) as u32
            }, cx))
            .child(self.stepper("temp", "Temperature", format!("{}°C (road {:.0}°C)", self.ambient, road), |t, d| {
                t.ambient = (t.ambient + d).clamp(0, 40)
            }, cx))
            .child(
                div().flex().flex_wrap().gap_2().children(self.content.weather.iter().enumerate().map(|(i, w)| {
                    let id = w.id.clone();
                    chip(("weather", i), w.name.clone(), self.weather_id == w.id).on_click(cx.listener(move |this, _, _, cx| {
                        this.weather_id = id.clone();
                        cx.notify();
                    }))
                })),
            );

        let go_label = if self.game_running { "Running…" } else { "Go!" };
        div()
            .flex()
            .size_full()
            .child(self.car_list(cx))
            .child(self.track_list(cx))
            .child(
                div()
                    .id("qd-detail")
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .overflow_y_scroll()
                    .p_4()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(car_block)
                    .child(track_block)
                    .child(mode_block)
                    .child(conditions)
                    .child(
                        div().flex().justify_end().child(
                            primary_button("go", go_label)
                                .when(self.game_running, |d| d.opacity(0.6))
                                .when(!self.game_running, |d| d.on_click(cx.listener(Self::go))),
                        ),
                    ),
            )
    }

    fn cars_page(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let detail = match self.selected_car() {
            None => div().p_4().text_color(theme::muted()).child("No car selected"),
            Some(car) => {
                let mut d = div()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .p_4()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_3()
                            .when_some(car.badge.clone(), |d, b| d.child(img(b).size(px(40.)).object_fit(ObjectFit::Contain)))
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .child(div().text_xl().font_weight(FontWeight::BOLD).child(car.name.clone()))
                                    .child(div().text_sm().text_color(theme::muted()).child(car.id.clone())),
                            ),
                    )
                    .child(
                        div().w_full().h(px(260.)).bg(theme::bg()).rounded_sm().overflow_hidden().when_some(
                            car.skins.get(self.skin).and_then(|s| s.preview.clone()).or(car.preview().map(|p| p.to_path_buf())),
                            |d, p| d.child(img(p).size_full().object_fit(ObjectFit::Contain)),
                        ),
                    );
                let mut info = panel().flex().flex_col().gap_1();
                for (k, v) in [("Brand", &car.brand), ("Class", &car.class), ("Year", &car.year), ("Country", &car.country), ("Author", &car.author)] {
                    if let Some(v) = v {
                        info = info.child(field(k, v.clone()));
                    }
                }
                for (k, v) in &car.specs {
                    info = info.child(field(content::spec_label(k).to_owned(), v.clone()));
                }
                info = info.child(field(
                    "Data",
                    match (car.has_data_acd, car.has_data_dir) {
                        (true, true) => "data.acd + data folder",
                        (true, false) => "data.acd (packed)",
                        (false, true) => "data folder (unpacked)",
                        (false, false) => "missing",
                    },
                ));
                d = d.child(info);
                if !car.tags.is_empty() {
                    d = d.child(div().flex().flex_wrap().gap_1().children(car.tags.iter().map(|t| {
                        div().px_2().rounded_sm().bg(theme::panel()).text_xs().text_color(theme::muted()).child(t.clone())
                    })));
                }
                if let Some(desc) = &car.description {
                    d = d.child(div().text_sm().child(desc.clone()));
                }
                if let Some(err) = &car.ui_error {
                    d = d.child(div().text_sm().text_color(theme::error()).child(format!("ui_car.json: {err}")));
                }
                let dir = car.dir.clone();
                d = d.child(
                    div()
                        .flex()
                        .gap_2()
                        .child(button("open-folder", "Open folder").on_click(cx.listener(move |_, _, _, cx| cx.open_with_system(&dir))))
                        .when(car.has_data_acd && !car.has_data_dir, |d| {
                            d.child(button("unpack", "Unpack data.acd").on_click(cx.listener(Self::unpack_acd)))
                        })
                        .when(car.has_data_dir, |d| d.child(button("pack", "Pack data → data.acd").on_click(cx.listener(Self::pack_acd)))),
                );
                d = d.child(section_title(format!("Skins ({})", car.skins.len()))).child(
                    div().flex().flex_wrap().gap_2().children(car.skins.iter().enumerate().map(|(i, s)| {
                        let selected = i == self.skin;
                        div()
                            .id(("skin", i))
                            .w(px(150.))
                            .flex()
                            .flex_col()
                            .rounded_sm()
                            .border_1()
                            .border_color(if selected { theme::accent() } else { theme::border() })
                            .cursor_pointer()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.skin = i;
                                cx.notify();
                            }))
                            .child(div().h(px(84.)).bg(theme::bg()).overflow_hidden().when_some(s.preview.clone(), |d, p| {
                                d.child(img(p).size_full().object_fit(ObjectFit::Cover))
                            }))
                            .child(div().px_1().text_xs().truncate().child(s.name.clone()))
                    })),
                );
                d
            }
        };
        div()
            .flex()
            .size_full()
            .child(self.car_list(cx))
            .child(div().id("car-detail").flex_1().min_w_0().h_full().overflow_y_scroll().child(detail))
    }

    fn tracks_page(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let detail = match self.selected_track() {
            None => div().p_4().text_color(theme::muted()).child("No track selected"),
            Some(t) => {
                let l = &t.layout;
                let mut info = panel().flex().flex_col().gap_1().child(field("Folder", t.key()));
                for (k, v) in [("Country", &l.country), ("City", &l.city), ("Length", &l.length.clone().map(|x| format!("{x} m")))] {
                    if let Some(v) = v {
                        info = info.child(field(k, v.clone()));
                    }
                }
                if let Some(p) = l.pitboxes {
                    info = info.child(field("Pit boxes", p.to_string()));
                }
                div()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .p_4()
                    .child(div().text_xl().font_weight(FontWeight::BOLD).child(l.name.clone()))
                    .child(
                        div()
                            .flex()
                            .gap_3()
                            .child(div().flex_1().h(px(300.)).bg(theme::bg()).rounded_sm().overflow_hidden().when_some(l.preview.clone(), |d, p| {
                                d.child(img(p).size_full().object_fit(ObjectFit::Contain))
                            }))
                            .child(div().w(px(260.)).h(px(300.)).when_some(l.outline.clone(), |d, p| {
                                d.child(img(p).size_full().object_fit(ObjectFit::Contain))
                            })),
                    )
                    .child(info)
                    .when_some(l.description.clone(), |d, desc| d.child(div().text_sm().child(desc)))
            }
        };
        div()
            .flex()
            .size_full()
            .child(self.track_list(cx))
            .child(div().id("track-detail").flex_1().min_w_0().h_full().overflow_y_scroll().child(detail))
    }

    fn install_page(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut page = div()
            .flex()
            .flex_col()
            .gap_3()
            .p_4()
            .max_w(px(760.))
            .child(div().text_xl().font_weight(FontWeight::BOLD).child("Install mods"))
            .child(div().text_sm().text_color(theme::muted()).child(
                "Pick a .zip with cars or tracks. Folders are detected by ui_car.json / data.acd / ui_track.json, \
                 wherever they sit in the archive.",
            ))
            .child(div().flex().child(button("choose-zip", "Choose archive…").on_click(cx.listener(Self::choose_archive))));
        if let Some(state) = &self.install {
            let mut list = panel().flex().flex_col().gap_1().child(section_title(state.archive.display().to_string()));
            for item in &state.items {
                let kind = match item.kind {
                    install::Kind::Car => "Car",
                    install::Kind::Track => "Track",
                };
                list = list.child(
                    div()
                        .flex()
                        .gap_2()
                        .text_sm()
                        .child(div().w(px(60.)).text_color(theme::muted()).child(kind))
                        .child(div().flex_1().child(item.id.clone()))
                        .child(if item.exists {
                            div().text_color(theme::accent()).child("update (overwrites files)")
                        } else {
                            div().text_color(theme::ok()).child("new")
                        }),
                );
            }
            page = page.child(list).child(
                div().flex().child(
                    primary_button("install", if self.busy { "Installing…" } else { "Install" })
                        .when(!self.busy, |d| d.on_click(cx.listener(Self::do_install))),
                ),
            );
        }
        page
    }

    fn settings_page(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let row = |label: &'static str, input: Entity<TextInput>| {
            div()
                .flex()
                .items_center()
                .gap_3()
                .child(div().w(px(160.)).flex_shrink_0().text_sm().text_color(theme::muted()).child(label))
                .child(div().flex_1().child(input))
        };
        let race_ini = self
            .ac_root
            .as_ref()
            .and_then(|r| paths::race_ini_path(r))
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "not found (on Linux: run AC from Steam once)".into());
        div()
            .id("settings")
            .size_full()
            .overflow_y_scroll()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .p_4()
                    .max_w(px(820.))
                    .child(div().text_xl().font_weight(FontWeight::BOLD).child("Settings"))
                    .child(row("Assetto Corsa folder", self.in_root.clone()))
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .pl(px(172.))
                            .child(button("browse-root", "Browse…").on_click(cx.listener(Self::browse_root)))
                            .child(button("detect-root", "Auto-detect").on_click(cx.listener(Self::detect_root))),
                    )
                    .child(row("Driver name", self.in_driver.clone()))
                    .child(row("Nationality (3 letters)", self.in_nation.clone()))
                    .when(!cfg!(windows), |d| d.child(row("Proton script", self.in_proton.clone())))
                    .child(div().flex().pl(px(172.)).child(primary_button("save-settings", "Save").on_click(cx.listener(Self::save_settings))))
                    .child(section_title("Detected"))
                    .child(field("race.ini", race_ini))
                    .when(!cfg!(windows), |d| {
                        let proton = self
                            .ac_root
                            .as_ref()
                            .and_then(|r| launch::find_proton(r))
                            .map(|p| p.display().to_string())
                            .unwrap_or_else(|| "none found".into());
                        d.child(field("Proton", proton))
                    })
                    .child(field("Settings file", ac_core::settings::path().map(|p| p.display().to_string()).unwrap_or_default())),
            )
    }
}

impl Render for CmApp {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let page: gpui::AnyElement = if self.loading {
            div().p_4().text_color(theme::muted()).child("Loading content…").into_any_element()
        } else {
            match self.page {
                Page::QuickDrive => self.quick_drive_page(cx).into_any_element(),
                Page::Cars => self.cars_page(cx).into_any_element(),
                Page::Tracks => self.tracks_page(cx).into_any_element(),
                Page::Install => self.install_page(cx).into_any_element(),
                Page::Settings => self.settings_page(cx).into_any_element(),
            }
        };
        let status: SharedString = self.status.as_ref().map(|(s, _)| s.clone()).unwrap_or_default().into();
        let status_err = self.status.as_ref().is_some_and(|(_, e)| *e);

        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(theme::bg())
            .text_color(theme::text())
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .w(px(190.))
                            .flex_shrink_0()
                            .bg(theme::sidebar())
                            .pt_4()
                            .child(div().px_4().pb_4().text_lg().font_weight(FontWeight::BOLD).text_color(theme::accent()).child("Content Manager"))
                            .child(self.nav_item(Page::QuickDrive, "Quick Drive", cx))
                            .child(self.nav_item(Page::Cars, "Cars", cx))
                            .child(self.nav_item(Page::Tracks, "Tracks", cx))
                            .child(self.nav_item(Page::Install, "Install mods", cx))
                            .child(self.nav_item(Page::Settings, "Settings", cx)),
                    )
                    .child(div().flex_1().min_w_0().h_full().child(page)),
            )
            .child(
                div()
                    .h(px(26.))
                    .px_3()
                    .flex()
                    .items_center()
                    .border_t_1()
                    .border_color(theme::border())
                    .bg(theme::sidebar())
                    .text_xs()
                    .text_color(if status_err { theme::error() } else { theme::muted() })
                    .child(status),
            )
    }
}
