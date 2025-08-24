// SPDX-License-Identifier: MPL-2.0

use crate::config::Config;
use crate::fl;
use cosmic::app::context_drawer;
use cosmic::cosmic_config::{self, CosmicConfigEntry};
use cosmic::iced::alignment::Vertical;
use cosmic::iced::{Alignment, Length, Subscription};
use cosmic::prelude::*;
use cosmic::widget::{self, icon, menu, nav_bar};
use cosmic::{cosmic_theme, theme};
use futures_util::SinkExt;
use music_player::audio::backend::MediaPlayer;
use music_player::audio::mpris::{self, MprisCommand, MprisEvent};
use music_player::audio::queue::{scan_music_dir, Queue};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::sync::mpsc;

const REPOSITORY: &str = env!("CARGO_PKG_REPOSITORY");
const APP_ICON: &[u8] = include_bytes!("../resources/icons/hicolor/scalable/apps/icon.svg");

/// The application model stores app-specific state used to describe its interface and
/// drive its logic.
pub struct AppModel {
    /// Application state which is managed by the COSMIC runtime.
    core: cosmic::Core,
    /// Display a context drawer with the designated page if defined.
    context_page: ContextPage,
    /// Contains items assigned to the nav bar panel.
    nav: nav_bar::Model,
    /// Key bindings for the application's menu bar.
    key_binds: HashMap<menu::KeyBind, MenuAction>,
    // Configuration data that persists between application runs.
    config: Config,
    /// Optional audio backend (GStreamer-backed media player).
    audio: Option<MediaPlayer>,
    /// Playback queue
    queue: Queue,
    /// Library tracks scanned from user's Music directory
    library_tracks: Vec<PathBuf>,
    /// Current playback position in milliseconds
    position_ms: u64,
    /// Current track duration in milliseconds
    duration_ms: u64,
    /// Whether media is currently playing
    is_playing: bool,
    /// After loading a track, wait for tags to arrive and push metadata once
    mpris_needs_metadata_flush: bool,
    /// MPRIS command channel (to MPRIS task)
    mpris_tx: Option<mpsc::Sender<MprisCommand>>,
    /// MPRIS event channel (from MPRIS task)
    mpris_rx: Option<mpsc::Receiver<MprisEvent>>,
}

/// Messages emitted by the application and its widgets.
#[derive(Debug, Clone)]
pub enum Message {
    OpenRepositoryUrl,
    SubscriptionChannel,
    ToggleContextPage(ContextPage),
    UpdateConfig(Config),
    LaunchUrl(String),
    // Playback controls
    Play,
    Pause,
    Stop,
    LoadPath(String),
    /// Library scan completed
    LibraryScanned(Vec<PathBuf>),
    /// Add a path to the playback queue without starting playback
    Enqueue(String),
    Next,
    Prev,
    /// Periodic UI tick to update position/duration
    Tick,
    /// Seek to a fraction of the current duration (0.0 - 1.0)
    SeekTo(f32),
}

/// Create a COSMIC application from the app model
impl cosmic::Application for AppModel {
    /// The async executor that will be used to run your application's commands.
    type Executor = cosmic::executor::Default;

    /// Data that your application receives to its init method.
    type Flags = ();

    /// Messages which the application and its widgets will emit.
    type Message = Message;

    /// Unique identifier in RDNN (reverse domain name notation) format.
    const APP_ID: &'static str = "io.github.bloomdevelop.music-player";

    fn core(&self) -> &cosmic::Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut cosmic::Core {
        &mut self.core
    }

