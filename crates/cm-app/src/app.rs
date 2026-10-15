//! The main window: Quick Drive, Cars, Tracks, Install and Settings pages.

use std::ops::Range;
use std::path::PathBuf;
use std::sync::Arc;

use ac_core::content::{self, Car, TrackEntry, Weather};
use ac_core::game_cfg::{self, GameConfig, Kind, Setting, Tab};
use ac_core::race::{self, Conditions, Mode, QuickDrive};
use ac_core::settings::Settings;
use ac_core::{acd, install, launch, paths};
use gpui::{
    ClickEvent, Context, Div, Entity, FocusHandle, FontWeight, KeyDownEvent, ModifiersChangedEvent,
    ObjectFit, PathPromptOptions, ScrollStrategy, SharedString, UniformListScrollHandle, Window, div, img,
    prelude::*, px, uniform_list,
};

use crate::browser_view::{BrowserView, PreviewDownload};
use crate::events_view::{EventsView, Running};
use crate::text_input::TextInput;
use crate::theme;
use crate::widgets::{
    button, chip, field, panel, picture, primary_button, section_title, segmented, setting_row,
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    QuickDrive,
    Cars,
    Tracks,
    Career,
    SpecialEvents,
    Install,
    Browser,
    Game,
    Settings,
}

/// Sidebar order, which L1/R1 on the controller walk through.
const PAGES: [Page; 9] = [
    Page::QuickDrive,
    Page::Cars,
    Page::Tracks,
    Page::Career,
    Page::SpecialEvents,
    Page::Browser,
    Page::Install,
    Page::Game,
    Page::Settings,
];

/// Next list index after moving `delta` rows, clamped; starts at the top with no selection.
fn step_index(pos: Option<usize>, delta: isize, len: usize) -> usize {
    match pos {
        None => 0,
        Some(p) => (p as isize + delta).clamp(0, len as isize - 1) as usize,
    }
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
    car_brands: Vec<(String, usize)>,
    track_search: Vec<String>,
}

impl Content {
    fn scan(root: &std::path::Path) -> Self {
        let cars = content::scan_cars(root);
        let mut car_brands: Vec<_> = content::brands(&cars).into_iter().collect();
        car_brands.sort_by(|(a, _), (b, _)| content::natural_cmp(a, b));
        let tracks = content::track_entries(&content::scan_tracks(root));
        Content {
            car_search: cars.iter().map(|c| c.search_text()).collect(),
            car_brands,
            track_search: tracks.iter().map(|t| t.search_text()).collect(),
            weather: content::scan_weather(root),
            cars,
            tracks,
        }
    }
}

struct InstallState {
    root: PathBuf,
    archive: PathBuf,
    items: Vec<install::Item>,
}

pub struct CmApp {
    settings: Settings,
    ac_root: Option<PathBuf>,
    content: Arc<Content>,
    loading: bool,
    page: Page,
    browser: Option<Entity<BrowserView>>,
    events: Option<Entity<EventsView>>,

    car: Option<usize>,
    skin: usize,
    track: Option<usize>,
    car_filter: Entity<TextInput>,
    track_filter: Entity<TextInput>,
    cars_view: Vec<usize>,
    car_scroll: UniformListScrollHandle,
    track_scroll: UniformListScrollHandle,
    /// Controller navigation task and which Quick Drive list the D-pad drives.
    pad_nav: Option<gpui::Task<()>>,
    /// The press that finishes a bind capture is also seen by the nav reader; ignore it until this time.
    pad_mute_until: Option<std::time::Instant>,
    pad_tracks: bool,
    car_brand: Option<String>,
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
    install_generation: u64,
    busy: bool,
    status: Option<(String, bool)>,
    game_running: bool,

    in_root: Entity<TextInput>,
    in_driver: Entity<TextInput>,
    in_nation: Entity<TextInput>,
    in_proton: Entity<TextInput>,

    game_cfg: Option<GameConfig>,
    game_error: Option<String>,
    game_tab: Tab,
    game_settings: Vec<Setting>,
    /// Index into [`game_cfg::BINDS`] waiting for a key press.
    capturing: Option<usize>,
    capture_focus: FocusHandle,
    /// Index into [`game_cfg::BINDS`] waiting for a controller button, and the task reading it.
    pad_capturing: Option<usize>,
    pad_task: Option<gpui::Task<()>>,
}

pub(crate) const SIDEBAR_W: f32 = 190.;
const CAR_LIST_W: f32 = 300.;
const TRACK_LIST_W: f32 = 280.;
const ROW_H: f32 = 52.;
const CAR_THUMB_W: f32 = 76.;
const TRACK_THUMB_W: f32 = 40.;
/// Text width left in a list row: column minus its right border, the row's
/// horizontal padding, selection border, thumbnail and gap.
const fn row_text_w(list_w: f32, thumb_w: f32) -> f32 {
    list_w - 1. - 16. - 2. - thumb_w - 8.
}

fn count_label(shown: usize, total: usize, noun: &str) -> String {
    if shown == total {
        format!("{total} {noun}")
    } else {
        format!("{shown} of {total} {noun}")
    }
}

/// One clickable row of a virtualized list (fixed height, as `uniform_list` requires).
fn list_row(ix: usize, selected: bool) -> gpui::Stateful<gpui::Div> {
    div()
        .id(ix)
        .w_full()
        .h(px(ROW_H))
        .px_2()
        .flex()
        .items_center()
        .gap_2()
        .cursor_pointer()
        .border_l_2()
        .when(selected, |d| {
            d.bg(theme::selected()).border_color(theme::accent())
        })
        .when(!selected, |d| {
            d.border_color(gpui::transparent_black())
                .hover(|s| s.bg(theme::panel_hover()))
        })
}

/// Title + muted subtitle, both truncated with an ellipsis.
///
/// GPUI caches the first measurement of nowrap text, and inside flex layouts
/// that first pass has no width limit, so `truncate` only works when the
/// text's own box has a fixed pixel width. Pass it when known (list rows);
/// `None` fills the remaining space and simply clips.
fn two_lines(
    title: impl Into<SharedString>,
    sub: impl Into<SharedString>,
    width: Option<f32>,
) -> gpui::Div {
    let line = |d: gpui::Div| match width {
        Some(w) => d.w(px(w)),
        None => d.w_full(),
    };
    div()
        .flex()
        .flex_col()
        .map(|d| match width {
            Some(w) => d.w(px(w)).flex_shrink_0(),
            None => d.flex_1().min_w_0(),
        })
        .child(line(div()).text_sm().truncate().child(title.into()))
        .child(
            line(div())
                .text_xs()
                .text_color(theme::muted())
                .truncate()
                .child(sub.into()),
        )
}

const INSTALL_MAX_W: f32 = 760.;
const SETTINGS_MAX_W: f32 = 860.;
/// Horizontal space a `panel()` takes from its content: `p_3` on both sides plus the border.
const PANEL_INSET: f32 = 2. * 12. + 2.;

/// Width for wrapping text inside a page column that is `p_4` padded and
/// capped at `max_w`.
///
/// GPUI measures text once and caches the result, so text that wraps in a
/// flex layout keeps its unwrapped (one line) height and spills over what
/// follows. A definite pixel width makes the first measurement the right one.
fn text_w(main_w: f32, max_w: f32) -> f32 {
    (main_w.min(max_w) - 2. * 16.).max(120.)
}

/// A text input that takes the remaining width of a row.
///
/// The wrapper must be a flex container: under a plain block wrapper the
/// input's `w_full` resolves before the flex item has grown and collapses to
/// a few pixels.
fn grow_input(input: Entity<TextInput>) -> gpui::Div {
    div().flex().flex_1().min_w_0().child(input)
}

fn fmt_time(seconds: u32) -> String {
    format!("{:02}:{:02}", seconds / 3600, seconds % 3600 / 60)
}

impl CmApp {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let (settings, settings_error) = match Settings::load() {
            Ok(settings) => (settings, None),
            Err(e) => (
                Settings::default(),
                Some(format!(
                    "Settings could not be loaded: {e:#}. Fix the file and restart."
                )),
            ),
        };
        let ac_root = settings
            .ac_root
            .clone()
            .filter(|p| paths::is_ac_root(p))
            .or_else(paths::find_ac_root);

        let car_filter = cx.new(|cx| TextInput::new(cx, "Search cars…", ""));
        let track_filter = cx.new(|cx| TextInput::new(cx, "Search tracks…", ""));
        cx.observe(&car_filter, |this, _, cx| this.refilter(cx))
            .detach();
        cx.observe(&track_filter, |this, _, cx| this.refilter(cx))
            .detach();

