use std::{collections::HashMap, io::Cursor, net::TcpStream, sync::mpsc, thread, time::Duration};
use ab_glyph::{point, Font, FontRef, PxScale, ScaleFont};
use base64::Engine;
use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};
use serde_json::{json, Value};
use tungstenite::{connect, stream::MaybeTlsStream, Message};

pub const SIZE: u32 = 144;
const FONT: &[u8] = include_bytes!("../assets/DejaVuSans.ttf");

#[derive(Debug, PartialEq, Eq)]
pub struct Args { pub port: u16, pub uuid: String, pub register_event: String }
impl Args {
    pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Self, String> {
        let mut it = args.into_iter(); let mut map = HashMap::new();
        while let Some(key) = it.next() {
            if !key.starts_with('-') { return Err(format!("Unexpected argument: {key}")); }
            map.insert(key, it.next().ok_or("Missing argument value")?);
        }
        let port = map.get("-port").ok_or("Missing -port")?.parse::<u16>().map_err(|_| "Invalid port")?;
        if port == 0 { return Err("Invalid port".into()); }
        Ok(Self { port, uuid: map.remove("-pluginUUID").ok_or("Missing -pluginUUID")?,
            register_event: map.remove("-registerEvent").ok_or("Missing -registerEvent")? })
    }
}

#[derive(Debug, Clone)]
pub struct Event { pub name: String, pub action: String, pub context: String, pub payload: Value }
impl Event {
    pub fn parse(raw: &str) -> Result<Self, serde_json::Error> {
        let value: Value = serde_json::from_str(raw)?;
        Ok(Self { name: value["event"].as_str().unwrap_or("").into(),
            action: value["action"].as_str().unwrap_or("").into(),
            context: value["context"].as_str().unwrap_or("").into(), payload: value["payload"].clone() })
    }
    pub fn settings(&self) -> &Value { &self.payload["settings"] }
}

pub trait Plugin {
    fn event(&mut self, event: Event, sdk: &mut Client) -> Result<(), String>;
    fn tick(&mut self, sdk: &mut Client) -> Result<(), String>;
}

pub struct Client { ws: tungstenite::WebSocket<MaybeTlsStream<TcpStream>> }
impl Client {
    pub fn send(&mut self, value: Value) -> Result<(), String> {
        self.ws.send(Message::Text(value.to_string().into())).map_err(|e| e.to_string())
    }
    pub fn title(&mut self, context: &str, title: &str) -> Result<(), String> {
        self.send(json!({"event":"setTitle", "context":context, "payload":{"title": title, "target":0}}))
    }
    pub fn image(&mut self, context: &str, png: &[u8]) -> Result<(), String> {
        let data = base64::engine::general_purpose::STANDARD.encode(png);
        self.send(json!({"event":"setImage", "context":context, "payload":{"image":format!("data:image/png;base64,{data}"),"target":0}}))
    }
    pub fn settings(&mut self, context: &str, settings: Value) -> Result<(), String> {
        self.send(json!({"event":"setSettings", "context":context, "payload":settings}))
    }
    pub fn ok(&mut self, context: &str) -> Result<(), String> { self.send(json!({"event":"showOk", "context":context})) }
    pub fn alert(&mut self, context: &str) -> Result<(), String> { self.send(json!({"event":"showAlert", "context":context})) }
    pub fn log(&mut self, message: &str) -> Result<(), String> {
        self.send(json!({"event":"logMessage", "payload":{"message":message}}))
    }
}

