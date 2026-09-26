//! The engine side of `pg.audio`: kira's `AudioManager` and Love2D-style sources on top of it.
//! It has no Lua dependency.
//!
//! A [`Source`] is one sound that can be played, paused and stopped. Each play hands kira a new
//! sound; the source keeps its handle and tracks the state the game asked for, because kira only
//! applies commands on its next audio block.
//!
//! Every source is decoded into memory up front, `"stream"` ones included. kira can't stream on
//! the web, and on native its streaming decoder hangs at the end of an OGG Vorbis file after a
//! seek (kira 0.12.4), which breaks looping music.

use std::{cell::RefCell, io::Cursor, rc::Rc};

use kira::{
    AudioManager, AudioManagerSettings, Decibels, DefaultBackend, Tween,
    sound::{
        PlaybackState, Region,
        static_sound::{StaticSoundData, StaticSoundHandle},
    },
};

/// Changes fade over this long (kira's default tween) to avoid clicks.
fn fade() -> Tween {
    Tween::default()
}

pub type SharedAudio = Rc<RefCell<Audio>>;

pub struct Audio {
    /// Created on first use, so games without sound never open an audio device.
    manager: Manager,
    volume: f32,
    /// Sources with a sound in kira (playing or paused). Like Love2D's source pool, this keeps
    /// them reachable by `stop()` and `pause()` after the game drops them.
    active: Vec<Rc<RefCell<SourceState>>>,
}

enum Manager {
    NotStarted,
    Running(Box<AudioManager>),
    /// No audio device, or the game stopped. Sources still work, but make no sound.
    Unavailable,
}

impl Audio {
    pub fn new() -> SharedAudio {
        Rc::new(RefCell::new(Audio {
            manager: Manager::NotStarted,
            volume: 1.0,
            active: Vec::new(),
        }))
    }

    fn manager(&mut self) -> Option<&mut AudioManager> {
        if let Manager::NotStarted = self.manager {
            self.manager = match AudioManager::<DefaultBackend>::new(AudioManagerSettings::default())
            {
                Ok(mut manager) => {
                    manager
                        .main_track()
                        .set_volume(decibels(self.volume), Tween::default());
                    Manager::Running(Box::new(manager))
                }
                Err(e) => {
                    // miniquad's log macros don't support inline format arguments.
                    macroquad::logging::warn!("audio is unavailable: {}", e);
                    Manager::Unavailable
                }
            };
        }
        match &mut self.manager {
            Manager::Running(manager) => Some(manager),
            _ => None,
        }
    }

    /// Silences everything and releases the audio device, for when the game stops.
    pub fn shutdown(&mut self) {
        self.manager = Manager::Unavailable;
        self.active.clear();
    }

    pub fn volume(&self) -> f32 {
        self.volume
    }

    /// Sets the master volume (clamped to 0-1).
    pub fn set_volume(&mut self, volume: f32) {
        self.volume = volume.clamp(0.0, 1.0);
        if let Manager::Running(manager) = &mut self.manager {
            manager
                .main_track()
                .set_volume(decibels(self.volume), fade());
        }
    }

    /// The sources that are playing or paused.
    fn active(&mut self) -> Vec<Rc<RefCell<SourceState>>> {
        self.active
            .retain(|state| state.borrow_mut().refresh() != Status::Stopped);
        self.active.clone()
    }

    pub fn playing_count(&mut self) -> usize {
        self.active()
            .iter()
            .filter(|state| state.borrow().status == Status::Playing)
            .count()
    }

    pub fn stop_all(&mut self) {
        for state in self.active() {
            state.borrow_mut().stop();
        }
        self.active.clear();
    }

    /// Pauses every playing source and returns them.
    pub fn pause_all(audio: &SharedAudio) -> Vec<Source> {
        let active = audio.borrow_mut().active();
        active
            .into_iter()
            .filter(|state| state.borrow().status == Status::Playing)
            .map(|state| {
                state.borrow_mut().pause();
                Source {
                    state,
                    audio: audio.clone(),
                }
            })
            .collect()
    }
}

/// Love2D's source types. Both are decoded up front (see the module docs).
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum SourceType {
    Static,
    Stream,
}

/// What the game last asked a source to do.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Status {
    Stopped,
    Playing,
    Paused,
}

struct SourceState {
    /// Decoded audio, shared by clones.
    sound: Rc<StaticSoundData>,
    source_type: SourceType,
    handle: Option<StaticSoundHandle>,
    status: Status,
    volume: f32,
    pitch: f64,
    looping: bool,
    /// Where the next play starts while stopped, in seconds.
    offset: f64,
}

impl SourceState {
    /// Notices a sound that finished on its own.
    fn refresh(&mut self) -> Status {
        let finished = self
            .handle
            .as_ref()
            .is_some_and(|handle| handle.state() == PlaybackState::Stopped);
        if finished {
            self.reset();
        }
        self.status
    }

    fn reset(&mut self) {
        self.handle = None;
        self.status = Status::Stopped;
        self.offset = 0.0;
    }

    fn stop(&mut self) {
        if let Some(handle) = &mut self.handle {
            handle.stop(fade());
        }
        self.reset();
    }

    fn pause(&mut self) {
        if self.refresh() == Status::Playing {
            if let Some(handle) = &mut self.handle {
                handle.pause(fade());
            }
            self.status = Status::Paused;
        }
    }

    fn loop_region(&self) -> Option<Region> {
        self.looping.then(|| Region::from(..))
    }

    /// Starts a new kira sound with the source's settings.
    fn start(&self, manager: &mut AudioManager) -> Option<StaticSoundHandle> {
        let data = self
            .sound
            .start_position(self.offset)
            .loop_region(self.loop_region())
            .volume(decibels(self.volume))
            .playback_rate(self.pitch);
        manager.play(data).ok()
    }
}