    /// Initializes the application with any given flags and startup commands.
    fn init(
        core: cosmic::Core,
        _flags: Self::Flags,
    ) -> (Self, Task<cosmic::Action<Self::Message>>) {
        // Create a nav bar with three page items.
        let mut nav = nav_bar::Model::default();

        nav.insert()
            .text(fl!("nav-library-label"))
            .data::<Page>(Page::Page1)
            .icon(icon::from_name("folder-symbolic"))
            .activate();

        nav.insert()
            .text(fl!("nav-now-playing-label"))
            .data::<Page>(Page::Page2)
            .icon(icon::from_name("folder-music-symbolic"));

        // Construct the app model with the runtime's core.
        let mut app = AppModel {
            core,
            context_page: ContextPage::default(),
            nav,
            key_binds: HashMap::new(),
            // Optional configuration file for an application.
            config: cosmic_config::Config::new(Self::APP_ID, Config::VERSION)
                .map(|context| {
                    Config::get_entry(&context).unwrap_or_else(|(_errors, config)| {
                        // for why in errors {
                        //     tracing::error!(%why, "error loading app config");
                        // }

                        config
                    })
                })
                .unwrap_or_default(),
            // Try to initialize the audio backend. If it fails, keep None and continue
            audio: match MediaPlayer::new() {
                Ok(player) => {
                    // Start a thread to watch the GStreamer bus for EOS/errors.
                    let _ = player.start_bus_watch();
                    Some(player)
                }
                Err(err) => {
                    eprintln!("failed to initialize audio backend: {err}");
                    None
                }
            },
            // Start with an empty queue
            queue: Queue::new(),
            // Library will be populated asynchronously
            library_tracks: Vec::new(),
            position_ms: 0,
            duration_ms: 0,
            is_playing: false,
            mpris_needs_metadata_flush: false,
            mpris_tx: None,
            mpris_rx: None,
        };

        // Initialize MPRIS manager
        let mpris = mpris::start(Self::APP_ID);
        app.mpris_tx = Some(mpris.cmd_tx.clone());
        app.mpris_rx = Some(mpris.evt_rx);

        // Create a startup command that sets the window title.
        let command = app.update_title();

        // Start scanning the user's Music directory in the background and
        // send a LibraryScanned message when complete.
        let home = std::env::var("HOME").unwrap_or_else(|_| String::from("."));
        let music_dir = std::path::PathBuf::from(format!("{}/Music", home));
        let scan_task = cosmic::task::future(async move {
            let tracks = scan_music_dir(music_dir);
            Message::LibraryScanned(tracks)
        });

        (app, Task::batch(vec![command, scan_task]))
    }

    /// Display a context drawer if the context page is requested.
    fn context_drawer(&self) -> Option<context_drawer::ContextDrawer<'_, Self::Message>> {
        if !self.core.window.show_context {
            return None;
        }

