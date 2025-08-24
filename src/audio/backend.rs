use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use gstreamer as gst;
use gst::prelude::*;
use std::path::Path;
use std::thread;
use std::time::Duration;
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};
// Backend focuses purely on GStreamer playback. MPRIS is handled by a separate module.

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct TrackMetadata {
    pub title: Option<String>,
    pub album: Option<String>,
    pub artist: Option<String>,
}

#[derive(Clone)]
pub struct MediaPlayer {
    playbin: gst::Element,
    eos_flag: Arc<AtomicBool>,
    metadata: Arc<Mutex<TrackMetadata>>, // updated from bus tag messages
}

impl MediaPlayer {
    pub fn new() -> Result<Self> {
        gst::init()?;
        let playbin = gst::ElementFactory::make("playbin")
            .build()
            .map_err(|_| anyhow!("Failed to create playbin element"))?;
        Ok(Self {
            playbin,
            eos_flag: Arc::new(AtomicBool::new(false)),
            metadata: Arc::new(Mutex::new(TrackMetadata::default())),
        })
    }

    pub fn path_to_uri(path: &Path) -> Result<String> {
        let abs = std::fs::canonicalize(path)?;
        let s = abs.to_str().ok_or_else(|| anyhow!("Invalid Path"))?;

        let s = s.replace(' ', "%20");
        Ok(format!("file://{}", s)) 
    }

    pub fn set_uri(&self, uri: &str) -> Result<()> {
        self.playbin.set_property("uri", &uri);
        Ok(())
    }

    pub fn load_path(&self, path: &Path) -> Result<()> {
        let uri = Self::path_to_uri(path)?;
        self.set_uri(&uri)
    }

    pub fn play(&self) -> Result<()> {
        self.playbin
            .set_state(gst::State::Playing)
            .map_err(|e| anyhow!("Failed to set state to Playing: {}", e))?;
        // reset EOS when we start playing
        self.eos_flag.store(false, Ordering::SeqCst);
        Ok(())
    }

    pub fn pause(&self) -> Result<()> {
        self.playbin
            .set_state(gst::State::Paused)
            .map_err(|e| anyhow!("Failed to set state to Playing: {}", e))?;
        Ok(())
    }

    pub fn stop(&self) -> Result<()> {
        self.playbin
            .set_state(gst::State::Ready)
            .map_err(|e| anyhow!("Failed to set state to Null: {}", e))?;
        Ok(())
    }

    /// Query the current playback position.
    pub fn position(&self) -> Option<Duration> {
        self.playbin
            .query_position::<gst::ClockTime>()
            .map(|ct| Duration::from_nanos(ct.nseconds()))
    }

    /// Query the total duration of the currently loaded media.
    pub fn duration(&self) -> Option<Duration> {
        self.playbin
            .query_duration::<gst::ClockTime>()
            .map(|ct| Duration::from_nanos(ct.nseconds()))
    }

    /// Seek to the specified absolute position.
    pub fn seek(&self, position: Duration) -> Result<()> {
        let clock_time = gst::ClockTime::from_nseconds(position.as_nanos() as u64);
        self.playbin
            .seek_simple(
                gst::SeekFlags::FLUSH | gst::SeekFlags::KEY_UNIT,
                clock_time,
            )
            .map_err(|e| anyhow!("Failed to seek: {}", e))?;
        Ok(())
    }

    pub fn start_bus_watch(&self) -> thread::JoinHandle<()> {
        let bus = self.playbin.bus().expect("playbin has no bus");
        let playbin = self.playbin.clone();
        let eos_flag = self.eos_flag.clone();
        let metadata = self.metadata.clone();

        thread::spawn(move || {
            for msg in bus.iter_timed(gst::ClockTime::NONE) {
                match msg.view() {
                    gst::MessageView::Eos(..) => {
                        eprint!("GStreamer: End-Of-Stream");
                        // Signal EOS and reset to Ready so a new URI can be loaded
                        eos_flag.store(true, Ordering::SeqCst);
                        let _ = playbin.set_state(gst::State::Ready);
                    }

                    gst::MessageView::Tag(tag_msg) => {
                        let tags = tag_msg.tags();
                        if let Ok(mut guard) = metadata.lock() {
                            if let Some(v) = tags.get::<gst::tags::Title>() {
                                guard.title = Some(v.get().to_string());
                            }
                            if let Some(v) = tags.get::<gst::tags::Album>() {
                                guard.album = Some(v.get().to_string());
                            }
                            if let Some(v) = tags.get::<gst::tags::Artist>() {
                                guard.artist = Some(v.get().to_string());
                            }
                        }
                    }

                    gst::MessageView::Error(err) => {
                        eprint!(
                            "GStreamer Error from {:?}: {} ({:?})",
                            err.src().map(|s| s.path_string()),
                            err.error(),
                            err.debug()
                        );
                        break;
                    }

                    _ => {}
                }

                std::thread::sleep(Duration::from_millis(10));
            }
        })
    }

    /// Check and clear EOS flag set by the bus watcher.
    pub fn take_eos(&self) -> bool {
        self.eos_flag.swap(false, Ordering::SeqCst)
    }

    /// Get the last-known metadata extracted from tags.
    pub fn metadata(&self) -> TrackMetadata {
        if let std::result::Result::Ok(guard) = self.metadata.lock() {
            guard.clone()
        } else {
            TrackMetadata::default()
        }
    }

    // MPRIS is managed by audio::mpris
}

impl Drop for MediaPlayer {
    fn drop(&mut self) {
        let _ = self.playbin.set_state(gst::State::Null);
    }
}