//! Adapted from piekstra/google-home-cli (MIT), commit 1370939af0bd3c1b85bd68701832833a8b7a045e:
//! gpsoauth exchange/auth forms, Foyer JSON gateway, GetHomeGraph and UpdateTraits layouts.
//! Restricted to listed Foyer RPCs; no user-supplied URL, method or shell command.
use std::{collections::{HashMap, HashSet}, fs, path::PathBuf, sync::{atomic::{AtomicBool, Ordering}, mpsc::{self, Receiver, Sender}}, thread, time::Duration};
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
fn selected_devices<'a>(homes: &'a [Value], settings: &Value) -> Result<Vec<&'a Value>, String> {
    let target = selection(homes, settings)?;
    if settings["scope"] == "device" { return Ok(vec![target]); }
    let room_id = target["id"].as_str().filter(|id| !id.is_empty()).ok_or("Invalid room identity")?;
    let home = homes.iter().find(|h| h["id"] == settings["home"]).ok_or("Home not found")?;
    let devices: Vec<_> = home["devices"].as_array().into_iter().flatten()
        .filter(|device| device["room_id"].as_str() == Some(room_id)).collect();
    if devices.is_empty() { return Err("No devices in this room".into()); }
    Ok(devices)
}
// Adapted from piekstra/google-home-cli src/traits.rs (MIT), commit 1370939af0bd3c1b85bd68701832833a8b7a045e.
// GetTraits request: [[id], ...] wrapped in one array; response entries: [[id], [[trait, [[field, wrapper]]]]].
fn toggle_commands(devices: &[&Value], raw: &Value) -> Result<Vec<Value>, String> {
    if devices.is_empty() { return Err("No devices to toggle".into()); }
    let mut ids = HashSet::new();
    for d in devices {
        control_payload(d,"on",None)?;
        let id = d["id"].as_str().ok_or("Invalid device identity")?;
        if !ids.insert(id) { return Err("Duplicate device identity".into()); }
    }
    let entries = raw.get(0).and_then(Value::as_array).ok_or("Invalid GetTraits response")?;
    let mut observed = HashMap::new();
    for entry in entries {
        let id = entry.get(0).and_then(|v| v.get(0)).and_then(Value::as_str).ok_or("Invalid GetTraits device")?;
        if !ids.contains(id) { continue; }
        if observed.contains_key(id) { return Err("Duplicate GetTraits device".into()); }
        let traits = entry.get(1).and_then(Value::as_array).ok_or("Missing GetTraits traits")?;
        let mut on = None;
        for trait_entry in traits {
            if trait_entry.get(0).and_then(Value::as_str) != Some("onOff") { continue; }
            if on.is_some() { return Err("Duplicate onOff trait".into()); }
            let fields = trait_entry.get(1).and_then(Value::as_array).ok_or("Invalid onOff trait")?;
            for field in fields {
                if field.get(0).and_then(Value::as_str) != Some("onOff") { continue; }
                if on.is_some() { return Err("Duplicate onOff state".into()); }
                let wrapper = field.get(1).and_then(Value::as_array).ok_or("Unknown onOff state")?;
                on = Some(match wrapper.get(3) { Some(Value::Bool(b)) => *b, Some(Value::Number(n)) if n.as_i64()==Some(0) => false, Some(Value::Number(n)) if n.as_i64()==Some(1) => true, _ => return Err("Unknown onOff state".into()) });
            }
        }
        observed.insert(id, on.ok_or("Missing onOff state")?);
    }
    if observed.len()!=devices.len() { return Err("Missing GetTraits device state".into()); }
    let mode = if observed.values().all(|&on| on) { "off" } else { "on" };
    devices.iter().map(|d| Ok(control_payload(d,mode,None)?[0][0].clone())).collect()
}
fn execute_with_rpc(settings: Value, mut call: impl FnMut(&str, Value) -> Result<Value,String>) -> Result<(),String> {
    let homes = graph(&call("StructuresService/GetHomeGraph",json!([]))?);
    let devices = selected_devices(&homes, &settings)?;
    let mode = settings["mode"].as_str().ok_or("Choose an action")?;
    let brightness = if mode == "brightness" { Some(settings["brightness"].as_u64().filter(|n| *n <= 100).ok_or("Brightness must be 0–100")? as u8) } else { None };
    let commands = if mode == "toggle" {
        // Validate every selected device before fetching states; never write on failed reads.
        for d in &devices { control_payload(d,"on",None)?; }
        let ids: Vec<Value> = devices.iter().map(|d| json!([d["id"]])).collect();
        let raw = call("HomeControlService/GetTraits",json!([ids]))?;
        toggle_commands(&devices,&raw)?
    } else {
        devices.iter().map(|d| Ok(control_payload(d,mode,brightness)?[0][0].clone())).collect::<Result<Vec<_>,String>>()?
    };
    call("HomeControlService/UpdateTraits",json!([commands]))?;
    Ok(())
}
fn execute(settings: Value) -> Result<(), String> {
    let client = http()?; let token = bearer(&client, &load()?)?;
    execute_with_rpc(settings, |method,body| rpc(&client,&token,method,body))
}
fn list() -> Result<Value, String> {
    let client = http()?; let token = bearer(&client,&load()?)?;
    Ok(json!(graph(&rpc(&client,&token,"StructuresService/GetHomeGraph",json!([]))?)))
}
static EXECUTING: AtomicBool = AtomicBool::new(false);
struct BusyGuard<'a>(&'a AtomicBool);
impl<'a> BusyGuard<'a> {
    fn acquire(flag: &'a AtomicBool) -> Result<Self,String> {
        flag.compare_exchange(false,true,Ordering::AcqRel,Ordering::Acquire)
            .map(|_|Self(flag)).map_err(|_|"Google Home command already in progress".into())
    }
}
impl Drop for BusyGuard<'_> { fn drop(&mut self) { self.0.store(false,Ordering::Release); } }
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
            "keyDown" if e.action==ACTION => { let busy=BusyGuard::acquire(&EXECUTING)?; let s=self.settings.get(&ctx).cloned().unwrap_or(Value::Null); let tx=self.tx.as_ref().ok_or("Channel unavailable")?.clone(); sdk.title(&ctx,"…")?; let outcome_ctx=ctx.clone(); thread::spawn(move|| { let result=execute(s); let _=tx.send(Outcome::Execute(outcome_ctx,result)); drop(busy); }); },
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
    #[test] fn selection_is_scoped_by_stable_ids() {
        let homes = vec![json!({"id":"home-a","rooms":[{"id":"r1","name":"Office"},{"id":"r2","name":"Office"},{"id":"empty","name":"Empty"}],"devices":[{"id":"d1","room_id":"r1","room":"Office"},{"id":"d2","room_id":"r2","room":"Office"},{"id":"d3","room_id":null}]}), json!({"id":"home-b","rooms":[{"id":"r1","name":"Office"}],"devices":[{"id":"other","room_id":"r1"}]})];
        let settings = json!({"home":"home-a","scope":"room","target":"r2"});
        let selected = selected_devices(&homes, &settings).unwrap();
        assert_eq!(selected.len(), 1); assert_eq!(selected[0]["id"], "d2");
        for invalid in [json!({"home":"missing","scope":"room","target":"r2"}),json!({"home":"home-a","scope":"all","target":"r2"}),json!({"home":"home-a","scope":"room","target":"Office"}),json!({"home":"home-a","scope":"room","target":"empty"})] { assert!(selected_devices(&homes,&invalid).is_err()); }
        assert_eq!(selected_devices(&homes,&json!({"home":"home-a","scope":"device","target":"d3"})).unwrap()[0]["id"],"d3");
    }
    #[test] fn toggle_accepts_onoff_device() {
        let device = json!({"id":"lamp","agent":"agent","partner":"partner","traits":["action.devices.traits.OnOff"]});
        assert!(toggle_commands(&[&device],&states(&[("lamp",json!([null,null,null,0]))])).is_ok(), "toggle must control an OnOff device");
    }
    fn device(id: &str, room: &str, supported: bool) -> Value { json!({"id":id,"agent":"agent","partner":"partner","room_id":room,"traits":if supported {vec!["action.devices.traits.OnOff"]} else {vec![]}}) }
    fn states(values: &[(&str, Value)]) -> Value { json!([values.iter().map(|(id, value)| json!([[id],[["onOff",[["onOff",value]]]]])).collect::<Vec<_>>()]) }
    #[test] fn toggle_reads_real_boolean_and_inverts() {
        let d=device("lamp","r1",true);
        assert_eq!(toggle_commands(&[&d], &states(&[("lamp",json!([null,null,null,1]))])).unwrap(), vec![control_payload(&d,"off",None).unwrap()[0][0].clone()]);
        assert_eq!(toggle_commands(&[&d], &states(&[("lamp",json!([null,null,null,0]))])).unwrap(), vec![control_payload(&d,"on",None).unwrap()[0][0].clone()]);
    }
    #[test] fn room_toggle_all_on_turns_all_off_otherwise_all_on() {
        let a=device("a","same",true); let b=device("b","same",true);
        let both=[&a,&b];
        for (raw,mode) in [(states(&[("a",json!([null,null,null,1])),("b",json!([null,null,null,1]))]),"off"),(states(&[("a",json!([null,null,null,1])),("b",json!([null,null,null,0]))]),"on")] {
            assert_eq!(toggle_commands(&both,&raw).unwrap(),both.iter().map(|d|control_payload(d,mode,None).unwrap()[0][0].clone()).collect::<Vec<_>>());
        }
    }
    #[test] fn toggle_missing_invalid_or_unsupported_state_refuses_every_write() {
        let a=device("a","r",true);let b=device("b","r",true);let unsupported=device("u","r",false);
        for raw in [states(&[("a",json!([null,null,null,1]))]),states(&[("a",json!([null,null,null,1])),("b",json!([null,null,null,2]))]),json!([[[["a"],[["brightness",[["brightness",[null,50]]]]]],[["b"],[["onOff",[["onOff",[null,null,null,1]]]]]]]])] {
            assert!(toggle_commands(&[&a,&b],&raw).is_err());
        }
        assert!(toggle_commands(&[&a,&unsupported],&states(&[("a",json!([null,null,null,1])),("u",json!([null,null,null,1]))])).is_err());
    }
    #[test] fn toggle_uses_only_ids_in_selected_room_even_with_homonyms() {
        let homes=vec![json!({"id":"h1","rooms":[{"id":"r1","name":"Office"},{"id":"r2","name":"Office"}],"devices":[device("a","r1",true),device("b","r2",true)]}),json!({"id":"h2","rooms":[{"id":"r1","name":"Office"}],"devices":[device("c","r1",true)]})];
        let chosen=selected_devices(&homes,&json!({"home":"h1","scope":"room","target":"r1"})).unwrap();
        assert_eq!(chosen.len(),1);assert_eq!(chosen[0]["id"],"a");
        assert_eq!(toggle_commands(&chosen,&states(&[("a",json!([null,null,null,0]))])).unwrap().len(),1);
    }
    #[test] fn simultaneous_press_cannot_start_two_reads() {
        let busy=AtomicBool::new(false);
        let first=BusyGuard::acquire(&busy).unwrap();
        assert!(BusyGuard::acquire(&busy).is_err());
        drop(first);
        assert!(BusyGuard::acquire(&busy).is_ok());
    }
    #[test] fn failed_gettraits_never_sends_update() {
        let raw=json!([0,[["h","Home",null,null,null,[["r",null,"Room",null,[[["a",["agent","partner"]]]]]],[[["a",["agent","partner"]],null,null,"Lamp",null,null,["action.devices.traits.OnOff"]]]]]]);
        let mut calls=Vec::new();
        let result=execute_with_rpc(json!({"home":"h","scope":"room","target":"r","mode":"toggle"}), |method,body| { calls.push((method.to_string(),body)); match method {"StructuresService/GetHomeGraph"=>Ok(raw.clone()),"HomeControlService/GetTraits"=>Err("Google Home HTTP 503".into()),_=>panic!("unexpected write") } });
        assert!(result.is_err());assert_eq!(calls.iter().map(|(m,_)|m.as_str()).collect::<Vec<_>>(),vec!["StructuresService/GetHomeGraph","HomeControlService/GetTraits"]);
        assert_eq!(calls[1].1,json!([[ ["a"] ]]));
    }
    #[test] fn successful_toggle_reads_before_single_write() {
        let raw=json!([0,[["h","Home",null,null,null,[["r",null,"Room",null,[[["a",["agent","partner"]]]]]],[[["a",["agent","partner"]],null,null,"Lamp",null,null,["action.devices.traits.OnOff"]]]]]]);
        let mut calls=Vec::new();
        execute_with_rpc(json!({"home":"h","scope":"room","target":"r","mode":"toggle"}), |method,body| {
            calls.push((method.to_string(),body));
            match method {"StructuresService/GetHomeGraph"=>Ok(raw.clone()),"HomeControlService/GetTraits"=>Ok(states(&[("a",json!([null,null,null,0]))])),"HomeControlService/UpdateTraits"=>Ok(json!([])),_=>panic!("unexpected RPC")}
        }).unwrap();
        assert_eq!(calls.iter().map(|(m,_)|m.as_str()).collect::<Vec<_>>(),vec!["StructuresService/GetHomeGraph","HomeControlService/GetTraits","HomeControlService/UpdateTraits"]);
        assert_eq!(calls[1].1,json!([[ ["a"] ]]));
        assert_eq!(calls[2].1,json!([[control_payload(&device("a","r",true),"on",None).unwrap()[0][0].clone()]]));
    }
    #[test] fn group_selection_rejects_invalid_lists_without_truncation() {
        let a=device("a","r1",true); let b=device("b","r2",true);
        let homes=vec![json!({"id":"h1","rooms":[{"id":"r1","name":"Office"},{"id":"r2","name":"Office"}],"devices":[a,b]}),json!({"id":"h2","rooms":[{"id":"r1","name":"Office"}],"devices":[device("other","r1",true)]})];
        let choose=|targets:Value| selected_devices(&homes,&json!({"home":"h1","scope":"group","targets":targets})).map(|v|v.iter().map(|d|d["id"].clone()).collect::<Vec<_>>());
        assert_eq!(choose(json!(["b","a"])).unwrap(),vec![json!("b"),json!("a")]);
        for targets in [json!([]),json!(["a","a"]),json!(["other"]),json!(["missing"]),json!(["a",null]),json!("a"),json!(["a";21])] { assert!(choose(targets).is_err()); }
        let many=vec![json!({"id":"h1","devices":(0..21).map(|n|device(&format!("d{n}"),"r1",true)).collect::<Vec<_>>()})];
        let ids:Vec<_>=(0..20).map(|n|format!("d{n}")).collect();
        assert_eq!(selected_devices(&many,&json!({"home":"h1","scope":"group","targets":ids})).unwrap().len(),20);
        let ids:Vec<_>=(0..21).map(|n|format!("d{n}")).collect();
        assert!(selected_devices(&many,&json!({"home":"h1","scope":"group","targets":ids})).is_err());
    }
    #[test] fn group_toggle_reads_all_before_single_write_or_none() {
        let raw=json!([0,[["h","Home",null,null,null,[["r1",null,"Office",null,[[["a",["agent","partner"]]]]], ["r2",null,"Office",null,[[["b",["agent","partner"]]]]]],[[["a",["agent","partner"]],null,null,"Lamp",null,null,["action.devices.traits.OnOff"]],[["b",["agent","partner"]],null,null,"Fan",null,null,["action.devices.traits.OnOff"]]]]]]);
        for (states_reply,expected) in [(states(&[("a",json!([null,null,null,1])),("b",json!([null,null,null,0]))]),true),(states(&[("a",json!([null,null,null,1]))]),false)] {
            let mut calls=Vec::new();
            let result=execute_with_rpc(json!({"home":"h","scope":"group","targets":["b","a"],"mode":"toggle"}),|method,body| {calls.push((method.to_string(),body));match method {"StructuresService/GetHomeGraph"=>Ok(raw.clone()),"HomeControlService/GetTraits"=>Ok(states_reply.clone()),"HomeControlService/UpdateTraits"=>Ok(json!([])),_=>panic!("unexpected RPC")}});
            assert_eq!(result.is_ok(),expected);
            assert_eq!(calls.iter().map(|(m,_)|m.as_str()).collect::<Vec<_>>(),if expected {vec!["StructuresService/GetHomeGraph","HomeControlService/GetTraits","HomeControlService/UpdateTraits"]} else {vec!["StructuresService/GetHomeGraph","HomeControlService/GetTraits"]});
            assert_eq!(calls[1].1,json!([[ ["b"],["a"] ]]));
            if expected {assert_eq!(calls[2].1.as_array().unwrap()[0].as_array().unwrap().len(),2);}
        }
    }
    #[test] fn auth_response_and_invalid_input() { assert_eq!(response("Auth=ya29.abc=x\n", "Auth","ya29").unwrap(),"ya29.abc=x"); assert!(response("Error=BadAuthentication","Auth","ya29").is_err()); assert!(connect("invalid","not-a-token").is_err()); }
    #[test] fn graph_and_commands() { let raw=json!([0,[["home","House",null,null,null,[["room-id",null,"Office",null,[[["dev",["agent","partner"]]]]], ["other-room",null,"Kitchen",null,[[["kitchen-dev",["agent","partner"]]]]], ["nameless-room",null,null,null,[[["nameless-dev",["agent","partner"]]]]]],[[["dev",["agent","partner"]],null,null,"Light",null,null,["action.devices.traits.OnOff","action.devices.traits.Brightness"]],[["kitchen-dev",["agent","partner"]],null,null,"Kettle",null,null,[]],[["unplaced",["agent","partner"]],null,null,"Unplaced",null,null,[]],[["nameless-dev",["agent","partner"]],null,null,"Unknown",null,null,[]]]]]]); let g=graph(&raw); assert_eq!(g[0]["devices"][0]["room_id"],"room-id"); assert_eq!(g[0]["devices"][0]["room"],"Office"); assert_eq!(g[0]["devices"][1]["room_id"],"other-room"); assert_eq!(g[0]["devices"][1]["room"],"Kitchen"); assert!(g[0]["devices"][2]["room_id"].is_null()); assert!(g[0]["devices"][2]["room"].is_null()); assert_eq!(g[0]["devices"][3]["room_id"],"nameless-room"); assert!(g[0]["devices"][3]["room"].is_null());let d=&g[0]["devices"][0]; assert_eq!(control_payload(d,"off",None).unwrap(),json!([[[["dev",["agent","partner"]],[["onOff",[["onOff",[null,null,null,0]]]]]]]]));assert!(control_payload(d,"brightness",None).is_err());assert!(control_payload(d,"shell",None).is_err()); }
}
