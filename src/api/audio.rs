//! `pg.audio`.

use luars::{
    Lua, LuaApi, LuaResult, LuaState, LuaTable, LuaUserData, LuaUserdata, LuaValue, lua_methods,
};

use super::{Args, Module, SharedHost, number};
use crate::audio::{self as engine, Audio, SourceType};

const SOURCE_TYPES: &[(&str, SourceType)] = &[
    ("static", SourceType::Static),
    ("stream", SourceType::Stream),
];

/// Love2D's `TimeUnit`.
#[derive(Clone, Copy)]
enum TimeUnit {
    Seconds,
    Samples,
}

fn time_unit(unit: Option<String>) -> Result<TimeUnit, String> {
    match unit.as_deref() {
        None | Some("seconds") => Ok(TimeUnit::Seconds),
        Some("samples") => Ok(TimeUnit::Samples),
        Some(other) => Err(format!(
            "invalid time unit '{other}', expected one of 'seconds', 'samples'"
        )),
    }
}

#[derive(LuaUserData)]
#[lua_impl(PartialEq)]
pub struct Source {
    source: engine::Source,
}

/// `pg.audio.pause()` returns new userdata for the sources it paused, so equality compares the
/// underlying source rather than the userdata.
impl PartialEq for Source {
    fn eq(&self, other: &Self) -> bool {
        self.source.same(&other.source)
    }
}

impl Source {
    fn in_unit(&self, seconds: f64, unit: TimeUnit) -> LuaValue {
        match unit {
            TimeUnit::Seconds => number(seconds),
            TimeUnit::Samples => number((seconds * self.source.sample_rate()).round()),
        }
    }

    fn seconds_in(&self, value: f64, unit: TimeUnit) -> f64 {
        match unit {
            TimeUnit::Seconds => value,
            TimeUnit::Samples => value / self.source.sample_rate().max(1.0),
        }
    }
}

#[lua_methods]
impl Source {
    pub fn play(&self) -> bool {
        self.source.play()
    }

    pub fn pause(&self) {
        self.source.pause();
    }

    pub fn stop(&self) {
        self.source.stop();
    }

    #[lua(name = "isPlaying")]
    pub fn is_playing(&self) -> bool {
        self.source.is_playing()
    }

    #[lua(name = "setVolume")]
    pub fn set_volume(&self, volume: f64) {
        self.source.set_volume(volume as f32);
    }

    #[lua(name = "getVolume")]
    pub fn get_volume(&self) -> LuaValue {
        number(self.source.volume())
    }

    #[lua(name = "setPitch")]
    pub fn set_pitch(&self, pitch: f64) -> Result<(), String> {
        if !(pitch.is_finite() && pitch > 0.0) {
            return Err("pitch must be a positive, finite number".to_string());
        }
        self.source.set_pitch(pitch);
        Ok(())
    }

    #[lua(name = "getPitch")]
    pub fn get_pitch(&self) -> LuaValue {
        number(self.source.pitch())
    }

    #[lua(name = "setLooping")]
    pub fn set_looping(&self, looping: bool) {
        self.source.set_looping(looping);
    }

    #[lua(name = "isLooping")]
    pub fn is_looping(&self) -> bool {
        self.source.looping()
    }

    pub fn seek(&self, position: f64, unit: Option<String>) -> Result<(), String> {
        let seconds = self.seconds_in(position, time_unit(unit)?);
        self.source.seek(seconds);
        Ok(())
    }

    pub fn tell(&self, unit: Option<String>) -> Result<LuaValue, String> {
        Ok(self.in_unit(self.source.tell(), time_unit(unit)?))
    }

    #[lua(name = "getDuration")]
    pub fn get_duration(&self, unit: Option<String>) -> Result<LuaValue, String> {
        Ok(self.in_unit(self.source.duration(), time_unit(unit)?))
    }

    #[lua(name = "getType")]
    pub fn get_type(&self) -> String {
        match self.source.source_type() {
            SourceType::Static => "static",
            SourceType::Stream => "stream",
        }
        .to_string()
    }

    #[lua(name = "clone")]
    pub fn duplicate(&self) -> Source {
        Source {
            source: self.source.duplicate(),
        }
    }

    #[lua(name = "type")]
    pub fn lua_type(&self) -> String {
        "Source".to_string()
    }
}

fn as_source(value: &LuaValue) -> Option<engine::Source> {
    let userdata = value.as_userdata_mut()?;
    userdata
        .downcast_ref::<Source>()
        .map(|source| source.source.clone())
}

/// The sources given to `play`, `stop` or `pause`: any mix of Sources and lists of Sources.
fn sources(args: &mut Args) -> LuaResult<Vec<engine::Source>> {
    let mut sources = Vec::new();
    for i in 1..=args.len().max(1) {
        if let Some(table) = args.table(i)? {
            for j in 1.. {
                let value: LuaValue = table.raw_geti(j)?;
                if value.is_nil() {
                    break;
                }
                match as_source(&value) {
                    Some(source) => sources.push(source),
                    None => return Err(args.arg_error(i, "table must contain only Sources")),
                }
            }
        } else {
            sources.push(args.userdata(i, "Source", |s: &Source| s.source.clone())?);
        }
    }
    Ok(sources)
}

pub fn install(lua: &mut Lua, pg: &LuaTable, host: &SharedHost) -> LuaResult<()> {
    let audio = host.borrow().audio.clone();
    let mut m = Module::new(lua)?;
    {
        let host = host.clone();
        let audio = audio.clone();
        m.function("newSource", move |args| {
            let path = args.string(1)?;
            let source_type = args.option(2, "source type", SOURCE_TYPES)?;
            let bytes = host.borrow().fs.read(&path);
            let result = bytes
                .map_err(|e| e.to_string())
                .and_then(|bytes| engine::Source::new(&audio, bytes, source_type));
            match result {
                Ok(source) => {
                    args.state.push(Source { source })?;
                    Ok(1)
                }
                Err(e) => Err(args.error(format!("could not load audio '{path}': {e}"))),
            }
        })?;
    }
    m.function("play", |args| {
        let mut started = true;
        for source in sources(args)? {
            started &= source.play();
        }
        args.ret(started)
    })?;
    {
        let audio = audio.clone();
        m.function("stop", move |args| {
            if args.len() == 0 {
                audio.borrow_mut().stop_all();
            } else {
                sources(args)?.iter().for_each(engine::Source::stop);
            }
            Ok(0)
        })?;
    }
    {
        let audio = audio.clone();
        m.function("pause", move |args| {
            if args.len() > 0 {
                sources(args)?.iter().for_each(engine::Source::pause);
                return Ok(0);
            }
            let paused = Audio::pause_all(&audio);
            let table = LuaApi::create_table(args.state)?;
            for (i, source) in paused.into_iter().enumerate() {
                let value =
                    LuaState::create_userdata(args.state, LuaUserdata::new(Source { source }))?;
                table.raw_seti(i as i64 + 1, value)?;
            }
            args.ret(table)
        })?;
    }
    {
        let audio = audio.clone();
        m.function("setVolume", move |args| {
            let volume = args.f32(1)?;
            audio.borrow_mut().set_volume(volume);
            Ok(0)
        })?;
    }
    {
        let audio = audio.clone();
        m.function("getVolume", move |args| {
            let volume = audio.borrow().volume();
            args.ret(number(volume))
        })?;
    }
    m.function("getActiveSourceCount", move |args| {
        let count = audio.borrow_mut().playing_count();
        args.ret(count as i64)
    })?;
    m.finish(pg, "audio")
}