        Some(match self.context_page {
            ContextPage::About => context_drawer::context_drawer(
                self.about(),
                Message::ToggleContextPage(ContextPage::About),
            )
            .title(fl!("about")),
            ContextPage::Queue => context_drawer::context_drawer(
                self.queue_context_view(),
                Message::ToggleContextPage(ContextPage::Queue),
            )
            .title(fl!("queue-context-title")),
        })
    }

    /// Elements to pack at the start of the header bar.
    fn header_start(&'_ self) -> Vec<Element<'_, Self::Message>> {
        let menu_bar = menu::bar(vec![menu::Tree::with_children(
            menu::root(fl!("view")).apply(Element::from),
            menu::items(
                &self.key_binds,
                vec![menu::Item::Button(fl!("about"), None, MenuAction::About)],
            ),
        )]);

        vec![menu_bar.into()]
    }

    fn header_end(&self) -> Vec<Element<'_, Self::Message>> {
        let queue_button = widget::button::text(fl!("queue-button", count = self.queue.len()))
            .leading_icon(icon::from_name("view-list-symbolic"))
            .on_press(Message::ToggleContextPage(ContextPage::Queue));

        vec![queue_button.into()]
    }

    /// Enables the COSMIC application to create a nav bar with this model.
    fn nav_model(&self) -> Option<&nav_bar::Model> {
        Some(&self.nav)
    }

    /// Called when a nav item is selected.
    fn on_nav_select(&mut self, id: nav_bar::Id) -> Task<cosmic::Action<Self::Message>> {
        // Activate the page in the model.
        self.nav.activate(id);

        self.update_title()
    }

    /// Register subscriptions for this application.
    ///
    /// Subscriptions are long-running async tasks running in the background which
    /// emit messages to the application through a channel. They are started at the
    /// beginning of the application, and persist through its lifetime.
    fn subscription(&self) -> Subscription<Self::Message> {
        struct MySubscription;

        Subscription::batch(vec![
            // Create a subscription which emits updates through a channel.
            Subscription::run_with_id(
                std::any::TypeId::of::<MySubscription>(),
                cosmic::iced::stream::channel(4, move |mut channel| async move {
                    _ = channel.send(Message::SubscriptionChannel).await;

                    futures_util::future::pending().await
                }),
            ),
            // Watch for application configuration changes.
            self.core()
                .watch_config::<Config>(Self::APP_ID)
                .map(|update| {
                    // for why in update.errors {
                    //     tracing::error!(?why, "app config error");
                    // }

                    Message::UpdateConfig(update.config)
                }),
            // Periodic tick to update seek bar (every 200ms)
            cosmic::iced::time::every(Duration::from_millis(200)).map(|_| Message::Tick),
        ])
    }

    /// Handles messages emitted by the application and its widgets.
    ///
    /// Tasks may be returned for asynchronous execution of code in the background
    /// on the application's async runtime.
    fn update(&mut self, message: Self::Message) -> Task<cosmic::Action<Self::Message>> {
        match message {
            Message::OpenRepositoryUrl => {
                _ = open::that_detached(REPOSITORY);
            }

            Message::SubscriptionChannel => {
                // For example purposes only.
            }

            Message::ToggleContextPage(context_page) => {
                if self.context_page == context_page {
                    // Close the context drawer if the toggled context page is the same.
                    self.core.window.show_context = !self.core.window.show_context;
                } else {
                    // Open the context drawer to display the requested context page.
                    self.context_page = context_page;
                    self.core.window.show_context = true;
                }
            }

            Message::UpdateConfig(config) => {
                self.config = config;
            }

            Message::LaunchUrl(url) => match open::that_detached(&url) {
                Ok(()) => {}
                Err(err) => {
                    eprintln!("failed to open {url:?}: {err}");
                }
            },

            // Playback messages
            Message::Play => {
                // Forces MPRIS to flush metadata if it's not already done (this is a workaround for now)
                self.mpris_needs_metadata_flush = true;

                if let Some(player) = &self.audio {
                    // If there's a current queue track and nothing loaded, load it.
                    if let Some(track) = self.queue.current() {
                        if let Err(err) = player.load_path(track) {
                            eprintln!("failed to load track from queue: {err}");
                        }
                    }

                    if let Err(err) = player.play() {
                        eprintln!("failed to play: {err}");
                    } else {
                        self.is_playing = true;
                        if let Some(tx) = &self.mpris_tx {
                            let _ = tx.try_send(MprisCommand::SetPlayback {
                                playing: true,
                                position: player.position(),
                            });
                        }
                    }

                    // If we haven't pushed metadata yet for this track, try now once tags are parsed
                    if self.mpris_needs_metadata_flush {
                        if let Some(tx) = &self.mpris_tx {
                            // Read current metadata and duration
                            if let Some(ap) = &self.audio {
                                let md = ap.metadata();
                                println!("mpris: metadata: {md:?}");
                                let have_any =
                                    md.title.is_some() || md.artist.is_some() || md.album.is_some();
                                let len = ap.duration();
                                if have_any || len.is_some() {
                                    let _ = tx.try_send(MprisCommand::SetMetadata {
                                        title: md.title,
                                        artist: md.artist,
                                        album: md.album,
                                        length: len,
                                    });
                                    self.mpris_needs_metadata_flush = false;
                                }
                            }
                        }
                    }
                }
            }

            Message::Pause => {
                if let Some(player) = &self.audio {
                    if let Err(err) = player.pause() {
                        eprintln!("failed to pause: {err}");
                    } else {
                        self.is_playing = false;
                        if let Some(tx) = &self.mpris_tx {
                            let _ = tx.try_send(MprisCommand::SetPlayback {
                                playing: false,
                                position: player.position(),
                            });
                        }
                    }
                }
            }

            Message::Stop => {
                if let Some(player) = &self.audio {
                    if let Err(err) = player.stop() {
                        eprintln!("failed to stop: {err}");
                    } else {
                        self.is_playing = false;
                        if let Some(tx) = &self.mpris_tx {
                            let _ = tx.try_send(MprisCommand::SetPlayback {
                                playing: false,
                                position: Some(Duration::from_millis(0)),
                            });
                        }
                    }
                }
            }

            Message::LoadPath(path) => {
                if let Some(player) = &self.audio {
                    let p = Path::new(&path);
                    // Ensure queue knows about this selection so Next/Prev operate
                    self.queue.select_or_push(PathBuf::from(p));
                    if let Err(err) = player.load_path(p) {
                        eprintln!("failed to load path {path}: {err}");
                    } else if let Err(err) = player.play() {
                        eprintln!("failed to start playback: {err}");
                    } else {
                        // Defer metadata send until tags parsed by GStreamer bus
                        self.mpris_needs_metadata_flush = true;

                        self.is_playing = true;
                        if let Some(tx) = &self.mpris_tx {
                            if let Some(ap) = &self.audio {
                                let _ = tx.try_send(MprisCommand::SetPlayback {
                                    playing: true,
                                    position: ap.position(),
                                });
                            }
                        }
                    }

                    if self.mpris_needs_metadata_flush {
                        if let Some(tx) = &self.mpris_tx {
                            // Read current metadata and duration
                            if let Some(ap) = &self.audio {
                                let md = ap.metadata();
                                println!("mpris: metadata: {md:?}");
                                let have_any =
                                    md.title.is_some() || md.artist.is_some() || md.album.is_some();
                                let len = ap.duration();
                                if have_any || len.is_some() {
                                    let _ = tx.try_send(MprisCommand::SetMetadata {
                                        title: md.title,
                                        artist: md.artist,
                                        album: md.album,
                                        length: len,
                                    });
                                    self.mpris_needs_metadata_flush = false;
                                }
                            }
                        }
                    }
                }
            }

            Message::LibraryScanned(tracks) => {
                self.library_tracks = tracks;
            }

            Message::Enqueue(path) => {
                self.queue.push(std::path::PathBuf::from(path));
            }

            Message::Next => {
                if let Some(next) = self.queue.next().cloned() {
                    if let Some(player) = &self.audio {
                        if let Err(err) = player.load_path(&next) {
                            eprintln!("failed to load next track: {err}");
                        } else if let Err(err) = player.play() {
                            eprintln!("failed to play next track: {err}");
                        } else {
                            self.is_playing = true;
                            // Defer metadata send until tags parsed by GStreamer bus
                            self.mpris_needs_metadata_flush = true;
                            if let Some(tx) = &self.mpris_tx {
                                let _ = tx.try_send(MprisCommand::SetPlayback {
                                    playing: true,
                                    position: player.position(),
                                });
                            }
                        }
                    }
                }
            }

            Message::Prev => {
                if let Some(prev) = self.queue.prev().cloned() {
                    if let Some(player) = &self.audio {
                        if let Err(err) = player.load_path(&prev) {
                            eprintln!("failed to load prev track: {err}");
                        } else if let Err(err) = player.play() {
                            eprintln!("failed to play prev track: {err}");
                        } else {
                            self.is_playing = true;
                            // Defer metadata send until tags parsed by GStreamer bus
                            self.mpris_needs_metadata_flush = true;
                            if let Some(tx) = &self.mpris_tx {
                                let _ = tx.try_send(MprisCommand::SetPlayback {
                                    playing: true,
                                    position: player.position(),
                                });
                            }
                        }
                    }
                }
            }

            Message::Tick => {
                if let Some(player) = &self.audio {
                    if let Some(dur) = player.duration() {
                        self.duration_ms = dur.as_millis() as u64;
                    }
                    if let Some(pos) = player.position() {
                        self.position_ms = pos.as_millis() as u64;
                    }

                    // Auto-advance on end-of-stream
                    if player.take_eos() {
                        if let Some(next) = self.queue.next().cloned() {
                            if let Err(err) = player.load_path(&next) {
                                eprintln!("failed to load next track at EOS: {err}");
                            } else if let Err(err) = player.play() {
                                eprintln!("failed to play next track at EOS: {err}");
                            } else {
                                self.is_playing = true;
                                // Defer metadata send until tags parsed by GStreamer bus
                                self.mpris_needs_metadata_flush = true;
                                if let Some(tx) = &self.mpris_tx {
                                    let _ = tx.try_send(MprisCommand::SetPlayback {
                                        playing: true,
                                        position: player.position(),
                                    });
                                }
                            }
                        }
                    }

                    // Periodic MPRIS position update
                    if let Some(tx) = &self.mpris_tx {
                        let _ = tx.try_send(MprisCommand::SetPlayback {
                            playing: self.is_playing,
                            position: player.position(),
                        });
                    }

                    // Drain incoming MPRIS events and act on them
                    if let Some(rx) = &mut self.mpris_rx {
                        while let Ok(evt) = rx.try_recv() {
                            match evt {
                                MprisEvent::Play => {
                                    // Inline behavior of Message::Play
                                    if let Some(track) = self.queue.current() {
                                        let _ = player.load_path(track);
                                    }
                                    let _ = player.play();
                                    self.is_playing = true;
                                }
                                MprisEvent::Pause => {
                                    let _ = player.pause();
                                    self.is_playing = false;
                                }
                                MprisEvent::Next => {
                                    if let Some(next) = self.queue.next().cloned() {
                                        let _ = player.load_path(&next);
                                        let _ = player.play();
                                        self.is_playing = true;
                                    }
                                }
                                MprisEvent::Previous => {
                                    if let Some(prev) = self.queue.prev().cloned() {
                                        let _ = player.load_path(&prev);
                                        let _ = player.play();
                                        self.is_playing = true;
                                    }
                                }
                                MprisEvent::SeekTo(d) => {
                                    let _ = player.seek(d);
                                    self.position_ms = d.as_millis() as u64;
                                }
                            }
                        }
                    }
                }
            }

            Message::SeekTo(frac) => {
                if let Some(player) = &self.audio {
                    if self.duration_ms > 0 {
                        let frac = frac.clamp(0.0, 1.0);
                        let target_ms = (self.duration_ms as f32 * frac) as u64;
                        let _ = player.seek(Duration::from_millis(target_ms));
                        // Reflect immediately in UI
                        self.position_ms = target_ms;
                        if let Some(tx) = &self.mpris_tx {
                            let _ = tx.try_send(MprisCommand::SetPlayback {
                                playing: self.is_playing,
                                position: Some(Duration::from_millis(target_ms)),
                            });
                        }
                    }
                }
            }
        }
        Task::none()
    }

    /// Describes the interface based on the current state of the application model.
    ///
    /// Application events will be processed through the view. Any messages emitted by
    /// events received by widgets will be passed to the update method.
    fn view(&self) -> Element<'_, Self::Message> {
        // Main content
        let content = pages::page_view(self);

        // Footer with seek bar
        let pos_ms = self.position_ms;
        let dur_ms = self.duration_ms;
        let (elapsed_str, total_str, frac) = if dur_ms > 0 {
            (
                format_time(pos_ms),
                format_time(dur_ms),
                (pos_ms as f32) / (dur_ms as f32),
            )
        } else {
            (String::from("0:00"), String::from("0:00"), 0.0)
        };

        let slider = widget::slider(0.0..=1.0, frac, Message::SeekTo)
            .step(0.001)
            .width(Length::Fill);

        // Play/Pause icon button per libcosmic
        let play_pause_btn = if self.is_playing {
            widget::button::icon(icon::from_name("media-playback-pause-symbolic"))
                .tooltip(fl!("tooltip-pause-button"))
                .on_press(Message::Pause)
        } else {
            widget::button::icon(icon::from_name("media-playback-start-symbolic"))
                .tooltip(fl!("tooltip-play-button"))
                .on_press(Message::Play)
        };

        let prev_btn = widget::button::icon(icon::from_name("media-skip-backward-symbolic"))
            .tooltip(fl!("tooltip-prev-button"))
            .on_press(Message::Prev);

        let next_btn = widget::button::icon(icon::from_name("media-skip-forward-symbolic"))
            .tooltip(fl!("tooltip-next-button"))
            .on_press(Message::Next);

        // Build a label for the current song: prefer metadata, else filename, else placeholder
        let song_label = if let Some(player) = &self.audio {
            let md = player.metadata();
            match (md.title, md.artist) {
                (Some(title), Some(artist)) => format!("{title} — {artist}"),
                (Some(title), None) => title,
                (None, Some(artist)) => artist,
                (None, None) => self
                    .queue
                    .current()
                    .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
                    .unwrap_or_else(|| String::from("No track")),
            }
        } else {
            self.queue
                .current()
                .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
                .unwrap_or_else(|| String::from("No track"))
        };

        let footer_controls = widget::row()
            .align_y(Vertical::Center)
            .spacing(8)
            .push(prev_btn)
            .push(play_pause_btn)
            .push(next_btn)
            .push(widget::text(elapsed_str))
            .push(slider)
            .push(widget::text(total_str))
            .width(Length::Fill);

        let footer = widget::container(
            widget::container(
                widget::column()
                    .spacing(6)
                    .push(widget::text(song_label).width(Length::Fill))
                    .push(footer_controls)
                    .width(Length::Fill),
            )
            .padding([8, 12])
            .class(cosmic::theme::Container::Card),
        )
        .padding([7, 4])
        .width(Length::Fill);

        widget::column()
            .push(content)
            .push(footer)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }
}