        let root_text = ac_root
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        let in_root = cx.new(|cx| TextInput::new(cx, "Assetto Corsa folder", root_text));
        let in_driver =
            cx.new(|cx| TextInput::new(cx, "Driver name", settings.driver_name.clone()));
        let in_nation = cx.new(|cx| TextInput::new(cx, "ITA", settings.nationality.clone()));
        let proton_text = settings
            .proton
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        let in_proton = cx.new(|cx| TextInput::new(cx, "Auto-detect", proton_text));

        let last = settings.last_quick_drive.clone().unwrap_or_default();
        let (mode, practice_minutes, laps, opponents, ai_level, start_position) = match last.mode {
            Mode::Practice { minutes } => (ModeKind::Practice, minutes, 5, 7, 95, 8),
            Mode::Hotlap => (ModeKind::Hotlap, 0, 5, 7, 95, 8),
            Mode::Race {
                laps,
                opponents,
                ai_level,
                starting_position,
            } => (
                ModeKind::Race,
                0,
                laps,
                opponents,
                ai_level,
                starting_position,
            ),
        };
        let c = if last.conditions.weather_id.is_empty() {
            Conditions::default()
        } else {
            last.conditions.clone()
        };

        let mut this = Self {
            page: if ac_root.is_some() {
                Page::QuickDrive
            } else {
                Page::Settings
            },
            browser: None,
            events: None,
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
            car_scroll: UniformListScrollHandle::new(),
            track_scroll: UniformListScrollHandle::new(),
            pad_nav: None,
            pad_mute_until: None,
            pad_tracks: false,
            car_brand: None,
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
            install_generation: 0,
            busy: false,
            status: None,
            game_running: false,
            in_root,
            in_driver,
            in_nation,
            in_proton,
            game_cfg: None,
            game_error: None,
            game_tab: Tab::Video,
            game_settings: game_cfg::settings(),
            capturing: None,
            capture_focus: cx.focus_handle(),
            pad_capturing: None,
            pad_task: None,
        };
        if this.ac_root.is_none() {
            this.status = Some((
                "Assetto Corsa wasn't found. Set its folder in Settings.".into(),
                true,
            ));
        }
        if let Some(error) = settings_error {
            this.page = Page::Settings;
            this.status = Some((error, true));
        } else {
            this.reload(cx);
        }
        this
    }

    // ---------- state ----------

    fn set_status(&mut self, text: impl Into<String>, error: bool, cx: &mut Context<Self>) {
        self.status = Some((text.into(), error));
        cx.notify();
    }

    fn reload(&mut self, cx: &mut Context<Self>) {
        let Some(root) = self.ac_root.clone() else {
            return;
        };
        self.loading = true;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let content = cx
                .background_executor()
                .spawn(async move { Content::scan(&root) })
                .await;
            this.update(cx, |this, cx| {
                this.content = Arc::new(content);
                this.loading = false;
                this.restore_selection();
                this.refilter(cx);
                let n = (this.content.cars.len(), this.content.tracks.len());
                this.set_status(
                    format!("Loaded {} cars and {} track layouts", n.0, n.1),
                    false,
                    cx,
                );
            })
            .ok();
        })
        .detach();
    }

    fn restore_selection(&mut self) {
        let last = self.settings.last_quick_drive.clone().unwrap_or_default();
        let c = &self.content;
        self.car = c
            .cars
            .iter()
            .position(|x| x.id == last.car_id)
            .or((!c.cars.is_empty()).then_some(0));
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
        // A brand that vanished after a rescan would hide every car.
        if let Some(b) = &self.car_brand
            && !self
                .content
                .car_brands
                .iter()
                .any(|(name, _)| name.to_lowercase() == b.to_lowercase())
        {
            self.car_brand = None;
        }
        let brand = self.car_brand.as_ref().map(|b| b.to_lowercase());
        let cars_view: Vec<_> = (0..self.content.cars.len())
            .filter(|&i| {
                brand
                    .as_deref()
                    .is_none_or(|b| self.content.cars[i].brand_name().to_lowercase() == b)
                    && matches(&self.content.car_search[i], &car_q)
            })
            .collect();
        if cars_view != self.cars_view && !cars_view.is_empty() {
            self.car_scroll.scroll_to_item(0, ScrollStrategy::Top);
        }
        self.cars_view = cars_view;
        self.tracks_view = (0..self.content.tracks.len())
            .filter(|&i| matches(&self.content.track_search[i], &track_q))
            .collect();
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
        let skin_id = car
            .skins
            .get(self.skin)
            .map(|s| s.id.clone())
            .unwrap_or_default();
        let mode = match self.mode {
            ModeKind::Practice => Mode::Practice {
                minutes: self.practice_minutes,
            },
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
        self.start_game(cx);
    }

    fn start_game(&mut self, cx: &mut Context<Self>) {
        if self.game_running {
            return;
        }
        let (Some(root), Some(q)) = (self.ac_root.clone(), self.quick_drive(cx)) else {
            return self.set_status("Pick a car and a track first.", true, cx);
        };
        if self.mode == ModeKind::Race {
            if let Some(t) = self.selected_track().and_then(|t| t.layout.pitboxes) {
                if self.opponents + 1 > t {
                    return self.set_status(
                        format!("This track has only {t} pit boxes; lower the opponent count."),
                        true,
                        cx,
                    );
                }
            }
        }
        self.settings.last_quick_drive = Some(q.clone());
        if let Err(e) = self.settings.save() {
            return self.set_status(format!("Couldn't save settings: {e:#}"), true, cx);
        }

        if let Err(e) = launch::write_race_ini(&root, &q) {
            return self.set_status(format!("Couldn't write race.ini: {e:#}"), true, cx);
        }
        let opts = launch::LaunchOptions {
            proton: self.settings.proton.clone(),
        };
        match launch::start(&root, &opts) {
            Ok(mut child) => {
                self.game_running = true;
                if let Some(events) = &self.events { events.update(cx, |view, cx| view.set_running(true, cx)); }
                self.set_status("Assetto Corsa is starting…", false, cx);
                cx.spawn(async move |this, cx| {
                    let code = cx
                        .background_executor()
                        .spawn(async move { child.wait() })
                        .await;
                    this.update(cx, |this, cx| {
                        this.game_running = false;
                        if let Some(events) = &this.events { events.update(cx, |view, cx| view.set_running(false, cx)); }
                        match code {
                            Ok(c) if c.success() => this.set_status("Game closed.", false, cx),
                            Ok(c) => this.set_status(
                                format!("Game exited with {c}. Is Steam running?"),
                                true,
                                cx,
                            ),
                            Err(e) => {
                                this.set_status(format!("Lost track of the game: {e}"), true, cx)
                            }
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
            return self.set_status(
                "That folder doesn't contain acs.exe and content/cars.",
                true,
                cx,
            );
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
        if changed {
            self.game_cfg = None;
        }
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
                        this.in_root
                            .update(cx, |i, cx| i.set_text(p.display().to_string(), cx));
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
                self.in_root
                    .update(cx, |i, cx| i.set_text(p.display().to_string(), cx));
                self.set_status(format!("Found {}", p.display()), false, cx);
            }
            None => self.set_status(
                "Couldn't find Assetto Corsa in any Steam library.",
                true,
                cx,
            ),
        }
    }

    fn ensure_browser(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.browser.is_some() {
            return;
        }
        let browser = cx.new(|cx| BrowserView::new(window, cx));
        cx.subscribe(&browser, |this, _, event: &PreviewDownload, cx| {
            if this.busy {
                this.set_status("Wait for the current installation to finish.", true, cx);
                return;
            }
            this.preview_archive(event.0.clone(), cx);
        })
        .detach();
        self.browser = Some(browser);
    }

    fn preview_archive(&mut self, archive: PathBuf, cx: &mut Context<Self>) {
        let Some(root) = self.ac_root.clone() else {
            self.set_status(
                "Set your Assetto Corsa folder in Settings before installing mods.",
                true,
                cx,
            );
            return;
        };
        if self.busy {
            return;
        }
        if let Some(browser) = &self.browser {
            browser.update(cx, |browser, cx| browser.set_active(false, cx));
        }
        self.install_generation = self.install_generation.wrapping_add(1);
        let generation = self.install_generation;
        self.install = None;
        self.page = Page::Install;
        self.set_status("Inspecting archive…", false, cx);
        cx.spawn(async move |this, cx| {
            let a = archive.clone();
            let planned_root = root.clone();
            let plan = cx
                .background_executor()
                .spawn(async move { install::plan_zip(&a, &planned_root) })
                .await;
            this.update(cx, |this, cx| {
                if this.install_generation != generation
                    || this.ac_root.as_ref() != Some(&root)
                    || this.busy
                {
                    return;
                }
                match plan {
                    Ok(items) => {
                        this.install = Some(InstallState {
                            root: root.clone(),
                            archive,
                            items,
                        });
                        this.status = None;
                        cx.notify();
                    }
                    Err(e) => this.set_status(format!("{e:#}"), true, cx),
                }
            })
            .ok();
        })
        .detach();
    }

    fn choose_archive(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.busy || self.ac_root.is_none() {
            return;
        }
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Select a mod archive (.zip)".into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = rx.await else {
                return;
            };
            let Some(archive) = paths.into_iter().next() else {
                return;
            };
            this.update(cx, |this, cx| this.preview_archive(archive, cx))
                .ok();
        })
        .detach();
    }

    fn do_install(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let Some(state) = self.install.take() else {
            return;
        };
        if self.ac_root.as_ref() != Some(&state.root) {
            self.set_status(
                "Game folder changed. Preview the archive again before installing.",
                true,
                cx,
            );
            return;
        }
        self.busy = true;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let InstallState { archive, items, .. } = state;
            let n = items.len();
            let result = cx
                .background_executor()
                .spawn(async move { install::install_zip(&archive, &items) })
                .await;
            this.update(cx, |this, cx| {
                this.busy = false;
                match result {
                    Ok(files) => {
                        this.set_status(
                            format!("Installed {n} item(s), {files} files."),
                            false,
                            cx,
                        );
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
        let Some(car) = self.selected_car() else {
            return;
        };
        let out = car.dir.join("data");
        if out.exists() {
            return self.set_status(
                "This car already has a data folder; not overwriting it.",
                true,
                cx,
            );
        }
        match acd::unpack(&car.dir.join("data.acd"), &out) {
            Ok(n) => self.set_status(
                format!("Unpacked {n} files to {}", out.display()),
                false,
                cx,
            ),
            Err(e) => self.set_status(format!("Unpack failed: {e:#}"), true, cx),
        }
    }

    fn pack_acd(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        let Some(car) = self.selected_car() else {
            return;
        };
        let acd_path = car.dir.join("data.acd");
        if acd_path.exists() {
            let backup = car.dir.join("data.acd~cm-rs-backup");
            if let Err(e) = std::fs::copy(&acd_path, &backup) {
                return self.set_status(format!("Couldn't back up data.acd: {e}"), true, cx);
            }
        }
        match acd::pack(&car.dir.join("data"), &acd_path) {
            Ok(n) => self.set_status(
                format!("Packed {n} files into data.acd (old one kept as data.acd~cm-rs-backup)"),
                false,
                cx,
            ),
            Err(e) => self.set_status(format!("Pack failed: {e:#}"), true, cx),
        }
    }

    // ---------- game settings ----------

    /// Loads the cfg files the first time the page opens (and after the AC folder changes).
    fn open_game_page(&mut self, cx: &mut Context<Self>) {
        if self.game_cfg.is_some() {
            return;
        }
        let Some(root) = self.ac_root.clone() else {
            self.game_error = Some("Set the Assetto Corsa folder in Settings first.".into());
            return;
        };
        match GameConfig::load(&root) {
            Ok(c) => {
                self.game_cfg = Some(c);
                self.game_error = None;
            }
            Err(e) => self.game_error = Some(format!("{e:#}")),
        }
        cx.notify();
    }

    fn save_game(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        let Some(c) = self.game_cfg.as_mut() else {
            return;
        };
        match c.save() {
            Ok(files) if files.is_empty() => self.set_status("Nothing to save.", false, cx),
            Ok(files) => {
                let note = if self.game_running {
                    " Restart the game to apply."
                } else {
                    ""
                };
                self.set_status(format!("Saved {}.{note}", files.join(", ")), false, cx)
            }
            Err(e) => self.set_status(format!("Couldn't save game settings: {e:#}"), true, cx),
        }
    }

    fn revert_game(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.game_cfg = None;
        self.capturing = None;
        self.open_game_page(cx);
        self.set_status("Reverted to the saved game settings.", false, cx);
    }

    fn assign_key(&mut self, vk: Option<u32>, cx: &mut Context<Self>) {
        let (Some(i), Some(c)) = (self.capturing.take(), self.game_cfg.as_mut()) else {
            return;
        };
        let bind = &game_cfg::BINDS[i];
        c.set_bind(bind, vk);
        let conflicts = c.conflicts(bind);
        if !conflicts.is_empty() {
            self.status = Some((
                format!(
                    "{} shares its key with {}.",
                    bind.label,
                    conflicts.join(", ")
                ),
                true,
            ));
        }
        cx.notify();
    }

    fn capture_key(&mut self, e: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.capturing.is_none() {
            return;
        }
        cx.stop_propagation();
        if e.keystroke.key == "escape" {
            self.capturing = None;
            return cx.notify();
        }
        match game_cfg::vk_from_key(&e.keystroke.key) {
            Some(vk) => self.assign_key(Some(vk), cx),
            None => self.set_status(
                format!("AC can't bind “{}”; try another key.", e.keystroke.key),
                true,
                cx,
            ),
        }
    }

    /// Modifier keys arrive as modifier changes, not key presses. AC wants the
    /// left/right variant; GPUI doesn't say which side, so the left one is used.
    fn capture_modifier(
        &mut self,
        e: &ModifiersChangedEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.capturing.is_none() {
            return;
        }
        let m = e.modifiers;
        let vk = if m.control {
            0xA2
        } else if m.shift {
            0xA0
        } else if m.alt {
            0xA4
        } else {
            return;
        };
        self.assign_key(Some(vk), cx);
    }

    fn game_control(
        &self,
        idx: usize,
        s: &Setting,
        c: &GameConfig,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let value = c.get(s.file, s.section, s.key).unwrap_or("").to_owned();
        let base = idx * 100;
        fn set(
            idx: usize,
            v: String,
            cx: &mut Context<CmApp>,
        ) -> impl Fn(&ClickEvent, &mut Window, &mut gpui::App) + 'static {
            cx.listener(move |this, _, _, cx| {
                let s = &this.game_settings[idx];
                let (file, section, key) = (s.file, s.section, s.key);
                if let Some(c) = this.game_cfg.as_mut() {
                    c.set(file, section, key, &v);
                }
                cx.notify();
            })
        }
        match &s.kind {
            Kind::Toggle => {
                let on = value.trim() != "0" && !value.is_empty();
                segmented(vec![
                    chip(("gs", base), "On", on).on_click(set(idx, "1".into(), cx)),
                    chip(("gs", base + 1), "Off", !on).on_click(set(idx, "0".into(), cx)),
                ])
                .into_any_element()
            }
            Kind::Choice(opts) => {
                let known = opts.iter().any(|(v, _)| *v == value);
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap_1()
                    .children(opts.iter().enumerate().map(|(j, (v, label))| {
                        chip(("gs", base + j), *label, *v == value).on_click(set(
                            idx,
                            (*v).to_owned(),
                            cx,
                        ))
                    }))
                    .when(!known && !value.is_empty(), |d| {
                        d.child(
                            div()
                                .text_xs()
                                .text_color(theme::muted())
                                .child(format!("current: {value}")),
                        )
                    })
                    .into_any_element()
            }
            Kind::Filter => {
                div()
                    .flex()
                    .flex_1()
                    .flex_wrap()
                    .gap_1()
                    .children(c.filters.iter().enumerate().map(|(j, f)| {
                        chip(("gs", base + j), f.clone(), f.eq_ignore_ascii_case(&value))
                            .on_click(set(idx, f.clone(), cx))
                    }))
                    .into_any_element()
            }
            Kind::Number { scale, suffix, .. } => {
                let shown = format!("{}{suffix}", game_cfg::fmt_number(c.number(s) * scale));
                fn step(
                    idx: usize,
                    d: i32,
                    cx: &mut Context<CmApp>,
                ) -> impl Fn(&ClickEvent, &mut Window, &mut gpui::App) + 'static {
                    cx.listener(move |this, _, _, cx| {
                        let s = this.game_settings[idx].clone();
                        if let Some(c) = this.game_cfg.as_mut() {
                            c.step(&s, d);
                        }
                        cx.notify();
                    })
                }
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .child(button(("gs", base), "−").on_click(step(idx, -1, cx)))
                    .child(div().w(px(96.)).text_center().text_sm().child(shown))
                    .child(button(("gs", base + 1), "+").on_click(step(idx, 1, cx)))
                    .into_any_element()
            }
        }
    }

    #[cfg(target_os = "linux")]
    fn start_pad_capture(&mut self, i: usize, cx: &mut Context<Self>) {
        use ac_core::gamepad::{Press, Reader};
        self.capturing = None;
        let mut reader = match Reader::open() {
            Ok(r) => r,
            Err(e) => return self.set_status(format!("{e:#}"), true, cx),
        };
        self.pad_capturing = Some(i);
        self.status = Some((format!("Listening on {}…", reader.names.join(", ")), false));
        cx.notify();
        self.pad_task = Some(cx.spawn(async move |this, cx| {
            for _ in 0..(15_000 / 16) {
                cx.background_executor().timer(std::time::Duration::from_millis(16)).await;
                let Some(press) = reader.poll() else { continue };
                let done = this
                    .update(cx, |this, cx| {
                        if this.pad_capturing != Some(i) {
                            return true;
                        }
                        match press {
                            Press::Button(name) => {
                                this.pad_capturing = None;
                                this.pad_mute_until = Some(std::time::Instant::now() + std::time::Duration::from_millis(400));
                                let b = &game_cfg::BINDS[i];
                                if let Some(c) = this.game_cfg.as_mut() {
                                    c.set_pad(b, Some(name));
                                    let others = c.pad_conflicts(b);
                                    this.status = Some(if others.is_empty() {
                                        (format!("{} → {}", b.label, game_cfg::pad_label(name)), false)
                                    } else {
                                        (format!("{} shares {} with {}.", b.label, game_cfg::pad_label(name), others.join(", ")), true)
                                    });
                                }
                                cx.notify();
                                true
                            }
                            Press::Unbindable(_) => {
                                this.set_status("AC can't bind that button (L2/R2 are brake/throttle). Try another.", true, cx);
                                false
                            }
                        }
                    })
                    .unwrap_or(true);
                if done {
                    return;
                }
            }
            this.update(cx, |this, cx| {
                if this.pad_capturing == Some(i) {
                    this.pad_capturing = None;
                    this.set_status("No button pressed.", true, cx);
                }
            })
            .ok();
        }));
    }

    #[cfg(not(target_os = "linux"))]
    fn start_pad_capture(&mut self, _: usize, cx: &mut Context<Self>) {
        self.set_status("Controller capture is Linux-only for now.", true, cx);
    }

    fn keybinds(&self, c: &GameConfig, cx: &mut Context<Self>) -> Vec<Div> {
        let mut panels: Vec<Div> = Vec::new();
        let mut current = "";
        for (i, b) in game_cfg::BINDS.iter().enumerate() {
            if b.group != current {
                current = b.group;
                panels.push(
                    panel()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(section_title(b.group)),
                );
            }
            let capturing = self.capturing == Some(i);
            let bound = c.bind(b);
            let label = if capturing {
                "Press a key… (Esc cancels)".to_string()
            } else {
                bound.map(game_cfg::vk_name).unwrap_or_else(|| "—".into())
            };
            let mut conflicts = c.conflicts(b);
            conflicts.extend(c.pad_conflicts(b));
            let pad_capturing = self.pad_capturing == Some(i);
            let pad = c.pad(b);
            let pad_cell = match b.pad_axis() {
                Some(axis) => div()
                    .w(px(200.))
                    .px_3()
                    .text_sm()
                    .text_color(theme::muted())
                    .child(format!("{axis} (automatic)"))
                    .into_any_element(),
                None => div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .id(("pad", i))
                            .w(px(140.))
                            .px_3()
                            .py_1()
                            .rounded_sm()
                            .border_1()
                            .text_sm()
                            .cursor_pointer()
                            .when(pad_capturing, |d| {
                                d.border_color(theme::accent()).text_color(theme::accent())
                            })
                            .when(!pad_capturing, |d| {
                                d.border_color(theme::border())
                                    .bg(theme::panel())
                                    .hover(|s| s.bg(theme::panel_hover()))
                            })
                            .when(pad.is_none() && !pad_capturing, |d| {
                                d.text_color(theme::muted())
                            })
                            .child(if pad_capturing {
                                "Press a button…".to_string()
                            } else {
                                pad.map(|p| game_cfg::pad_label(p).to_owned())
                                    .unwrap_or_else(|| "—".into())
                            })
                            .on_click(
                                cx.listener(move |this, _, _, cx| this.start_pad_capture(i, cx)),
                            ),
                    )
                    .when(pad.is_some(), |d| {
                        d.child(button(("unpad", i), "Clear").on_click(cx.listener(
                            move |this, _, _, cx| {
                                if let Some(c) = this.game_cfg.as_mut() {
                                    c.set_pad(&game_cfg::BINDS[i], None);
                                }
                                cx.notify();
                            },
                        )))
                    })
                    .into_any_element(),
            };
            let key_button = div()
                .id(("bind", i))
                .w(px(200.))
                .px_3()
                .py_1()
                .rounded_sm()
                .border_1()
                .text_sm()
                .cursor_pointer()
                .when(capturing, |d| {
                    d.border_color(theme::accent()).text_color(theme::accent())
                })
                .when(!capturing, |d| {
                    d.border_color(theme::border())
                        .bg(theme::panel())
                        .hover(|s| s.bg(theme::panel_hover()))
                })
                .when(bound.is_none() && !capturing, |d| {
                    d.text_color(theme::muted())
                })
                .child(label)
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.capturing = Some(i);
                    this.pad_capturing = None;
                    this.pad_task = None;
                    window.focus(&this.capture_focus);
                    cx.notify();
                }));
            let row = div()
                .flex()
                .items_center()
                .gap_2()
                .min_h(px(28.))
                .child(
                    div()
                        .w(px(170.))
                        .flex_shrink_0()
                        .text_sm()
                        .text_color(theme::muted())
                        .child(b.label),
                )
                .child(key_button)
                .child(div().w(px(56.)).flex_shrink_0().child(if bound.is_some() {
                    button(("unbind", i), "Clear")
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.capturing = Some(i);
                            this.assign_key(None, cx);
                        }))
                        .into_any_element()
                } else {
                    div().into_any_element()
                }))
                .child(pad_cell)
                .when(!conflicts.is_empty(), |d| {
                    d.child(
                        div()
                            .text_xs()
                            .text_color(theme::error())
                            .child(format!("also {}", conflicts.join(", "))),
                    )
                });
            let last = panels.pop().unwrap();
            panels.push(last.child(row));
        }
        panels
    }

    fn game_page(&self, text_w: f32, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(c) = self.game_cfg.as_ref() else {
            let msg = self.game_error.clone().unwrap_or_else(|| "Loading…".into());
            return div()
                .id("game")
                .p_4()
                .flex()
                .flex_col()
                .gap_2()
                .child(
                    div()
                        .text_xl()
                        .font_weight(FontWeight::BOLD)
                        .child("Game settings"),
                )
                .child(div().text_sm().text_color(theme::error()).child(msg));
        };

        let tabs = segmented(
            Tab::ALL
                .iter()
                .map(|&t| {
                    chip(t.label(), t.label(), self.game_tab == t).on_click(cx.listener(
                        move |this, _, _, cx| {
                            this.game_tab = t;
                            this.capturing = None;
                            this.pad_capturing = None;
                            this.pad_task = None;
                            cx.notify();
                        },
                    ))
                })
                .collect(),
        );

        let mut panels: Vec<Div> = Vec::new();
        let mut current = "";
        for (idx, s) in self
            .game_settings
            .iter()
            .enumerate()
            .filter(|(_, s)| s.tab == self.game_tab)
        {
            if s.group != current {
                current = s.group;
                panels.push(
                    panel()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(section_title(s.group)),
                );
            }
            let row = div()
                .flex()
                .items_center()
                .gap_3()
                .min_h(px(28.))
                .child(
                    div()
                        .w(px(170.))
                        .flex_shrink_0()
                        .text_sm()
                        .text_color(theme::muted())
                        .child(s.label),
                )
                .child(self.game_control(idx, s, c, cx));
            let last = panels.pop().unwrap();
            panels.push(last.child(row));
        }
        if self.game_tab == Tab::Keys {
            panels.push(
                panel().child(
                    div()
                        .w(px(text_w - PANEL_INSET))
                        .text_sm()
                        .text_color(theme::muted())
                        .child(
                            "Keyboard: click a key, then press the new one (Esc cancels; Ctrl, Shift and Alt bind as the \
                             left-hand key). Controller: click its column, then press the button. Set Input method to \
                             Gamepad on the Controls tab to drive with it.",
                        ),
                ),
            );
            panels.extend(self.keybinds(c, cx));
        }

        let dirty = c.dirty_files();
        let footer = div()
            .flex()
            .items_center()
            .gap_3()
            .px_4()
            .py_2()
            .border_t_1()
            .border_color(theme::border())
            .bg(theme::sidebar())
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_sm()
                    .text_color(theme::muted())
                    .truncate()
                    .child(if dirty.is_empty() {
                        format!("Saved in {}", c.dir.display())
                    } else {
                        format!("Unsaved: {}", dirty.join(", "))
                    }),
            )
            .when(!dirty.is_empty(), |d| {
                d.child(button("game-revert", "Revert").on_click(cx.listener(Self::revert_game)))
            })
            .child(
                primary_button("game-save", "Save")
                    .when(dirty.is_empty(), |d| d.opacity(0.5))
                    .when(!dirty.is_empty(), |d| {
                        d.on_click(cx.listener(Self::save_game))
                    }),
            );

        div()
            .id("game")
            .track_focus(&self.capture_focus)
            .on_key_down(cx.listener(Self::capture_key))
            .on_modifiers_changed(cx.listener(Self::capture_modifier))
            .flex()
            .flex_col()
            .size_full()
            .child(
                div()
                    .id("game-body")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_3()
                            .p_4()
                            .w_full()
                            .max_w(px(SETTINGS_MAX_W))
                            .child(
                                div()
                                    .text_xl()
                                    .font_weight(FontWeight::BOLD)
                                    .child("Game settings"),
                            )
                            .child(tabs)
                            .children(panels),
                    ),
            )
            .child(footer)
    }

    // ---------- views ----------

    fn ensure_events(&mut self, career: bool, cx: &mut Context<Self>) {
        let Some(root) = self.ac_root.clone() else { return; };
        if self.events.as_ref().is_some_and(|e| e.read(cx).root != root) && !self.game_running {
            self.events = None;
        }
        if self.events.is_none() {
            let driver = self.settings.driver_name.clone();
            let nationality = self.settings.nationality.clone();
            let proton = self.settings.proton.clone();
            let view = cx.new(|cx| EventsView::new(root, career, driver, nationality, proton, cx));
            cx.subscribe(&view, |this, _, event: &Running, cx| {
                this.game_running = event.0;
                cx.notify();
            }).detach();
            view.update(cx, |view, cx| view.set_running(self.game_running, cx));
            self.events = Some(view);
        } else if let Some(view) = &self.events {
            view.update(cx, |view, cx| view.configure(career, self.settings.driver_name.clone(), self.settings.nationality.clone(), self.settings.proton.clone(), cx));
        }
    }

    fn goto_page(&mut self, page: Page, window: &mut Window, cx: &mut Context<Self>) {
        if matches!(page, Page::Career | Page::SpecialEvents) {
            self.ensure_events(page == Page::Career, cx);
        }
        if page == Page::Browser {
            self.ensure_browser(window, cx);
        }
        if let Some(browser) = &self.browser {
            browser.update(cx, |browser, cx| browser.set_active(page == Page::Browser, cx));
        }
        self.page = page;
        if page == Page::Game {
            self.open_game_page(cx);
        }
        cx.notify();
    }

    /// Moves the car or track selection (whichever list the page shows) by `delta` rows.
    fn pad_move(&mut self, delta: isize) {
        let tracks = match self.page {
            Page::Tracks => true,
            Page::Cars => false,
            Page::QuickDrive => self.pad_tracks,
            _ => return,
        };
        if tracks {
            if self.tracks_view.is_empty() {
                return;
            }
            let pos = self.track.and_then(|t| self.tracks_view.iter().position(|&i| i == t));
            let next = step_index(pos, delta, self.tracks_view.len());
            self.track = Some(self.tracks_view[next]);
            self.track_scroll.scroll_to_item(next, ScrollStrategy::Center);
        } else {
            if self.cars_view.is_empty() {
                return;
            }
            let pos = self.car.and_then(|c| self.cars_view.iter().position(|&i| i == c));
            let next = step_index(pos, delta, self.cars_view.len());
            self.car = Some(self.cars_view[next]);
            self.skin = 0;
            self.car_scroll.scroll_to_item(next, ScrollStrategy::Center);
        }
    }

    /// One controller press aimed at the launcher UI. Ignored while a bind is
    /// being captured, the game runs, or the window is in the background.
    fn pad_press(&mut self, name: &str, window: &mut Window, cx: &mut Context<Self>) {
        let mute = std::time::Instant::now() + std::time::Duration::from_millis(400);
        if self.pad_capturing.is_some() || self.capturing.is_some() {
            self.pad_mute_until = Some(mute);
            return;
        }
        if self.pad_mute_until.is_some_and(|t| std::time::Instant::now() < t) {
            return;
        }
        if self.game_running || !window.is_window_active() {
            return;
        }
        if matches!(self.page, Page::Career | Page::SpecialEvents) && !matches!(name, "LSHOULDER" | "RSHOULDER" | "B") {
            if let Some(events) = &self.events { events.update(cx, |view, cx| view.pad(name, cx)); }
            return;
        }
        let here = PAGES.iter().position(|p| *p == self.page).unwrap_or(0);
        match name {
            "LSHOULDER" => self.goto_page(PAGES[(here + PAGES.len() - 1) % PAGES.len()], window, cx),
            "RSHOULDER" => self.goto_page(PAGES[(here + 1) % PAGES.len()], window, cx),
            "DPAD_UP" => self.pad_move(-1),
            "DPAD_DOWN" => self.pad_move(1),
            "DPAD_LEFT" | "DPAD_RIGHT" if self.page == Page::QuickDrive => {
                self.pad_tracks = name == "DPAD_RIGHT";
                let which = if self.pad_tracks { "tracks" } else { "cars" };
                self.set_status(format!("Controller: D-pad moves the {which} list."), false, cx);
            }
            "A" if matches!(self.page, Page::Cars | Page::Tracks) => self.goto_page(Page::QuickDrive, window, cx),
            "B" if self.page != Page::QuickDrive => self.goto_page(Page::QuickDrive, window, cx),
            "X" => {
                let n = self.selected_car().map(|c| c.skins.len()).unwrap_or(0);
                if n > 0 {
                    self.skin = (self.skin + 1) % n;
                }
            }
            "START" if matches!(self.page, Page::QuickDrive | Page::Cars | Page::Tracks) => self.start_game(cx),
            _ => {}
        }
        cx.notify();
    }

    #[cfg(target_os = "linux")]
    pub fn start_pad_nav(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        use ac_core::gamepad::{Press, Reader};
        self.pad_nav = Some(cx.spawn_in(window, async move |this, cx| {
            let mut reader: Option<Reader> = None;
            let mut ticks = 0u32;
            loop {
                cx.background_executor().timer(std::time::Duration::from_millis(16)).await;
                // Reopen every ~5 s so a pad that connects or reconnects is picked up.
                if ticks % 312 == 0 {
                    reader = Reader::open().ok();
                }
                ticks = ticks.wrapping_add(1);
                let Some(Press::Button(name)) = reader.as_mut().and_then(|r| r.poll()) else { continue };
                if this.update_in(cx, |this, window, cx| this.pad_press(name, window, cx)).is_err() {
                    return;
                }
            }
        }));
    }

    fn nav_item(
        &self,
        page: Page,
        label: &'static str,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let active = self.page == page;
        div()
            .id(label)
            .px_4()
            .py_2()
            .text_sm()
            .cursor_pointer()
            .border_l_2()
            .when(active, |d| {
                d.border_color(theme::accent())
                    .bg(theme::panel())
                    .text_color(theme::text())
            })
            .when(!active, |d| {
                d.border_color(theme::sidebar())
                    .text_color(theme::muted())
                    .hover(|s| s.text_color(theme::text()))
            })
            .on_click(cx.listener(move |this, _, window, cx| this.goto_page(page, window, cx)))
            .child(label)
    }

    /// Search box, "shown of total" line and the list itself.
    fn list_column(
        &self,
        width: f32,
        filter: Entity<TextInput>,
        counter: String,
        header: Option<gpui::AnyElement>,
        list: impl IntoElement,
    ) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .w(px(width))
            .flex_shrink_0()
            .h_full()
            .border_r_1()
            .border_color(theme::border())
            .child(div().flex().w_full().px_2().pt_2().child(filter))
            .children(header)
            .child(
                div()
                    .px_3()
                    .py_1()
                    .text_xs()
                    .text_color(theme::muted())
                    .child(counter),
            )
            .child(list)
    }

    /// Always-visible, scrollable brands from the installed car catalog.
    fn brand_header(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let current = self.car_brand.as_deref();
        let all = chip(
            "brand-all",
            format!("All brands ({})", self.content.cars.len()),
            current.is_none(),
        )
        .w_full()
        .flex_shrink_0()
        .on_click(cx.listener(|this, _, _, cx| {
            this.car_brand = None;
            this.refilter(cx);
        }));
        let mut rows = Vec::new();
        for (i, (name, count)) in self.content.car_brands.iter().enumerate() {
            let active = current.is_some_and(|b| b.to_lowercase() == name.to_lowercase());
            let pick = name.clone();
            rows.push(
                chip(("brand", i), "", active)
                    .w_full()
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .child(div().w(px(CAR_LIST_W - 90.)).truncate().child(name.clone()))
                    .child(div().flex_shrink_0().child(count.to_string()))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.car_brand = Some(pick.clone());
                        this.refilter(cx);
                    }))
                    .into_any_element(),
            );
        }
        div()
            .flex()
            .flex_col()
            .flex_shrink_0()
            .w_full()
            .px_2()
            .pt_2()
            .gap_1()
            .child(
                div()
                    .text_xs()
                    .text_color(theme::muted())
                    .child(format!("Brands ({})", self.content.car_brands.len())),
            )
            .child(all)
            .child(
                div()
                    .id("brand-list")
                    .flex()
                    .flex_col()
                    .gap_1()
                    .max_h(px(180.))
                    .overflow_y_scroll()
                    .children(rows),
            )
            .into_any_element()
    }

    fn car_list(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let counter = count_label(self.cars_view.len(), self.content.cars.len(), "cars");
        let list = uniform_list(
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
                        list_row(ix, selected)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.car = Some(car_ix);
                                this.skin = 0;
                                cx.notify();
                            }))
                            .child(
                                picture(car.preview().map(|p| p.to_path_buf()), 43.)
                                    .w(px(CAR_THUMB_W)),
                            )
                            .child(two_lines(
                                car.name.clone(),
                                sub,
                                Some(row_text_w(CAR_LIST_W, CAR_THUMB_W)),
                            ))
                    })
                    .collect()
            }),
        )
        .track_scroll(self.car_scroll.clone())
        .flex_1();
        let header = self.brand_header(cx);
        self.list_column(CAR_LIST_W, self.car_filter.clone(), counter, Some(header), list)
    }

    fn track_list(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let counter = count_label(self.tracks_view.len(), self.content.tracks.len(), "layouts");
        let list = uniform_list(
            "track-list",
            self.tracks_view.len(),
            cx.processor(|this, range: Range<usize>, _window, cx| {
                range
                    .map(|ix| {
                        let t_ix = this.tracks_view[ix];
                        let t = &this.content.tracks[t_ix];
                        let selected = this.track == Some(t_ix);
                        let sub = [
                            t.layout.country.clone(),
                            t.layout.length.clone().map(|l| format!("{l} m")),
                        ]
                        .into_iter()
                        .flatten()
                        .collect::<Vec<_>>()
                        .join(" · ");
                        list_row(ix, selected)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.track = Some(t_ix);
                                cx.notify();
                            }))
                            .child(
                                picture(t.layout.outline.clone(), TRACK_THUMB_W)
                                    .w(px(TRACK_THUMB_W)),
                            )
                            .child(two_lines(
                                t.layout.name.clone(),
                                sub,
                                Some(row_text_w(TRACK_LIST_W, TRACK_THUMB_W)),
                            ))
                    })
                    .collect()
            }),
        )
        .track_scroll(self.track_scroll.clone())
        .flex_1();
        self.list_column(TRACK_LIST_W, self.track_filter.clone(), counter, None, list)
    }

    /// `−  value  +` control; pair it with [`setting_row`] for the label.
    fn stepper(
        &self,
        id: &'static str,
        value: String,
        step: fn(&mut CmApp, i32),
        cx: &mut Context<Self>,
    ) -> Div {
        div()
            .flex()
            .items_center()
            .gap_1()
            .child(
                button((id, 0usize), "−").on_click(cx.listener(move |this, _, _, cx| {
                    step(this, -1);
                    cx.notify();
                })),
            )
            .child(div().w(px(96.)).text_center().text_sm().child(value))
            .child(
                button((id, 1usize), "+").on_click(cx.listener(move |this, _, _, cx| {
                    step(this, 1);
                    cx.notify();
                })),
            )
    }

    fn open_quick_drive(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.page = Page::QuickDrive;
        cx.notify();
    }

    fn quick_drive_page(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let car = self.selected_car();
        let skin = car.and_then(|c| c.skins.get(self.skin));
        let track = self.selected_track();
        let skin_count = car.map(|c| c.skins.len()).unwrap_or(0);

        let skin_label = skin
            .map(|s| format!("Skin: {} · {}/{}", s.name, self.skin + 1, skin_count))
            .unwrap_or_else(|| "No skins".into());
        let car_block = panel()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(two_lines(
                        car.map(|c| c.name.clone())
                            .unwrap_or_else(|| "No car".into()),
                        skin_label,
                        None,
                    ))
                    .child(
                        button("skin-prev", "◀").on_click(cx.listener(move |this, _, _, cx| {
                            if skin_count > 0 {
                                this.skin = (this.skin + skin_count - 1) % skin_count;
                                cx.notify();
                            }
                        })),
                    )
                    .child(button("skin-next", "▶").on_click(cx.listener(
                        move |this, _, _, cx| {
                            if skin_count > 0 {
                                this.skin = (this.skin + 1) % skin_count;
                                cx.notify();
                            }
                        },
                    ))),
            )
            .child(picture(skin.and_then(|s| s.preview.clone()), 240.).w_full());

        let track_block = panel()
            .flex()
            .items_center()
            .gap_3()
            .child(picture(track.and_then(|t| t.layout.preview.clone()), 72.).w(px(128.)))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .gap_1()
                    .child(
                        div().font_weight(FontWeight::BOLD).truncate().child(
                            track
                                .map(|t| t.layout.name.clone())
                                .unwrap_or_else(|| "No track".into()),
                        ),
                    )
                    .when_some(track, |d, t| {
                        let place = [t.layout.city.clone(), t.layout.country.clone()]
                            .into_iter()
                            .flatten()
                            .collect::<Vec<_>>()
                            .join(", ");
                        let size = format!(
                            "{} · {} pit boxes",
                            t.layout
                                .length
                                .clone()
                                .map(|l| format!("{l} m"))
                                .unwrap_or_else(|| "length ?".into()),
                            t.layout
                                .pitboxes
                                .map(|p| p.to_string())
                                .unwrap_or_else(|| "?".into())
                        );
                        d.child(
                            div()
                                .text_sm()
                                .text_color(theme::muted())
                                .truncate()
                                .child(place),
                        )
                        .child(div().text_sm().text_color(theme::muted()).child(size))
                    }),
            )
            .child(picture(track.and_then(|t| t.layout.outline.clone()), 72.).w(px(72.)));

        let mut session = panel()
            .flex()
            .flex_col()
            .gap_2()
            .child(section_title("Session"))
            .child(setting_row(
                "Mode",
                segmented(vec![
                    chip("mode-practice", "Practice", self.mode == ModeKind::Practice).on_click(
                        cx.listener(|this, _, _, cx| {
                            this.mode = ModeKind::Practice;
                            cx.notify();
                        }),
                    ),
                    chip("mode-hotlap", "Hotlap", self.mode == ModeKind::Hotlap).on_click(
                        cx.listener(|this, _, _, cx| {
                            this.mode = ModeKind::Hotlap;
                            cx.notify();
                        }),
                    ),
                    chip("mode-race", "Quick race", self.mode == ModeKind::Race).on_click(
                        cx.listener(|this, _, _, cx| {
                            this.mode = ModeKind::Race;
                            cx.notify();
                        }),
                    ),
                ]),
            ));
        match self.mode {
            ModeKind::Practice => {
                let label = if self.practice_minutes == 0 {
                    "Unlimited".to_string()
                } else {
                    format!("{} min", self.practice_minutes)
                };
                session = session.child(setting_row(
                    "Duration",
                    self.stepper(
                        "minutes",
                        label,
                        |t, d| {
                            t.practice_minutes =
                                (t.practice_minutes as i32 + d * 5).clamp(0, 240) as u32
                        },
                        cx,
                    ),
                ));
            }
            ModeKind::Hotlap => {}
            ModeKind::Race => {
                session = session
                    .child(setting_row(
                        "Laps",
                        self.stepper(
                            "laps",
                            self.laps.to_string(),
                            |t, d| t.laps = (t.laps as i32 + d).clamp(1, 200) as u32,
                            cx,
                        ),
                    ))
                    .child(setting_row(
                        "Opponents",
                        self.stepper(
                            "opponents",
                            self.opponents.to_string(),
                            |t, d| {
                                t.opponents = (t.opponents as i32 + d).clamp(1, 40) as u32;
                                t.start_position = t.start_position.min(t.opponents + 1);
                            },
                            cx,
                        ),
                    ))
                    .child(setting_row(
                        "AI strength",
                        self.stepper(
                            "ai",
                            format!("{}%", self.ai_level),
                            |t, d| t.ai_level = (t.ai_level as i32 + d).clamp(70, 100) as u32,
                            cx,
                        ),
                    ))
                    .child(setting_row(
                        "Start position",
                        self.stepper(
                            "grid",
                            self.start_position.to_string(),
                            |t, d| {
                                t.start_position = (t.start_position as i32 + d)
                                    .clamp(1, t.opponents as i32 + 1)
                                    as u32
                            },
                            cx,
                        ),
                    ));
            }
        }
        session = session.child(setting_row(
            "Penalties",
            segmented(vec![
                chip("pen-on", "On", self.penalties).on_click(cx.listener(|this, _, _, cx| {
                    this.penalties = true;
                    cx.notify();
                })),
                chip("pen-off", "Off", !self.penalties).on_click(cx.listener(|this, _, _, cx| {
                    this.penalties = false;
                    cx.notify();
                })),
            ]),
        ));

        let road = race::road_temperature(self.time_seconds as f64, self.ambient as f64, 1.0);
        let conditions = panel()
            .flex()
            .flex_col()
            .gap_2()
            .child(section_title("Conditions"))
            .child(setting_row(
                "Time",
                self.stepper(
                    "time",
                    fmt_time(self.time_seconds),
                    |t, d| {
                        t.time_seconds = (t.time_seconds as i32 + d * 1800)
                            .clamp(race::TIME_MIN as i32, race::TIME_MAX as i32)
                            as u32
                    },
                    cx,
                ),
            ))
            .child(setting_row(
                "Temperature",
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(self.stepper(
                        "temp",
                        format!("{}°C", self.ambient),
                        |t, d| t.ambient = (t.ambient + d).clamp(0, 40),
                        cx,
                    ))
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme::muted())
                            .child(format!("road {road:.0}°C")),
                    ),
            ))
            .child(setting_row(
                "Weather",
                div().flex().flex_1().flex_wrap().gap_1().children(
                    self.content.weather.iter().enumerate().map(|(i, w)| {
                        let id = w.id.clone();
                        chip(("weather", i), w.name.clone(), self.weather_id == w.id).on_click(
                            cx.listener(move |this, _, _, cx| {
                                this.weather_id = id.clone();
                                cx.notify();
                            }),
                        )
                    }),
                ),
            ));

        let mode_name = match self.mode {
            ModeKind::Practice => "Practice",
            ModeKind::Hotlap => "Hotlap",
            ModeKind::Race => "Quick race",
        };
        let summary = [
            car.map(|c| c.name.clone()),
            track.map(|t| t.layout.name.clone()),
            Some(mode_name.to_owned()),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join("  ·  ");
        let go_label = if self.game_running {
            "Running…"
        } else {
            "Go!"
        };
        let footer = div()
            .flex()
            .items_center()
            .gap_3()
            .px_4()
            .py_2()
            .border_t_1()
            .border_color(theme::border())
            .bg(theme::sidebar())
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_sm()
                    .text_color(theme::muted())
                    .truncate()
                    .child(summary),
            )
            .child(
                primary_button("go", go_label)
                    .when(self.game_running, |d| d.opacity(0.6))
                    .when(!self.game_running, |d| d.on_click(cx.listener(Self::go))),
            );

        div()
            .flex()
            .size_full()
            .child(self.car_list(cx))
            .child(self.track_list(cx))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .child(
                        div()
                            .id("qd-detail")
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scroll()
                            .p_3()
                            .flex()
                            .flex_col()
                            .gap_3()
                            .child(car_block)
                            .child(track_block)
                            .child(session)
                            .child(conditions),
                    )
                    .child(footer),
            )
    }

    fn cars_page(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let detail =
            match self.selected_car() {
                None => div()
                    .p_4()
                    .text_color(theme::muted())
                    .child("No car selected"),
                Some(car) => {
                    let dir = car.dir.clone();
                    let header = div()
                        .flex()
                        .items_center()
                        .gap_3()
                        .when_some(car.badge.clone(), |d, b| {
                            d.child(
                                img(b)
                                    .size(px(40.))
                                    .flex_shrink_0()
                                    .object_fit(ObjectFit::Contain),
                            )
                        })
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .flex_1()
                                .min_w_0()
                                .child(
                                    div()
                                        .text_xl()
                                        .font_weight(FontWeight::BOLD)
                                        .truncate()
                                        .child(car.name.clone()),
                                )
                                .child(
                                    div()
                                        .text_sm()
                                        .text_color(theme::muted())
                                        .truncate()
                                        .child(car.id.clone()),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_shrink_0()
                                .items_center()
                                .gap_2()
                                .child(button("open-folder", "Open folder").on_click(
                                    cx.listener(move |_, _, _, cx| cx.open_with_system(&dir)),
                                ))
                                .when(car.has_data_acd && !car.has_data_dir, |d| {
                                    d.child(
                                        button("unpack", "Unpack data.acd")
                                            .on_click(cx.listener(Self::unpack_acd)),
                                    )
                                })
                                .when(car.has_data_dir, |d| {
                                    d.child(
                                        button("pack", "Pack data.acd")
                                            .on_click(cx.listener(Self::pack_acd)),
                                    )
                                })
                                .child(
                                    primary_button("drive-car", "Drive")
                                        .on_click(cx.listener(Self::open_quick_drive)),
                                ),
                        );

                    let preview = car
                        .skins
                        .get(self.skin)
                        .and_then(|s| s.preview.clone())
                        .or(car.preview().map(|p| p.to_path_buf()));
                    let skins = div().flex().flex_wrap().gap_2().children(
                        car.skins.iter().enumerate().map(|(i, s)| {
                            let selected = i == self.skin;
                            div()
                                .id(("skin", i))
                                .w(px(112.))
                                .flex()
                                .flex_col()
                                .rounded_sm()
                                .overflow_hidden()
                                .border_1()
                                .border_color(if selected {
                                    theme::accent()
                                } else {
                                    theme::border()
                                })
                                .cursor_pointer()
                                .hover(|d| d.bg(theme::panel_hover()))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.skin = i;
                                    cx.notify();
                                }))
                                .child(picture(s.preview.clone(), 63.).w_full())
                                .child(div().px_1().text_xs().truncate().child(s.name.clone()))
                        }),
                    );

                    let mut info = panel().grid().grid_cols(2).gap_x_6().gap_y_1();
                    for (k, v) in [
                        ("Brand", &car.brand),
                        ("Class", &car.class),
                        ("Year", &car.year),
                        ("Country", &car.country),
                        ("Author", &car.author),
                    ] {
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

                    div()
                        .flex()
                        .flex_col()
                        .gap_3()
                        .p_4()
                        .child(header)
                        .child(picture(preview, 300.).w_full())
                        .child(section_title(format!("Skins ({})", car.skins.len())))
                        .child(skins)
                        .child(section_title("Specs"))
                        .child(info)
                        .when(!car.tags.is_empty(), |d| {
                            d.child(
                                div()
                                    .flex()
                                    .flex_wrap()
                                    .gap_1()
                                    .children(car.tags.iter().map(|t| {
                                        div()
                                            .px_2()
                                            .rounded_sm()
                                            .bg(theme::panel())
                                            .text_xs()
                                            .text_color(theme::muted())
                                            .child(t.clone())
                                    })),
                            )
                        })
                        .when_some(car.description.clone(), |d, desc| {
                            d.child(div().text_sm().child(desc))
                        })
                        .when_some(car.ui_error.clone(), |d, err| {
                            d.child(
                                div()
                                    .text_sm()
                                    .text_color(theme::error())
                                    .child(format!("ui_car.json: {err}")),
                            )
                        })
                }
            };
        div().flex().size_full().child(self.car_list(cx)).child(
            div()
                .id("car-detail")
                .flex_1()
                .min_w_0()
                .h_full()
                .overflow_y_scroll()
                .child(detail),
        )
    }

    fn tracks_page(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let detail =
            match self.selected_track() {
                None => div()
                    .p_4()
                    .text_color(theme::muted())
                    .child("No track selected"),
                Some(t) => {
                    let l = &t.layout;
                    let mut info = panel()
                        .grid()
                        .grid_cols(2)
                        .gap_x_6()
                        .gap_y_1()
                        .child(field("Folder", t.key()));
                    for (k, v) in [
                        ("Country", &l.country),
                        ("City", &l.city),
                        ("Length", &l.length.clone().map(|x| format!("{x} m"))),
                    ] {
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
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_3()
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .text_xl()
                                        .font_weight(FontWeight::BOLD)
                                        .truncate()
                                        .child(l.name.clone()),
                                )
                                .child(
                                    primary_button("drive-track", "Drive")
                                        .on_click(cx.listener(Self::open_quick_drive)),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .gap_3()
                                .child(picture(l.preview.clone(), 240.).flex_1().min_w_0())
                                .child(picture(l.outline.clone(), 240.).w(px(240.))),
                        )
                        .child(info)
                        .when(!l.tags.is_empty(), |d| {
                            d.child(div().flex().flex_wrap().gap_1().children(l.tags.iter().map(
                                |t| {
                                    div()
                                        .px_2()
                                        .rounded_sm()
                                        .bg(theme::panel())
                                        .text_xs()
                                        .text_color(theme::muted())
                                        .child(t.clone())
                                },
                            )))
                        })
                        .when_some(l.description.clone(), |d, desc| {
                            d.child(div().text_sm().child(desc))
                        })
                }
            };
        div().flex().size_full().child(self.track_list(cx)).child(
            div()
                .id("track-detail")
                .flex_1()
                .min_w_0()
                .h_full()
                .overflow_y_scroll()
                .child(detail),
        )
    }

    /// `text_w`: pixel width for wrapping text in the page column (see [`text_w`]).
    fn install_page(&self, text_w: f32, cx: &mut Context<Self>) -> impl IntoElement {
        let dest = self
            .ac_root
            .as_ref()
            .map(|r| format!("Installs into {}", r.join("content").display()))
            .unwrap_or_else(|| "Set the Assetto Corsa folder in Settings first.".into());
        let mut page = div()
            .flex()
            .flex_col()
            .gap_3()
            .p_4()
            .w_full()
            .max_w(px(INSTALL_MAX_W))
            .child(div().text_xl().font_weight(FontWeight::BOLD).child("Install mods"))
            .child(div().w(px(text_w)).text_sm().text_color(theme::muted()).child(
                "Pick a .zip with cars or tracks. Folders are found by ui_car.json, data.acd or ui_track.json, \
                 wherever they sit in the archive.",
            ))
            .child(div().w(px(text_w)).text_sm().text_color(theme::muted()).child(dest))
            .child(
                div().flex().child(
                    primary_button("choose-zip", "Choose archive…")
                        .when(self.busy || self.ac_root.is_none(), |d| d.opacity(0.5))
                        .when(!self.busy && self.ac_root.is_some(), |d| d.on_click(cx.listener(Self::choose_archive))),
                ),
            );
        if let Some(state) = &self.install {
            let name = state
                .archive
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let mut list = panel()
                .flex()
                .flex_col()
                .gap_1()
                .child(div().font_weight(FontWeight::BOLD).child(name))
                .child(
                    div()
                        .w(px(text_w - PANEL_INSET))
                        .text_xs()
                        .text_color(theme::muted())
                        .child(state.archive.display().to_string()),
                )
                .child(section_title(format!("Found {}", state.items.len())));
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
                        .child(
                            div()
                                .w(px(60.))
                                .flex_shrink_0()
                                .text_color(theme::muted())
                                .child(kind),
                        )
                        .child(div().flex_1().min_w_0().truncate().child(item.id.clone()))
                        .child(if item.exists {
                            div()
                                .flex_shrink_0()
                                .text_color(theme::accent())
                                .child("update, overwrites files")
                        } else {
                            div().flex_shrink_0().text_color(theme::ok()).child("new")
                        }),
                );
            }
            page = page.child(list).child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        primary_button(
                            "install",
                            if self.busy {
                                "Installing…"
                            } else {
                                "Install"
                            },
                        )
                        .when(self.busy, |d| d.opacity(0.6))
                        .when(!self.busy, |d| d.on_click(cx.listener(Self::do_install))),
                    )
                    .when(!self.busy, |d| {
                        d.child(button("cancel-install", "Cancel").on_click(cx.listener(
                            |this, _, _, cx| {
                                this.install = None;
                                cx.notify();
                            },
                        )))
                    }),
            );
        }
        div()
            .id("install")
            .size_full()
            .overflow_y_scroll()
            .child(page)
    }

    fn settings_page(&self, text_w: f32, cx: &mut Context<Self>) -> impl IntoElement {
        const LABEL_W: f32 = 170.;
        let label = |text: &'static str| {
            div()
                .w(px(LABEL_W))
                .flex_shrink_0()
                .text_sm()
                .text_color(theme::muted())
                .child(text)
        };
        let row = |text: &'static str, control: gpui::AnyElement| {
            div()
                .flex()
                .items_center()
                .gap_3()
                .child(label(text))
                .child(control)
        };
        // Label above value so long paths get the whole panel width to wrap in.
        let info = |text: &'static str, value: String| {
            div()
                .flex()
                .flex_col()
                .w_full()
                .child(div().text_xs().text_color(theme::muted()).child(text))
                .child(div().w(px(text_w - PANEL_INSET)).text_sm().child(value))
        };
        let hint = |text: &'static str| {
            div()
                .pl(px(LABEL_W + 12.))
                .text_xs()
                .text_color(theme::muted())
                .child(text)
        };

        let race_ini = self
            .ac_root
            .as_ref()
            .and_then(|r| paths::race_ini_path(r))
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "not found (on Linux: run AC from Steam once)".into());

        let game = panel()
            .flex()
            .flex_col()
            .gap_2()
            .child(section_title("Game"))
            .child(row(
                "Assetto Corsa folder",
                div()
                    .flex()
                    .flex_1()
                    .min_w_0()
                    .items_center()
                    .gap_2()
                    .child(grow_input(self.in_root.clone()))
                    .child(
                        button("browse-root", "Browse…").on_click(cx.listener(Self::browse_root)),
                    )
                    .child(
                        button("detect-root", "Auto-detect")
                            .on_click(cx.listener(Self::detect_root)),
                    )
                    .into_any_element(),
            ))
            .when(!cfg!(windows), |d| {
                d.child(row(
                    "Proton script",
                    grow_input(self.in_proton.clone()).into_any_element(),
                ))
                .child(hint(
                    "Leave empty to use the last Proton recorded in the prefix (shown below).",
                ))
            });

        let driver = panel()
            .flex()
            .flex_col()
            .gap_2()
            .child(section_title("Driver"))
            .child(row(
                "Name",
                div()
                    .w(px(280.))
                    .child(self.in_driver.clone())
                    .into_any_element(),
            ))
            .child(row(
                "Nationality",
                div()
                    .w(px(80.))
                    .child(self.in_nation.clone())
                    .into_any_element(),
            ))
            .child(hint("Three-letter code, for example ITA, GBR or JOR."));

        let detected = panel()
            .flex()
            .flex_col()
            .gap_2()
            .child(section_title("Detected"))
            .child(info("race.ini", race_ini))
            .when(!cfg!(windows), |d| {
                let proton = self
                    .ac_root
                    .as_ref()
                    .and_then(|r| launch::find_proton(r))
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| "none found".into());
                d.child(info("Proton", proton))
            })
            .child(info(
                "Settings file",
                ac_core::settings::path()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default(),
            ));

        div().id("settings").size_full().overflow_y_scroll().child(
            div()
                .flex()
                .flex_col()
                .gap_3()
                .p_4()
                .w_full()
                .max_w(px(SETTINGS_MAX_W))
                .child(
                    div()
                        .text_xl()
                        .font_weight(FontWeight::BOLD)
                        .child("Settings"),
                )
                .child(game)
                .child(driver)
                .child(
                    div().flex().justify_end().child(
                        primary_button("save-settings", "Save")
                            .on_click(cx.listener(Self::save_settings)),
                    ),
                )
                .child(detected),
        )
    }
}

