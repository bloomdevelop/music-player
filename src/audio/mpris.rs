use std::time::Duration;
use std::thread;

use mpris_server::{Metadata, Player, Time};
use tokio::sync::mpsc;

#[derive(Debug, Clone)]
pub enum MprisCommand {
    SetPlayback { playing: bool, position: Option<Duration> },
    SetMetadata {
        title: Option<String>,
        artist: Option<String>,
        album: Option<String>,
        length: Option<Duration>,
    },
}

#[derive(Debug, Clone)]
pub enum MprisEvent {
    Play,
    Pause,
    Next,
    Previous,
    SeekTo(Duration),
}

pub struct MprisHandle {
    pub cmd_tx: mpsc::Sender<MprisCommand>,
    pub evt_rx: mpsc::Receiver<MprisEvent>,
}

pub fn start(app_id: &str) -> MprisHandle {
    let (cmd_tx, mut cmd_rx) = mpsc::channel::<MprisCommand>(32);
    let (evt_tx, evt_rx) = mpsc::channel::<MprisEvent>(32);
    let app_id = app_id.to_string();

    // Spawn a tokio task to own the Player and handle commands/events
    thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .enable_io()
            .build()
            .expect("failed to build tokio current-thread runtime for MPRIS");
        let local = tokio::task::LocalSet::new();
        local.block_on(&rt, async move {
            // Build player
            let player = match Player::builder(&app_id)
                .can_play(true)
                .can_pause(true)
                .can_go_next(true)
                .can_go_previous(true)
                .can_seek(true)
                .identity("COSMIC Music Player")
                .build()
                .await
            {
                Ok(p) => p,
                Err(e) => {
                    eprintln!("MPRIS build failed: {e}");
                    return;
                }
            };

            // Connect callbacks to emit events
            let tx = evt_tx.clone();
            player.connect_play(move |_p| {
                let _ = tx.try_send(MprisEvent::Play);
            });
            
            let tx = evt_tx.clone();
            player.connect_pause(move |_p| {
                let _ = tx.try_send(MprisEvent::Pause);
            });
            
            let tx = evt_tx.clone();
            player.connect_next(move |_p| {
                let _ = tx.try_send(MprisEvent::Next);
            });
            
            let tx = evt_tx.clone();
            player.connect_previous(move |_p| {
                let _ = tx.try_send(MprisEvent::Previous);
            });
            
            let tx = evt_tx.clone();
            player.connect_seek(move |_p, pos| {
                let dur = Duration::from_micros(pos.as_micros() as u64);
                let _ = tx.try_send(MprisEvent::SeekTo(dur));
            });

            // Run event loop for mpris_server on the local set
            tokio::task::spawn_local(player.run());

            // Command loop
            while let Some(cmd) = cmd_rx.recv().await {
                match cmd {
                    MprisCommand::SetPlayback { playing, position } => {
                        let _ = player
                            .set_playback_status(if playing {
                                mpris_server::PlaybackStatus::Playing
                            } else {
                                mpris_server::PlaybackStatus::Paused
                            })
                            .await;
                        if let Some(pos) = position {
                            let _ = player.seeked(Time::from_millis(pos.as_millis() as i64)).await;
                        }
                    }
                    MprisCommand::SetMetadata {
                        title,
                        artist,
                        album,
                        length,
                    } => {
                        let mut builder = Metadata::builder();
                        if let Some(t) = title { builder = builder.title(t); }
                        if let Some(a) = album { builder = builder.album(a); }
                        if let Some(ar) = artist { builder = builder.artist([ar]); }
                        if let Some(d) = length { builder = builder.length(Time::from_micros(d.as_micros() as i64)); }
                        let _ = player.set_metadata(builder.build()).await;
                    }
                }
            }
        });
    });

    MprisHandle { cmd_tx, evt_rx }
}
