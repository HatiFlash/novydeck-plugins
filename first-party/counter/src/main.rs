use std::{collections::HashMap, time::{Duration, Instant}};
use novydeck_sdk::{Client, Event, Plugin};
use serde_json::{json, Value};

const COUNTER: &str = "com.novydeck.counter.counter";
const NOTE: &str = "com.novydeck.counter.note";
const CLIPBOARD: &str = "com.novydeck.counter.clipboard";
const LONG_PRESS: Duration = Duration::from_millis(600);
fn int(settings: &Value, key: &str, fallback: i64) -> i64 { settings[key].as_i64().unwrap_or(fallback) }
fn short(text: &str) -> String { text.chars().take(24).collect() }
fn next_value(settings: &Value, long: bool) -> i64 {
    if long { int(settings, "reset", 0) }
    else { int(settings, "value", 0).saturating_add(int(settings, "step", 1)) }
}
struct State { action: String, settings: Value, pressed: Option<Instant> }
impl State {
    fn text(&self) -> Vec<String> {
        if self.action == COUNTER { vec!["Count".into(), int(&self.settings, "value", 0).to_string()] }
        else if self.action == NOTE { vec!["Note".into(), short(self.settings["text"].as_str().unwrap_or("(empty)"))] }
        else { vec!["Clipboard".into(), short(self.settings["saved"].as_str().unwrap_or("(empty)"))] }
    }
}
#[derive(Default)] struct Counter { states: HashMap<String,State> }
impl Counter {
    fn draw(&self, ctx: &str, sdk: &mut Client) -> Result<(),String> {
        if let Some(state) = self.states.get(ctx) {
            let text = state.text();
            sdk.image(ctx, &novydeck_sdk::render(&text.iter().map(String::as_str).collect::<Vec<_>>(),None)?)?;
        }
        Ok(())
    }
    fn activate(&mut self, ctx: &str, long: bool, sdk: &mut Client) -> Result<(),String> {
        let Some(state) = self.states.get_mut(ctx) else { return Ok(()); };
        if state.action == COUNTER {
            let value = if long { int(&state.settings, "reset", 0) }
                else { int(&state.settings, "value", 0).saturating_add(int(&state.settings, "step", 1)) };
            state.settings["value"] = json!(value);
            sdk.settings(ctx, state.settings.clone())?;
        } else if state.action == NOTE {
            if !long { match arboard::Clipboard::new().and_then(|mut clip| clip.set_text(state.settings["text"].as_str().unwrap_or("").to_owned())) {
                Ok(()) => sdk.ok(ctx)?, Err(e) => { sdk.log(&format!("Clipboard: {e}"))?; sdk.alert(ctx)?; }
            } }
        } else if state.action == CLIPBOARD {
            let result = arboard::Clipboard::new().and_then(|mut clip| {
                if long { clip.get_text().map(Some) }
                else { clip.set_text(state.settings["saved"].as_str().unwrap_or("").to_owned()).map(|_| None) }
            });
            match result {
                Ok(Some(saved)) => { state.settings["saved"] = json!(saved); sdk.settings(ctx,state.settings.clone())?; sdk.ok(ctx)?; }
                Ok(None) => sdk.ok(ctx)?,
                Err(e) => { sdk.log(&format!("Clipboard: {e}"))?; sdk.alert(ctx)?; }
            }
        }
        self.draw(ctx,sdk)
    }
}
impl Plugin for Counter {
    fn event(&mut self, event: Event, sdk: &mut Client) -> Result<(),String> {
        let ctx = event.context.clone();
        match event.name.as_str() {
            "willAppear" => { self.states.insert(ctx.clone(), State {action:event.action.clone(),settings:event.settings().clone(),pressed:None}); self.draw(&ctx,sdk)?; }
            "willDisappear" => { self.states.remove(&ctx); }
            "didReceiveSettings" => { if let Some(state) = self.states.get_mut(&ctx) { state.settings = event.settings().clone(); } self.draw(&ctx,sdk)?; }
            "keyDown" => { if let Some(state) = self.states.get_mut(&ctx) { state.pressed = Some(Instant::now()); } }
            "keyUp" => { if let Some(state) = self.states.get_mut(&ctx) {
                let held = state.pressed.take().map(|t|t.elapsed()).unwrap_or_default();
                self.activate(&ctx, held >= LONG_PRESS,sdk)?;
            } }
            _ => {}
        }
        Ok(())
    }
    fn tick(&mut self, _sdk: &mut Client) -> Result<(),String> { Ok(()) }
}
fn main() { if let Err(e) = novydeck_sdk::run(Counter::default()) { eprintln!("counter: {e}"); std::process::exit(1); } }
#[cfg(test)] mod tests {
    use super::*;
    #[test] fn settings_defaults() { assert_eq!(int(&Value::Null,"step",1),1); assert_eq!(int(&json!({"step":3}),"step",1),3); }
    #[test] fn long_press_boundary() { assert!(Duration::from_millis(600) >= LONG_PRESS); assert!(Duration::from_millis(599) < LONG_PRESS); }
    #[test] fn safe_short_utf8() { assert_eq!(short("é"),"é"); }
}