pub fn run(mut plugin: impl Plugin) -> Result<(), String> {
    let args = Args::parse(std::env::args().skip(1))?;
    let (ws, _) = connect(format!("ws://127.0.0.1:{}", args.port)).map_err(|e| e.to_string())?;
    let mut sdk = Client { ws };
    sdk.send(json!({"event":args.register_event, "uuid":args.uuid}))?;
    if let MaybeTlsStream::Plain(stream) = sdk.ws.get_mut() {
        stream.set_nonblocking(true).map_err(|e| e.to_string())?;
    }
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || loop { thread::sleep(Duration::from_millis(200)); if tx.send(()).is_err() { break; } });
    loop {
        match sdk.ws.read() {
            Ok(Message::Text(raw)) => { match Event::parse(&raw) {
                Ok(event) => plugin.event(event, &mut sdk)?,
                Err(err) => { let _ = sdk.log(&format!("Invalid event: {err}")); }
            } },
            Ok(Message::Ping(data)) => sdk.ws.send(Message::Pong(data)).map_err(|e| e.to_string())?,
            Ok(Message::Close(_)) | Err(tungstenite::Error::ConnectionClosed) => return Ok(()),
            Err(tungstenite::Error::Io(ref err)) if err.kind() == std::io::ErrorKind::WouldBlock => {},
            Err(err) => return Err(err.to_string()),
            _ => {}
        }
        while rx.try_recv().is_ok() { plugin.tick(&mut sdk)?; }
        thread::sleep(Duration::from_millis(20));
    }
}

pub fn render(lines: &[&str], progress: Option<f32>) -> Result<Vec<u8>, String> {
    let font = FontRef::try_from_slice(FONT).map_err(|e| e.to_string())?;
    let mut img = RgbaImage::from_pixel(SIZE, SIZE, Rgba([18, 24, 36, 255]));
    for (i, line) in lines.iter().take(4).enumerate() {
        let scale = PxScale::from(if line.chars().count() > 12 { 15.0 } else { 23.0 });
        let scaled = font.as_scaled(scale);
        let width: f32 = line.chars().map(|c| scaled.h_advance(scaled.glyph_id(c))).sum();
        let mut x = ((SIZE as f32 - width) / 2.0).max(2.0);
        let y = 32.0 + i as f32 * 29.0;
        for ch in line.chars() {
            let id = scaled.glyph_id(ch);
            if let Some(outline) = font.outline_glyph(id.with_scale_and_position(scale, point(x, y))) {
                let bounds = outline.px_bounds();
                outline.draw(|px, py, coverage| {
                    let xx = px as i32 + bounds.min.x as i32;
                    let yy = py as i32 + bounds.min.y as i32;
                    if xx >= 0 && yy >= 0 && xx < SIZE as i32 && yy < SIZE as i32 {
                        let color = (coverage * 255.0) as u8;
                        img.put_pixel(xx as u32, yy as u32, Rgba([color, color, color, 255]));
                    }
                });
            }
            x += scaled.h_advance(id);
        }
    }
    if let Some(value) = progress {
        for x in 12..132 { for y in 131..138 {
            img.put_pixel(x, y, if (x - 12) as f32 / 120.0 <= value.clamp(0.0, 1.0) {
                Rgba([68, 196, 165, 255]) } else { Rgba([54, 62, 75, 255]) });
        } }
    }
    let mut out = Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(img).write_to(&mut out, ImageFormat::Png).map_err(|e| e.to_string())?;
    Ok(out.into_inner())
}

#[cfg(test)] mod tests {
    use super::*;
    #[test] fn parse_args() { let args = Args::parse(["-port", "1234", "-pluginUUID", "a", "-registerEvent", "registerPlugin", "-info", "{}"].map(str::to_owned)).unwrap(); assert_eq!(args.port, 1234); }
    #[test] fn parse_event() { let e = Event::parse(r#"{"event":"keyDown","context":"abc","payload":{"settings":{"x":1}}}"#).unwrap(); assert_eq!(e.name,"keyDown"); assert_eq!(e.settings()["x"], 1); }
    #[test] fn png_dimensions() { let png = render(&["Hello"],Some(0.5)).unwrap(); assert!(png.len() > 100); let decoded = image::load_from_memory(&png).unwrap(); assert_eq!((decoded.width(),decoded.height()),(144,144)); }
}
