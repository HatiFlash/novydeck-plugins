//! Adapted from piekstra/google-home-cli (MIT), commit 1370939af0bd3c1b85bd68701832833a8b7a045e:
//! gpsoauth exchange/auth forms, Foyer JSON gateway, GetHomeGraph and UpdateTraits layouts.
//! Restricted to listed Foyer RPCs; no user-supplied URL, method or shell command.
use std::{collections::HashMap, fs, path::PathBuf, sync::mpsc::{self, Receiver, Sender}, thread, time::Duration};
use novydeck_sdk::{Client, Event, Plugin};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use windows_sys::Win32::{Foundation::LocalFree, Security::Cryptography::{CryptProtectData, CryptUnprotectData, CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN}};

const ACTION: &str = "com.novydeck.googlehome.control";
const AUTH: &str = "https://android.clients.google.com/auth";
const RPC: &str = "https://googlehomefoyer-pa.clients6.google.com/$rpc/google.internal.home.foyer.v1.";
const SCOPE: &str = "oauth2:https://www.googleapis.com/auth/homegraph";
#[derive(Serialize, Deserialize)]
struct Secret { email: String, master: String, android_id: String }
fn path() -> Result<PathBuf, String> {
    Ok(PathBuf::from(std::env::var_os("APPDATA").ok_or("APPDATA unavailable")?)
        .join("NovyDeck/plugins/com.novydeck.googlehome/secrets.bin"))
}
fn crypt(data: &[u8], decrypt: bool) -> Result<Vec<u8>, String> {
    let input = CRYPT_INTEGER_BLOB { cbData: data.len().try_into().map_err(|_| "Secret too large")?, pbData: data.as_ptr() as *mut u8 };
    let mut output = CRYPT_INTEGER_BLOB { cbData: 0, pbData: std::ptr::null_mut() };
    let ok = unsafe { if decrypt {
        CryptUnprotectData(&input, std::ptr::null_mut(), std::ptr::null(), std::ptr::null(), std::ptr::null(), CRYPTPROTECT_UI_FORBIDDEN, &mut output)
    } else { CryptProtectData(&input, std::ptr::null(), std::ptr::null(), std::ptr::null(), std::ptr::null(), CRYPTPROTECT_UI_FORBIDDEN, &mut output) } };
    if ok == 0 { return Err("Windows encryption unavailable".into()); }
    let bytes = unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec() };
    unsafe { LocalFree(output.pbData as *mut _); }
    Ok(bytes)
}
fn load() -> Result<Secret, String> {
    let data = fs::read(path()?).map_err(|_| "Not connected / Non connecté")?;
    serde_json::from_slice(&crypt(&data, true)?).map_err(|_| "Invalid credential".into())
}
fn save(secret: &Secret) -> Result<(), String> {
    let p = path()?;
    fs::create_dir_all(p.parent().ok_or("Invalid path")?).map_err(|_| "Credential directory unavailable")?;
    let data = crypt(&serde_json::to_vec(secret).map_err(|_| "Invalid credential")?, false)?;
    let tmp = p.with_extension("tmp");
    fs::write(&tmp, data).map_err(|_| "Credential write failed")?;
    fs::rename(tmp, p).map_err(|_| "Credential save failed".into())
}
fn http() -> Result<reqwest::blocking::Client, String> {
    reqwest::blocking::Client::builder().timeout(Duration::from_secs(12)).build().map_err(|_| "HTTPS unavailable".into())
}
fn response(body: &str, key: &str, prefix: &str) -> Result<String, String> {
    let fields: HashMap<_, _> = body.lines().filter_map(|line| line.split_once('=')).collect();
    fields.get(key).filter(|s| s.starts_with(prefix)).map(|s| (*s).to_string())
        .ok_or_else(|| format!("Google authentication rejected ({})", fields.get("Error").copied().unwrap_or("unexpected response")))
}
fn form(client: &reqwest::blocking::Client, fields: &[(&str, &str)]) -> Result<String, String> {
    client.post(AUTH).header("User-Agent", "GoogleAuth/1.4").form(fields).send()
        .and_then(|r| r.text()).map_err(|_| "Google authentication unavailable".into())
}
fn connect(email: &str, oauth: &str) -> Result<(), String> {
    if email.len() > 254 || !email.contains('@') || email.contains(['\n','\r']) || !oauth.starts_with("oauth2_4/") || oauth.len() > 8192 { return Err("Invalid email or oauth_token".into()); }
    let mut id = [0u8; 8]; rand::thread_rng().fill_bytes(&mut id);
    let android_id: String = id.iter().map(|b| format!("{b:02x}")).collect();
    let client = http()?;
    let master = response(&form(&client, &[("accountType","HOSTED_OR_GOOGLE"),("Email",email),("has_permission","1"),("add_account","1"),("ACCESS_TOKEN","1"),("Token",oauth),("service","ac2dm"),("source","android"),("androidId",&android_id),("device_country","us"),("operatorCountry","us"),("lang","en"),("sdk_version","17"),("google_play_services_version","240913000"),("client_sig","38918a453d07199354f8b19af05ec6562ced5788"),("callerSig","38918a453d07199354f8b19af05ec6562ced5788"),("droidguard_results","dummy123")])?, "Token", "aas_et/")?;
    // Verify the exchange before replacing an existing encrypted credential.
    let secret = Secret { email: email.into(), master, android_id };
    let _ = bearer(&client, &secret)?;
    save(&secret)
}
fn bearer(client: &reqwest::blocking::Client, s: &Secret) -> Result<String, String> {
    response(&form(client, &[("accountType","HOSTED_OR_GOOGLE"),("Email",&s.email),("has_permission","1"),("EncryptedPasswd",&s.master),("service",SCOPE),("source","android"),("androidId",&s.android_id),("app","com.google.android.apps.chromecast.app"),("client_sig","24bb24c05e47e0aefa68a58a766179d9b613a600"),("device_country","us"),("operatorCountry","us"),("lang","en"),("sdk_version","17"),("google_play_services_version","240913000")])?, "Auth", "ya29")
}
fn rpc(client: &reqwest::blocking::Client, token: &str, method: &str, body: Value) -> Result<Value, String> {
    let url = format!("{RPC}{method}"); // method is always an internal constant
    let reply = client.post(url).bearer_auth(token).header("Content-Type", "application/json+protobuf")
        .header("X-User-Agent", "grpc-web-javascript/0.1").body(body.to_string()).send().map_err(|_| "Google Home unavailable".to_string())?;
    if !reply.status().is_success() { return Err(format!("Google Home HTTP {}", reply.status().as_u16())); }
    let text = reply.text().map_err(|_| "Invalid Google Home reply")?;
    serde_json::from_str(text.trim_start().strip_prefix(")]}'").unwrap_or(&text).trim_start()).map_err(|_| "Invalid Google Home JSON".into())
}
fn graph(raw: &Value) -> Vec<Value> {
    let slot = &raw[1];
    let homes: Vec<&Value> = if slot[0].is_string() { vec![slot] } else { slot.as_array().map(|v| v.iter().collect()).unwrap_or_default() };
    homes.into_iter().filter_map(|home| {
        let id = home[0].as_str()?;
        let rooms: Vec<Value> = home[5].as_array().into_iter().flatten().filter_map(|r| Some(json!({"id":r[0].as_str()?,"name":r[2].as_str()?}))).collect();
        let devices: Vec<Value> = home[6].as_array().into_iter().flatten().filter_map(|d| {
            let key = &d[0];
            let room_id = home[5].as_array().into_iter().flatten().find(|r| r[4].as_array().into_iter().flatten().any(|member| member[0][0] == key[0])).and_then(|r| r[0].as_str());
            let room_name = rooms.iter().find(|r| Some(r["id"].as_str().unwrap_or_default()) == room_id).and_then(|r| r["name"].as_str());
            Some(json!({"id":key[0].as_str()?,"name":d[3].as_str()?,"agent":key[1][0].as_str()?,"partner":key[1][1].as_str()?,"traits":d[6],"room_id":room_id,"room":room_name}))
        }).collect();
        Some(json!({"id":id,"name":home[1],"rooms":rooms,"devices":devices}))
    }).collect()
}
fn control_payload(device: &Value, mode: &str, brightness: Option<u8>) -> Result<Value, String> {
    let traits = device["traits"].as_array().ok_or("Select a device from the list")?;
    let has = |suffix: &str| traits.iter().any(|v| v.as_str().is_some_and(|s| s.ends_with(suffix)));
    let change = match mode {
        "on" | "off" if has("OnOff") => json!(["onOff",[["onOff",[null,null,null,if mode=="on" {1} else {0}]]]]),
        "brightness" if has("Brightness") => json!(["brightness",[["brightness",[null,brightness.ok_or("Brightness must be 0–100")?]]]]),
        "on" | "off" | "brightness" => return Err("Device does not advertise this capability".into()),
        _ => return Err("Invalid action".into()),
    };
    let key = |k| device[k].as_str().filter(|s| !s.is_empty()).ok_or("Invalid device identity");
    Ok(json!([[[[key("id")?,[key("agent")?,key("partner")?]],[change]]]]))
}
fn selection<'a>(homes: &'a [Value], settings: &Value) -> Result<&'a Value, String> {
    let home = settings["home"].as_str().ok_or("Choose a home")?;
    let target = settings["target"].as_str().ok_or("Choose a device or room")?;
    let home = homes.iter().find(|v| v["id"] == home).ok_or("Home not found; refresh list")?;
    let collection = match settings["scope"].as_str() { Some("device") => "devices", Some("room") => "rooms", _ => return Err("Choose device or room".into()) };
    home[collection].as_array().and_then(|items| items.iter().find(|v| v["id"] == target)).ok_or("Target not found; refresh list".into())
}
fn execute(settings: Value) -> Result<(), String> {
    let client = http()?; let token = bearer(&client, &load()?)?;
    let homes = graph(&rpc(&client,&token,"StructuresService/GetHomeGraph",json!([]))?);
    let target = selection(&homes,&settings)?;
    let mode = settings["mode"].as_str().ok_or("Choose an action")?;
    let brightness = if mode == "brightness" { Some(settings["brightness"].as_u64().filter(|n| *n <= 100).ok_or("Brightness must be 0–100")? as u8) } else { None };
    let home = homes.iter().find(|h| h["id"] == settings["home"]).ok_or("Home not found")?;
    let devices: Vec<&Value> = if settings["scope"] == "room" {
        let room_name = target["name"].as_str().ok_or("Room not found")?;
        home["devices"].as_array().into_iter().flatten().filter(|d|d["room"] == room_name).collect()
    } else { vec![target] };
    if devices.is_empty() { return Err("No devices in this room".into()); }
    // Explicitly reject unsupported devices before any write; no partial room update.
    let mut commands = Vec::new();
    for d in devices { commands.push(control_payload(d,mode,brightness)?[0][0].clone()); }
    rpc(&client,&token,"HomeControlService/UpdateTraits",json!([commands]))?;
    Ok(())
}
fn list() -> Result<Value, String> {
    let client = http()?; let token = bearer(&client,&load()?)?;
    Ok(json!(graph(&rpc(&client,&token,"StructuresService/GetHomeGraph",json!([]))?)))
}
enum Outcome { Connect(String,Result<(),String>), List(String,Result<Value,String>), Execute(String,Result<(),String>) }
#[derive(Default)] struct Home { settings: HashMap<String,Value>, tx: Option<Sender<Outcome>>, rx: Option<Receiver<Outcome>> }
impl Home {
    fn status(&self,ctx:&str,sdk:&mut Client)->Result<(),String> { sdk.inspector(ctx,json!({"type":"status","connected":load().is_ok()})) }
}
impl Plugin for Home {
    fn event(&mut self,e:Event,sdk:&mut Client)->Result<(),String> {
        let ctx=e.context.clone();
        let result = (|| -> Result<(),String> { match e.name.as_str() {
            "willAppear" => { self.settings.insert(ctx.clone(),e.settings().clone()); sdk.title(&ctx,"Home")?; },
            "willDisappear" => { self.settings.remove(&ctx); },
            "didReceiveSettings" => { self.settings.insert(ctx.clone(),e.settings().clone()); },
            "propertyInspectorDidAppear" => self.status(&ctx,sdk)?,
            "keyDown" if e.action==ACTION => { let s=self.settings.get(&ctx).cloned().unwrap_or(Value::Null); let tx=self.tx.as_ref().ok_or("Channel unavailable")?.clone(); sdk.title(&ctx,"…")?; let outcome_ctx=ctx.clone(); thread::spawn(move|| { let _=tx.send(Outcome::Execute(outcome_ctx,execute(s))); }); },
            "sendToPlugin" if e.action.is_empty() || e.action==ACTION => match e.payload["type"].as_str().unwrap_or("") {
                "status" => self.status(&ctx,sdk)?,
                "disconnect" => { match fs::remove_file(path()?) { Ok(())=>(),Err(err) if err.kind()==std::io::ErrorKind::NotFound=>(),Err(_)=>return Err("Credential deletion failed".into()) };self.status(&ctx,sdk)?; },
                "connect" => { let email=e.payload["email"].as_str().ok_or("Email required")?.to_string(); let oauth=e.payload["token"].as_str().ok_or("Token required")?.to_string(); let tx=self.tx.as_ref().ok_or("Channel unavailable")?.clone(); let outcome_ctx=ctx.clone(); thread::spawn(move|| { let _=tx.send(Outcome::Connect(outcome_ctx,connect(&email,&oauth))); }); },
                "list" => { let tx=self.tx.as_ref().ok_or("Channel unavailable")?.clone(); let outcome_ctx=ctx.clone(); thread::spawn(move|| { let _=tx.send(Outcome::List(outcome_ctx,list())); }); },
                _=>(),
            }, _=>(), } Ok(()) })();
        if let Err(message)=result { sdk.inspector(&ctx,json!({"type":"message","message":message}))?;sdk.alert(&ctx)?; }
        Ok(())
    }
    fn tick(&mut self,sdk:&mut Client)->Result<(),String> {
        while let Ok(outcome)=self.rx.as_ref().ok_or("Channel unavailable")?.try_recv() { match outcome {
            Outcome::Connect(ctx,r) => { sdk.inspector(&ctx,json!({"type":"message","message":r.as_ref().map(|_|"Connected / Connecté").unwrap_or_else(|e|e.as_str())}))?; self.status(&ctx,sdk)?; },
            Outcome::List(ctx,r) => match r { Ok(homes)=>sdk.inspector(&ctx,json!({"type":"homes","homes":homes}))?,Err(message)=>sdk.inspector(&ctx,json!({"type":"message","message":message}))? },
            Outcome::Execute(ctx,r) => match r { Ok(())=>{sdk.title(&ctx,"Sent")?;sdk.ok(&ctx)?;},Err(e)=>{sdk.title(&ctx,&e)?;sdk.alert(&ctx)?;} },
        } } Ok(())
    }
}
fn main() { let (tx,rx)=mpsc::channel(); if let Err(e)=novydeck_sdk::run(Home{tx:Some(tx),rx:Some(rx),..Home::default()}) { eprintln!("googlehome: {e}");std::process::exit(1); } }
#[cfg(test)] mod tests { use super::*;
    #[test] fn auth_response_and_invalid_input() { assert_eq!(response("Auth=ya29.abc=x\n", "Auth","ya29").unwrap(),"ya29.abc=x"); assert!(response("Error=BadAuthentication","Auth","ya29").is_err()); assert!(connect("invalid","not-a-token").is_err()); }
    #[test] fn graph_and_commands() { let raw=json!([0,[["home","House",null,null,null,[["room-id",null,"Office",null,[[["dev",["agent","partner"]]]]], ["other-room",null,"Kitchen",null,[[["kitchen-dev",["agent","partner"]]]]], ["nameless-room",null,null,null,[[["nameless-dev",["agent","partner"]]]]]],[[["dev",["agent","partner"]],null,null,"Light",null,null,["action.devices.traits.OnOff","action.devices.traits.Brightness"]],[["kitchen-dev",["agent","partner"]],null,null,"Kettle",null,null,[]],[["unplaced",["agent","partner"]],null,null,"Unplaced",null,null,[]],[["nameless-dev",["agent","partner"]],null,null,"Unknown",null,null,[]]]]]]); let g=graph(&raw); assert_eq!(g[0]["devices"][0]["room_id"],"room-id"); assert_eq!(g[0]["devices"][0]["room"],"Office"); assert_eq!(g[0]["devices"][1]["room_id"],"other-room"); assert_eq!(g[0]["devices"][1]["room"],"Kitchen"); assert!(g[0]["devices"][2]["room_id"].is_null()); assert!(g[0]["devices"][2]["room"].is_null()); assert_eq!(g[0]["devices"][3]["room_id"],"nameless-room"); assert!(g[0]["devices"][3]["room"].is_null());let d=&g[0]["devices"][0]; assert_eq!(control_payload(d,"off",None).unwrap(),json!([[[["dev",["agent","partner"]],[["onOff",[["onOff",[null,null,null,0]]]]]]]));assert!(control_payload(d,"brightness",None).is_err());assert!(control_payload(d,"shell",None).is_err()); }
}