impl Render for CmApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let main_w = f32::from(window.viewport_size().width) - SIDEBAR_W;
        let page: gpui::AnyElement = if self.loading && self.page != Page::Browser {
            div()
                .p_4()
                .text_color(theme::muted())
                .child("Loading content…")
                .into_any_element()
        } else {
            match self.page {
                Page::QuickDrive => self.quick_drive_page(cx).into_any_element(),
                Page::Cars => self.cars_page(cx).into_any_element(),
                Page::Tracks => self.tracks_page(cx).into_any_element(),
                Page::Career | Page::SpecialEvents => div().size_full().children(self.events.clone()).into_any_element(),
                Page::Install => self
                    .install_page(text_w(main_w, INSTALL_MAX_W), cx)
                    .into_any_element(),
                Page::Browser => div()
                    .size_full()
                    .children(self.browser.clone())
                    .into_any_element(),
                Page::Game => self
                    .game_page(text_w(main_w, SETTINGS_MAX_W), cx)
                    .into_any_element(),
                Page::Settings => self
                    .settings_page(text_w(main_w, SETTINGS_MAX_W), cx)
                    .into_any_element(),
            }
        };
        let status: SharedString = self
            .status
            .as_ref()
            .map(|(s, _)| s.clone())
            .unwrap_or_default()
            .into();
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
                            .w(px(SIDEBAR_W))
                            .flex_shrink_0()
                            .bg(theme::sidebar())
                            .pt_4()
                            .child(
                                div()
                                    .px_4()
                                    .pb_4()
                                    .text_lg()
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(theme::accent())
                                    .child("Content Manager"),
                            )
                            .child(self.nav_item(Page::QuickDrive, "Quick Drive", cx))
                            .child(self.nav_item(Page::Cars, "Cars", cx))
                            .child(self.nav_item(Page::Tracks, "Tracks", cx))
                            .child(self.nav_item(Page::Career, "Career", cx))
                            .child(self.nav_item(Page::SpecialEvents, "Special Events", cx))
                            .child(self.nav_item(Page::Browser, "Get mods", cx))
                            .child(self.nav_item(Page::Install, "Install mods", cx))
                            .child(self.nav_item(Page::Game, "Game settings", cx))
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
                    .text_color(if status_err {
                        theme::error()
                    } else {
                        theme::muted()
                    })
                    .child(status),
            )
    }
}
