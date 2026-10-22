use std::{collections::HashMap, time::{Duration, Instant}};
use chrono::Local;
use novydeck_sdk::{Client, Event, Plugin};
use serde_json::Value;

const CLOCK: &str = "com.novydeck.clock.clock";
const TIMER: &str = "com.novydeck.clock.timer";
const STOPWATCH: &str = "com.novydeck.clock.stopwatch";
const LONG_PRESS: Duration = Duration::from_millis(600);

fn duration_text(secs: u64) -> String { format!("{:02}:{:02}", secs / 60, secs % 60) }
fn duration_from_settings(settings: &Value) -> u64 {
    settings["seconds"].as_u64().unwrap_or(300).clamp(1, 86400)
}
struct State { action: String, settings: Value, pressed: Option<Instant>, running_since: Option<Instant>, elapsed: Duration, alarm_until: Option<Instant> }
impl State {
    fn new(action: String, settings: Value) -> Self { Self { action, settings, pressed: None, running_since: None, elapsed: Duration::ZERO, alarm_until: None } }
    fn elapsed(&self) -> Duration { self.elapsed + self.running_since.map(|start| start.elapsed()).unwrap_or_default() }
    fn reset(&mut self) { self.elapsed = Duration::ZERO; self.running_since = None; self.alarm_until = None; }
    fn press(&mut self, held: Duration) {
        if held >= LONG_PRESS { self.reset(); return; }
        if let Some(since) = self.running_since.take() { self.elapsed += since.elapsed(); }
        else if self.action != CLOCK { self.running_since = Some(Instant::now()); }
    }
    fn display(&self) -> (Vec<String>, Option<f32>) {
        if self.action == CLOCK {
            let now = Local::now();
            let time = if self.settings["format12"].as_bool().unwrap_or(false) { now.format("%I:%M %p").to_string() } else { now.format("%H:%M:%S").to_string() };
            let mut lines = vec![time];
            if self.settings["showDate"].as_bool().unwrap_or(false) { lines.push(now.format("%d/%m/%Y").to_string()); }
            (lines, None)
        } else if self.action == TIMER {
            let length = duration_from_settings(&self.settings);
            let left = length.saturating_sub(self.elapsed().as_secs());
            (vec!["Timer".into(),duration_text(left)], Some(left as f32 / length as f32))
        } else { (vec!["Stopwatch".into(),duration_text(self.elapsed().as_secs())],None) }
    }
}
#[derive(Default)] struct Clock { states: HashMap<String, State>, last_refresh: Option<Instant> }
impl Clock {
    fn draw(&self, context: &str, sdk: &mut Client) -> Result<(), String> {
        if let Some(state) = self.states.get(context) {
            let (lines, gauge) = state.display();
            if state.alarm_until.is_some_and(|until| until > Instant::now()) && Local::now().timestamp_millis() / 400 % 2 == 0 {
                sdk.image(context, &novydeck_sdk::render(&["TIME!"], None)?)?;
            } else {
                let refs: Vec<_> = lines.iter().map(String::as_str).collect();
                sdk.image(context, &novydeck_sdk::render(&refs, gauge)?)?;
            }
        }
        Ok(())
    }
}
impl Plugin for Clock {
    fn event(&mut self, event: Event, sdk: &mut Client) -> Result<(), String> {
        let ctx = event.context.clone();
        match event.name.as_str() {
            "willAppear" => { self.states.insert(ctx.clone(), State::new(event.action.clone(), event.settings().clone())); self.draw(&ctx,sdk)?; }
            "willDisappear" => { self.states.remove(&ctx); }
            "didReceiveSettings" => { if let Some(state) = self.states.get_mut(&ctx) { state.settings = event.settings().clone(); state.reset(); } self.draw(&ctx,sdk)?; }
            "keyDown" => { if let Some(state) = self.states.get_mut(&ctx) { state.pressed = Some(Instant::now()); } }
            "keyUp" => { if let Some(state) = self.states.get_mut(&ctx) { let held = state.pressed.take().map(|t| t.elapsed()).unwrap_or(Duration::ZERO); state.press(held); } self.draw(&ctx,sdk)?; }
            _ => {}
        }
        Ok(())
    }
    fn tick(&mut self, sdk: &mut Client) -> Result<(), String> {
        if self.last_refresh.is_some_and(|t| t.elapsed() < Duration::from_millis(400)) { return Ok(()); }
        self.last_refresh = Some(Instant::now());
        let mut alarms = Vec::new();
        for (ctx, state) in &mut self.states {
            if state.action == TIMER && state.running_since.is_some() && state.elapsed().as_secs() >= duration_from_settings(&state.settings) {
                state.reset(); state.alarm_until = Some(Instant::now() + Duration::from_secs(5)); alarms.push(ctx.clone());
            }
            if state.alarm_until.is_some_and(|t| t <= Instant::now()) { state.alarm_until = None; }
        }
        for ctx in alarms { sdk.alert(&ctx)?; }
        for ctx in self.states.keys() { self.draw(ctx,sdk)?; }
        Ok(())
    }
}
fn main() { if let Err(e) = novydeck_sdk::run(Clock::default()) { eprintln!("clock: {e}"); std::process::exit(1); } }
#[cfg(test)] mod tests {
    use super::*;
    #[test] fn timer_format() { assert_eq!(duration_text(125), "02:05"); assert_eq!(duration_from_settings(&serde_json::json!({"seconds":0})),1); }
    #[test] fn long_press_resets() { let mut s = State::new(TIMER.into(), Value::Null); s.press(Duration::ZERO); assert!(s.running_since.is_some()); s.press(Duration::from_millis(600)); assert!(s.running_since.is_none()); assert_eq!(s.elapsed(),Duration::ZERO); }
}
