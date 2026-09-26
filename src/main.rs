use kira::{AudioManager, AudioManagerSettings, DefaultBackend};
use luars::{Lua, LuaApi, SafeOption, Stdlib};
use macroquad::prelude::*;

fn run_lua() -> Result<String, Box<dyn std::error::Error>> {
    let mut lua = Lua::new(SafeOption::default());
    lua.open_stdlib(Stdlib::All)?;
    lua.register_function("add", |a: i64, b: i64| a + b)?;
    let result: String =
        lua.eval(r#"local x = add(40, 2); return string.format("lua says %d", x)"#)?;
    Ok(result)
}

#[macroquad::main("protogine")]
async fn main() {
    let lua_result = run_lua().unwrap_or_else(|e| format!("lua error: {e}"));

    // Keep the manager alive for the whole loop; dropping it closes the audio device.
    let audio = AudioManager::<DefaultBackend>::new(AudioManagerSettings::default());
    let audio_status = match &audio {
        Ok(_) => "kira ok".to_string(),
        Err(e) => format!("kira error: {e}"),
    };

    loop {
        clear_background(DARKGRAY);
        draw_text(&lua_result, 20.0, 40.0, 30.0, WHITE);
        draw_text(&audio_status, 20.0, 80.0, 30.0, WHITE);
        next_frame().await;
    }
}