mod pages;

impl AppModel {
    /// The about page for this app.
    pub fn about(&self) -> Element<'static, Message> {
        let cosmic_theme::Spacing { space_xxs, .. } = theme::active().cosmic().spacing;

        let icon = widget::svg(widget::svg::Handle::from_memory(APP_ICON));

        let title = widget::text::title3(fl!("app-title"));

        let hash = env!("VERGEN_GIT_SHA");
        let short_hash: String = hash.chars().take(7).collect();
        let date = env!("VERGEN_GIT_COMMIT_DATE");

        let link = widget::button::link(REPOSITORY)
            .on_press(Message::OpenRepositoryUrl)
            .padding(0);

        widget::column()
            .push(icon)
            .push(title)
            .push(link)
            .push(
                widget::button::link(fl!(
                    "git-description",
                    hash = short_hash.as_str(),
                    date = date
                ))
                .on_press(Message::LaunchUrl(format!("{REPOSITORY}/commits/{hash}")))
                .padding(0),
            )
            .align_x(Alignment::Center)
            .spacing(space_xxs)
            .into()
    }

    /// Updates the header and window titles.
    pub fn update_title(&mut self) -> Task<cosmic::Action<Message>> {
        let mut window_title = fl!("app-title");

        if let Some(page) = self.nav.text(self.nav.active()) {
            window_title.push_str(" — ");
            window_title.push_str(page);
        }

        if let Some(id) = self.core.main_window_id() {
            self.set_window_title(window_title, id)
        } else {
            Task::none()
        }
    }

    /// Read-only access to scanned library tracks.
    pub fn library_tracks(&self) -> &[PathBuf] {
        &self.library_tracks
    }

    /// The queue context page showing the current playback queue.
    /// TODO)) Add fallback when the queue are empty
    pub fn queue_context_view(&self) -> Element<'static, Message> {
        use cosmic::iced::Length;

        let mut items = widget::column().spacing(4);
        let current = self.queue.current().cloned();
        for path in self.queue.tracks() {
            let label = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.to_string_lossy().into_owned());

            let is_current = current.as_ref().map(|p| p == path).unwrap_or(false);
            // Tint current track instead of using a play indicator

            let row = widget::row()
                .spacing(8)
                .align_y(Vertical::Center)
                .push(
                    widget::button::icon(icon::from_name("media-playback-start-symbolic"))
                        .on_press(Message::LoadPath(path.to_string_lossy().into_owned())),
                )
                .push(widget::text(label.clone()))
                .width(Length::Fill);

            let mut container = widget::container(row).padding([4, 8]);

            if is_current {
                container = container.class(cosmic::theme::Container::Primary);
            }

            items = items.push(container);
        }

        widget::column()
            .spacing(12)
            .push(widget::scrollable(items))
            .width(Length::Fill)
            .into()
    }
}

/// Format milliseconds as m:ss or h:mm:ss
fn format_time(ms: u64) -> String {
    let total_secs = (ms / 1000) as u64;
    let hours = total_secs / 3600;
    let minutes = (total_secs % 3600) / 60;
    let seconds = total_secs % 60;

    if hours > 0 {
        format!("{}:{:02}:{:02}", hours, minutes, seconds)
    } else {
        format!("{}:{:02}", minutes, seconds)
    }
}

// ...existing code...

/// The page to display in the application.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Page {
    Page1,
    Page2,
}

/// The context page to display in the context drawer.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub enum ContextPage {
    #[default]
    About,
    Queue,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MenuAction {
    About,
}

impl menu::action::MenuAction for MenuAction {
    type Message = Message;

    fn message(&self) -> Self::Message {
        match self {
            MenuAction::About => Message::ToggleContextPage(ContextPage::About),
        }
    }
}