/// A Love2D `Source`. Clones refer to the same source (unlike Love2D's `Source:clone`, which is
/// [`Source::duplicate`]).
#[derive(Clone)]
pub struct Source {
    state: Rc<RefCell<SourceState>>,
    audio: SharedAudio,
}

impl Source {
    /// Decodes an audio file.
    pub fn new(
        audio: &SharedAudio,
        bytes: Vec<u8>,
        source_type: SourceType,
    ) -> Result<Self, String> {
        let sound = StaticSoundData::from_cursor(Cursor::new(bytes)).map_err(|e| e.to_string())?;
        Ok(Source::with_sound(audio, Rc::new(sound), source_type))
    }

    fn with_sound(
        audio: &SharedAudio,
        sound: Rc<StaticSoundData>,
        source_type: SourceType,
    ) -> Self {
        Source {
            state: Rc::new(RefCell::new(SourceState {
                sound,
                source_type,
                handle: None,
                status: Status::Stopped,
                volume: 1.0,
                pitch: 1.0,
                looping: false,
                offset: 0.0,
            })),
            audio: audio.clone(),
        }
    }

    /// A new, stopped source with the same sound and settings (Love2D's `Source:clone`).
    pub fn duplicate(&self) -> Self {
        let state = self.state.borrow();
        let copy = Source::with_sound(&self.audio, state.sound.clone(), state.source_type);
        {
            let mut copy_state = copy.state.borrow_mut();
            copy_state.volume = state.volume;
            copy_state.pitch = state.pitch;
            copy_state.looping = state.looping;
        }
        copy
    }

    /// Whether two values refer to the same source.
    pub fn same(&self, other: &Source) -> bool {
        Rc::ptr_eq(&self.state, &other.state)
    }

    /// Plays from the current position, or resumes if paused. Returns `false` if the sound
    /// couldn't start (no audio device, or too many sounds playing).
    pub fn play(&self) -> bool {
        let mut state = self.state.borrow_mut();
        match state.refresh() {
            Status::Playing => true,
            Status::Paused => {
                if let Some(handle) = &mut state.handle {
                    handle.resume(fade());
                }
                state.status = Status::Playing;
                true
            }
            Status::Stopped => {
                let mut audio = self.audio.borrow_mut();
                let Some(handle) = audio.manager().and_then(|manager| state.start(manager)) else {
                    return false;
                };
                state.handle = Some(handle);
                state.status = Status::Playing;
                if !audio.active.iter().any(|s| Rc::ptr_eq(s, &self.state)) {
                    audio.active.push(self.state.clone());
                }
                true
            }
        }
    }

    pub fn pause(&self) {
        self.state.borrow_mut().pause();
    }

    /// Stops and rewinds.
    pub fn stop(&self) {
        self.state.borrow_mut().stop();
    }

    pub fn is_playing(&self) -> bool {
        self.state.borrow_mut().refresh() == Status::Playing
    }

    pub fn source_type(&self) -> SourceType {
        self.state.borrow().source_type
    }

    pub fn volume(&self) -> f32 {
        self.state.borrow().volume
    }

    /// Sets the volume (clamped to 0-1).
    pub fn set_volume(&self, volume: f32) {
        let mut state = self.state.borrow_mut();
        state.volume = volume.clamp(0.0, 1.0);
        let volume = decibels(state.volume);
        if let Some(handle) = &mut state.handle {
            handle.set_volume(volume, fade());
        }
    }

    pub fn pitch(&self) -> f64 {
        self.state.borrow().pitch
    }

    /// Sets the playback speed, which also shifts the pitch. Must be positive.
    pub fn set_pitch(&self, pitch: f64) {
        let mut state = self.state.borrow_mut();
        state.pitch = pitch;
        if let Some(handle) = &mut state.handle {
            handle.set_playback_rate(pitch, fade());
        }
    }

    pub fn looping(&self) -> bool {
        self.state.borrow().looping
    }

    pub fn set_looping(&self, looping: bool) {
        let mut state = self.state.borrow_mut();
        state.looping = looping;
        let region = state.loop_region();
        if let Some(handle) = &mut state.handle {
            handle.set_loop_region(region);
        }
    }

    pub fn duration(&self) -> f64 {
        self.state.borrow().sound.duration().as_secs_f64()
    }

    pub fn sample_rate(&self) -> f64 {
        f64::from(self.state.borrow().sound.sample_rate)
    }

    /// The playback position in seconds.
    pub fn tell(&self) -> f64 {
        let mut state = self.state.borrow_mut();
        state.refresh();
        match &state.handle {
            Some(handle) => handle.position(),
            None => state.offset,
        }
    }

    /// Moves the playback position, in seconds (clamped to the sound).
    pub fn seek(&self, seconds: f64) {
        let mut state = self.state.borrow_mut();
        let seconds = seconds.clamp(0.0, state.sound.duration().as_secs_f64());
        state.refresh();
        match &mut state.handle {
            Some(handle) => handle.seek_to(seconds),
            None => state.offset = seconds,
        }
    }
}

/// Converts Love2D's linear volume to kira's decibels.
fn decibels(volume: f32) -> Decibels {
    if volume <= 0.0 {
        Decibels::SILENCE
    } else {
        Decibels((20.0 * volume.log10()).max(Decibels::SILENCE.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_volume_to_decibels() {
        assert_eq!(decibels(1.0), Decibels::IDENTITY);
        assert_eq!(decibels(0.0), Decibels::SILENCE);
        assert!((decibels(0.5).0 - -6.0206).abs() < 1e-3);
        assert!((decibels(0.5).as_amplitude() - 0.5).abs() < 1e-6);
        assert_eq!(decibels(1e-9), Decibels::SILENCE);
    }
}
