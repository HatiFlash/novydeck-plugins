use std::{collections::HashMap, path::{Path,PathBuf},process::Command};
use novydeck_sdk::{Client,Event,Plugin,StateSource};
use serde_json::{json,Value};
const PREFIX:&str="com.novydeck.wallpaperengine.";
fn executable(settings:&Value)->Result<PathBuf,String> {
    if let Some(raw)=settings["executable"].as_str().filter(|s|!s.trim().is_empty()) {
        let p=PathBuf::from(raw); if !p.is_file() || !valid_exe(&p) {return Err("Choose wallpaper32.exe or wallpaper64.exe".into());}return Ok(p);
    }
    for root in [std::env::var_os("PROGRAMFILES(X86)"),std::env::var_os("PROGRAMFILES"),std::env::var_os("STEAM_PATH")] {
        if let Some(root)=root { for base in [PathBuf::from(&root).join("Steam/steamapps/common/wallpaper_engine"),PathBuf::from(&root).join("steamapps/common/wallpaper_engine")] {
            for name in ["wallpaper64.exe","wallpaper32.exe"] {let p=base.join(name); if p.is_file(){return Ok(p);} }
        } }
    }
    Err("Wallpaper Engine executable not found; choose its installed EXE".into())
}
fn valid_exe(p:&Path)->bool { p.file_name().and_then(|s|s.to_str()).is_some_and(|s|s.eq_ignore_ascii_case("wallpaper64.exe")||s.eq_ignore_ascii_case("wallpaper32.exe")) }
fn required(value:&Value,key:&str)->Result<String,String> {
    let s=value[key].as_str().unwrap_or("").trim();
    if s.is_empty()||s.len()>2048||s.contains(['\n','\r','\0']) {return Err(format!("Invalid {key}"));}
    Ok(s.into())
}
fn target(s:&Value)->Result<Vec<String>,String> {
    match s["target"].as_str().ok_or("Choose a screen (or internal location)")? {
        "monitor" => {let index=s["monitor"].as_u64().filter(|v|*v<32).ok_or("Screen must be 1–32")?;Ok(vec!["-monitor".into(),index.to_string()])},
        "location" => Ok(vec!["-location".into(),required(s,"location")?]),
        _ => Err("Choose a screen (or internal location)".into())
    }
}
fn arguments(action:&str,s:&Value)->Result<Vec<String>,String> {
    let mut args=vec!["-control".into()];
    match action {
        "wallpaper" => {let p=PathBuf::from(required(s,"file")?);if !p.is_file(){return Err("Wallpaper file not found".into());}args.extend(["openWallpaper".into(),"-file".into(),p.to_string_lossy().into_owned()]);args.extend(target(s)?);},
        "playlist" => {args.extend(["openPlaylist".into(),"-playlist".into(),required(s,"playlist")?]);args.extend(target(s)?);},
        "next" => {args.push("nextWallpaper".into());args.extend(target(s)?);},
        "profile" => args.extend(["openProfile".into(),"-profile".into(),required(s,"profile")?]),
        "pause"|"play"|"stop"|"mute"|"unmute" => args.push(action.into()),
        _=>return Err("Unknown action".into()),
    }
    Ok(args)
}
fn run(action:&str,s:&Value)->Result<(),String> {
    let args=arguments(action,s)?;
    let exe=executable(s)?;
    // Command passes each argument separately: spaces and punctuation cannot inject shell commands.
    let status=Command::new(exe).args(args).status().map_err(|_|"Wallpaper Engine failed to start")?;
    if status.success(){Ok(())}else{Err(format!("Wallpaper Engine exit: {status}"))}
}
// Local command state only: external changes in Wallpaper Engine are not observable here.
#[derive(Clone,Copy,Default,PartialEq,Eq,Debug)] enum Playback {#[default] Unknown,Playing,Paused,Stopped}
#[derive(Default)] struct Wallpaper{states:HashMap<String,Value>,actions:HashMap<String,String>,playback:Playback}
impl Wallpaper {
    fn appearance_state(&self,ctx:&str)->Value {
        let state=match self.actions.get(ctx).map(String::as_str) {
            Some("stopplay") => match self.playback {Playback::Unknown=>None,Playback::Stopped=>Some(false),_=>Some(true)},
            Some("pause"|"play") => match self.playback {Playback::Playing=>Some(true),Playback::Paused=>Some(false),_=>None},
            _=>None,
        };
        match state {Some(value)=>novydeck_sdk::state_message(ctx,value,StateSource::Local),None=>novydeck_sdk::unknown_state_message(ctx)}
    }
    fn command_messages(&mut self,ctx:&str,success:bool)->Vec<Value> {
        if !success{return Vec::new();}
        let action=self.actions.get(ctx).map(String::as_str).unwrap_or("");
        let command=self.command_for(ctx).to_owned();
        if !matches!(action,"pause"|"play"|"stop"|"stopplay"){return Vec::new();}
        self.playback=match command.as_str(){"stop"=>Playback::Stopped,"pause"=>Playback::Paused,"play"=>Playback::Playing,_=>return Vec::new()};
        self.actions.iter().filter(|(_,a)|matches!(a.as_str(),"pause"|"play"|"stopplay"))
            .map(|(context,_)|self.appearance_state(context)).collect()
    }
    fn command_for(&self,ctx:&str)->&str {
        match self.actions.get(ctx).map(String::as_str) {
            Some("pause") if self.playback==Playback::Paused => "play",
            Some("stopplay") if self.playback==Playback::Stopped => "play",
            Some("stopplay") => "stop",
            Some(action) => action,
            None => "",
        }
    }
    fn refresh_pause_titles(&self,sdk:&mut Client)->Result<(),String>{
        let label=if self.playback==Playback::Paused {"Play / Lecture"} else {"Pause"};
        for (ctx,action) in &self.actions {if action=="pause" {sdk.title(ctx,label)?;}}
        Ok(())
    }
    fn refresh_stopplay_titles(&self,sdk:&mut Client)->Result<(),String>{
        let label=if self.playback==Playback::Stopped {"Play / Lecture"} else {"Stop / Arrêt"};
        for (ctx,action) in &self.actions {if action=="stopplay" {sdk.title(ctx,label)?;}}
        Ok(())
    }
}
impl Plugin for Wallpaper {
    fn event(&mut self,e:Event,sdk:&mut Client)->Result<(),String>{
        let ctx=e.context.clone();
        match e.name.as_str(){
            "willAppear"=>{
                self.states.insert(ctx.clone(),e.settings().clone());
                if let Some(action)=e.action.strip_prefix(PREFIX){self.actions.insert(ctx.clone(),action.into());}
                if self.actions.get(&ctx).is_some_and(|a|a=="pause") {self.refresh_pause_titles(sdk)?;}
                else if self.actions.get(&ctx).is_some_and(|a|a=="stopplay") {self.refresh_stopplay_titles(sdk)?;}
                else {sdk.title(&ctx,"Wallpaper")?;}
                if self.actions.get(&ctx).is_some_and(|a|matches!(a.as_str(),"pause"|"play"|"stopplay")) {sdk.send(self.appearance_state(&ctx))?;}
            },
            "willDisappear"=>{self.states.remove(&ctx);self.actions.remove(&ctx);},
            "didReceiveSettings"=>{self.states.insert(ctx.clone(),e.settings().clone());},
            "keyDown" if e.action.starts_with(PREFIX)=>{
                self.actions.insert(ctx.clone(),e.action.strip_prefix(PREFIX).unwrap_or("").into());
                let action=self.command_for(&ctx).to_owned();
                let settings=self.states.get(&ctx).cloned().unwrap_or(Value::Null);
                match run(&action,&settings){
                    Ok(())=>{for message in self.command_messages(&ctx,true){sdk.send(message)?;}self.refresh_pause_titles(sdk)?;self.refresh_stopplay_titles(sdk)?;sdk.ok(&ctx)?;},
                    Err(message)=>{sdk.inspector(&ctx,json!({"type":"message","message":message}))?;sdk.alert(&ctx)?;}
                }
            },_=>(),
        }Ok(())
    }
    fn tick(&mut self,_:&mut Client)->Result<(),String>{Ok(())}
}
fn main(){if let Err(e)=novydeck_sdk::run(Wallpaper::default()){eprintln!("wallpaperengine: {e}");std::process::exit(1);}}
#[cfg(test)] mod tests{use super::*;
    #[test] fn first_appearance_is_unknown_then_pause_play_broadcast_locally(){
        let mut state=Wallpaper::default();
        state.actions.insert("one".into(),"pause".into());state.actions.insert("two".into(),"play".into());
        assert_eq!(state.appearance_state("one"),novydeck_sdk::unknown_state_message("one"));
        let messages=state.command_messages("one",true);
        assert_eq!(messages.len(),2);for message in &messages {assert_eq!(message["payload"]["state"],0);assert_eq!(message["payload"]["novydeckStateSource"],"local");}
        assert_eq!(state.appearance_state("two")["payload"]["state"],0);
        assert!(state.command_messages("one",false).is_empty());assert_eq!(state.appearance_state("two")["payload"]["state"],0);
        let messages=state.command_messages("two",true);assert_eq!(messages.len(),2);for message in &messages {assert_eq!(message["payload"]["state"],1);}
    }
    #[test] fn official_argument_shapes(){let s=json!({"target":"monitor","monitor":0,"playlist":"My Playlist","profile":"Gaming"});assert_eq!(arguments("playlist",&s).unwrap(),["-control","openPlaylist","-playlist","My Playlist","-monitor","0"]);assert_eq!(arguments("next",&s).unwrap(),["-control","nextWallpaper","-monitor","0"]);assert_eq!(arguments("profile",&s).unwrap(),["-control","openProfile","-profile","Gaming"]);assert_eq!(arguments("pause",&s).unwrap(),["-control","pause"]);assert_eq!(arguments("stop",&s).unwrap(),["-control","stop"]);assert_eq!(arguments("play",&s).unwrap(),["-control","play"]);assert!(arguments("stopplay",&s).is_err());}
    #[test] fn stopplay_shared_with_explicit_commands_and_pause(){
        let mut state=Wallpaper::default();
        for (ctx,action) in [("one","stopplay"),("two","stopplay"),("pause","pause"),("play","play"),("stop","stop")] {state.actions.insert(ctx.into(),action.into());}
        assert_eq!(state.command_for("one"),"stop");
        assert_eq!(state.appearance_state("one"),novydeck_sdk::unknown_state_message("one"));
        assert!(state.command_messages("one",false).is_empty());
        assert_eq!(state.command_for("two"),"stop");
        let messages=state.command_messages("one",true);assert_eq!(messages.len(),4);
        assert_eq!(state.command_for("two"),"play");
        assert_eq!(state.appearance_state("one")["payload"]["state"],0);
        assert_eq!(state.appearance_state("one")["payload"]["novydeckStateSource"],"local");
        assert_eq!(state.appearance_state("pause"),novydeck_sdk::unknown_state_message("pause"));
        assert!(state.command_messages("two",false).is_empty());assert_eq!(state.command_for("two"),"play");
        state.command_messages("play",true);assert_eq!(state.command_for("one"),"stop");
        assert_eq!(state.appearance_state("two")["payload"]["state"],1);
        state.command_messages("pause",true);assert_eq!(state.playback,Playback::Paused);
        assert_eq!(state.command_for("pause"),"play");assert_eq!(state.command_for("one"),"stop");
        assert_eq!(state.appearance_state("one")["payload"]["state"],1);
        assert_eq!(state.appearance_state("pause")["payload"]["state"],0);
        state.command_messages("stop",true);assert_eq!(state.command_for("one"),"play");
        assert_eq!(state.appearance_state("pause"),novydeck_sdk::unknown_state_message("pause"));
        state.command_messages("one",true);assert_eq!(state.command_for("two"),"stop");
        assert_eq!(state.appearance_state("pause")["payload"]["state"],1);
    }
    #[test] fn invalid_targets_and_actions(){assert!(arguments("next",&json!({})).is_err());assert!(arguments("next",&json!({"target":"monitor","monitor":32})).is_err());assert!(arguments("playlist",&json!({"target":"location","location":"DISPLAY 1","playlist":"My Playlist"})).unwrap().contains(&"DISPLAY 1".into()));assert!(arguments("command",&json!({})).is_err());assert!(arguments("profile",&json!({"profile":"x\n-inject"})).is_err());}
    #[test] fn pause_alternates_globally_and_failed_commands_do_not_flip(){
        let mut state=Wallpaper::default();
        state.actions.insert("one".into(),"pause".into());
        state.actions.insert("two".into(),"pause".into());
        assert_eq!(state.command_for("one"),"pause");
        state.command_messages("one",true);
        assert_eq!(state.command_for("two"),"play");
        state.command_messages("two",false);
        assert_eq!(state.command_for("one"),"play");
        state.command_messages("two",true);
        assert_eq!(state.command_for("one"),"pause");
        state.actions.insert("three".into(),"play".into());
        state.command_messages("one",true);
        state.command_messages("three",true);
        assert_eq!(state.command_for("two"),"pause");
    }
    #[test] fn real_process_receives_next_and_spaces_and_reports_exit(){
        let dir=std::env::temp_dir().join(format!("wallpaper test {}",std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let source=dir.join("fixture.rs");let exe=dir.join("wallpaper64.exe");let log=dir.join("args.txt");
        std::fs::write(&source,include_str!("../tests/fixtures/cli.rs")).unwrap();
        assert!(Command::new("rustc").args([source.to_str().unwrap(),"-o",exe.to_str().unwrap()]).status().unwrap().success());
        let settings=json!({"executable":exe,"target":"location","location":"DISPLAY 1"});
        std::env::set_var("WALLPAPER_FIXTURE_LOG",&log);
        std::env::set_var("WALLPAPER_FIXTURE_EXIT","0");
        assert!(run("next",&settings).is_ok());
        assert_eq!(std::fs::read_to_string(&log).unwrap(),"-control\nnextWallpaper\n-location\nDISPLAY 1");
        let monitor=json!({"executable":exe,"target":"monitor","monitor":0});
        assert!(run("next",&monitor).is_ok());
        assert_eq!(std::fs::read_to_string(&log).unwrap(),"-control\nnextWallpaper\n-monitor\n0");
        let playlist=json!({"executable":exe,"target":"monitor","monitor":0,"playlist":"My Playlist"});
        assert!(run("playlist",&playlist).is_ok());
        assert_eq!(std::fs::read_to_string(&log).unwrap(),"-control\nopenPlaylist\n-playlist\nMy Playlist\n-monitor\n0");
        assert!(run("stop",&settings).is_ok());
        assert_eq!(std::fs::read_to_string(&log).unwrap(),"-control\nstop");
        assert!(run("play",&settings).is_ok());
        assert_eq!(std::fs::read_to_string(&log).unwrap(),"-control\nplay");
        std::env::set_var("WALLPAPER_FIXTURE_EXIT","7");
        assert!(run("stop",&settings).unwrap_err().contains("7"));
        std::env::remove_var("WALLPAPER_FIXTURE_LOG");std::env::remove_var("WALLPAPER_FIXTURE_EXIT");
        std::fs::remove_dir_all(&dir).unwrap();
    }
    #[test] fn command_uses_real_test_executable(){let path=std::env::temp_dir().join(format!("wallpaper-cli-fixture-{}-{:?}.json",std::process::id(),std::thread::current().id()));let mut file=std::fs::OpenOptions::new().write(true).create_new(true).open(&path).unwrap();use std::io::Write;file.write_all(b"{}").unwrap();let s=json!({"file":path.to_str().unwrap(),"target":"monitor","monitor":1});assert_eq!(arguments("wallpaper",&s).unwrap()[..4],["-control","openWallpaper","-file",path.to_str().unwrap()]);drop(file);std::fs::remove_file(path).unwrap();}
}
