//! Plays cloud-voice audio for Kairos Read through GStreamer, one clip at a time, at the
//! reader's speed (scaletempo keeps the pitch).

use gstreamer as gst;
use gst::prelude::*;
use gtk::glib;
use std::cell::RefCell;
use std::rc::Rc;

type Done = Box<dyn FnOnce(bool)>;

#[derive(Default)]
pub struct SpeechPlayer {
    current: RefCell<Option<Playing>>,
    count: RefCell<u64>,
}

struct Playing {
    id: u64,
    pipeline: gst::Element,
    _watch: gst::bus::BusWatchGuard,
    file: std::path::PathBuf,
    done: Option<Done>,
}

impl SpeechPlayer {
    pub fn play(self: &Rc<Self>, audio: &[u8], rate: f64, done: impl FnOnce(bool) + 'static) -> Result<(), String> {
        self.stop();
        gst::init().map_err(|e| format!("Audio playback isn\u{2019}t available: {e}"))?;
        let id = {
            let mut c = self.count.borrow_mut();
            *c += 1;
            *c
        };
        let dir = glib::user_runtime_dir();
        let file = dir.join(format!("kairos-voice-{}-{id}.mp3", std::process::id()));
        std::fs::write(&file, audio).map_err(|e| e.to_string())?;
        let uri = glib::filename_to_uri(&file, None).map_err(|e| e.to_string())?;
        let pipeline = gst::ElementFactory::make("playbin").property("uri", uri.as_str()).build().map_err(|e| e.to_string())?;
        let rate = rate.clamp(0.5, 2.0);
        if (rate - 1.0).abs() > 0.01 {
            if let Ok(tempo) = gst::ElementFactory::make("scaletempo").build() {
                pipeline.set_property("audio-filter", &tempo);
            }
        }
        let bus = pipeline.bus().ok_or("no bus")?;
        let weak = Rc::downgrade(self);
        let mut rate_set = false;
        let watch = bus
            .add_watch_local(move |_, msg| {
                let Some(me) = weak.upgrade() else { return glib::ControlFlow::Break };
                match msg.view() {
                    gst::MessageView::AsyncDone(_) if !rate_set && (rate - 1.0).abs() > 0.01 => {
                        rate_set = true;
                        if let Some(p) = me.current.borrow().as_ref().filter(|p| p.id == id) {
                            let _ = p.pipeline.seek(
                                rate,
                                gst::SeekFlags::FLUSH | gst::SeekFlags::ACCURATE,
                                gst::SeekType::Set,
                                gst::ClockTime::ZERO,
                                gst::SeekType::None,
                                gst::ClockTime::NONE,
                            );
                        }
                    }
                    gst::MessageView::Eos(_) | gst::MessageView::Error(_) => me.finish(id, false),
                    _ => {}
                }
                glib::ControlFlow::Continue
            })
            .map_err(|e| e.to_string())?;
        pipeline.set_state(gst::State::Playing).map_err(|e| e.to_string())?;
        *self.current.borrow_mut() = Some(Playing { id, pipeline, _watch: watch, file, done: Some(Box::new(done)) });
        Ok(())
    }

    fn finish(&self, id: u64, stopped: bool) {
        let taken = {
            let mut cur = self.current.borrow_mut();
            if cur.as_ref().map(|p| p.id) != Some(id) {
                return;
            }
            cur.take()
        };
        if let Some(mut p) = taken {
            let _ = p.pipeline.set_state(gst::State::Null);
            let _ = std::fs::remove_file(&p.file);
            if let Some(done) = p.done.take() {
                done(stopped);
            }
        }
    }

    pub fn stop(&self) {
        let id = self.current.borrow().as_ref().map(|p| p.id);
        if let Some(id) = id {
            self.finish(id, true);
        }
    }

    pub fn pause(&self) {
        if let Some(p) = self.current.borrow().as_ref() {
            let _ = p.pipeline.set_state(gst::State::Paused);
        }
    }

    pub fn resume(&self) {
        if let Some(p) = self.current.borrow().as_ref() {
            let _ = p.pipeline.set_state(gst::State::Playing);
        }
    }
}
