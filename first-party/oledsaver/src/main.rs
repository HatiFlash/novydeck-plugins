use std::{collections::HashMap,mem::{size_of,zeroed},ptr::{null,null_mut},sync::mpsc,thread};
use novydeck_sdk::{Client,Event,Plugin};
use serde_json::{json,Value};
use windows_sys::Win32::{Foundation::{HWND,LPARAM,RECT},Graphics::Gdi::{EnumDisplayMonitors,GetMonitorInfoW,GetStockObject,BLACK_BRUSH,MONITORINFOEXW,HDC,HMONITOR},UI::WindowsAndMessaging::*};

#[derive(Debug,Clone,Copy,PartialEq,Eq)]struct Rect{left:i32,top:i32,right:i32,bottom:i32}
#[derive(Debug,Clone,PartialEq,Eq)]struct Screen {name:String,rect:Rect,primary:bool}
unsafe extern "system" fn enumerate(m:HMONITOR,_dc:HDC,_rect:*mut RECT,data:LPARAM)->i32 {
    let screens=&mut *(data as *mut Vec<Screen>);
    let mut info:MONITORINFOEXW=zeroed();info.monitorInfo.cbSize=size_of::<MONITORINFOEXW>() as u32;
    if GetMonitorInfoW(m,&mut info.monitorInfo)!=0 {
        let name=String::from_utf16_lossy(&info.szDevice[..info.szDevice.iter().position(|c|*c==0).unwrap_or(info.szDevice.len())]);
        let r=info.monitorInfo.rcMonitor;
        let primary=(info.monitorInfo.dwFlags&1)!=0;
        screens.push(Screen{name,rect:Rect{left:r.left,top:r.top,right:r.right,bottom:r.bottom},primary});
    }
    1
}
fn monitors()->Vec<Screen>{let mut screens=Vec::new();unsafe{EnumDisplayMonitors(null_mut(),null(),Some(enumerate),&mut screens as *mut _ as isize);}screens}
/// Screen recorded when the user ticked it in the inspector (robust identity: name, rectangle, index).
#[derive(Debug,Clone,PartialEq,Eq)]struct Wanted{name:String,index:usize,rect:Rect}
#[derive(Debug,Clone,PartialEq,Eq)]enum Target{All,Others,Index(usize),List(Vec<Wanted>)}
fn wanted(v:&Value)->Option<Wanted>{
    let n=|k:&str|v[k].as_i64().and_then(|x|i32::try_from(x).ok());
    let (x,y,w,h)=(n("x")?,n("y")?,n("w")?,n("h")?);
    Some(Wanted{name:v["name"].as_str()?.to_string(),index:usize::try_from(v["index"].as_u64()?).ok()?,rect:Rect{left:x,top:y,right:x.checked_add(w)?,bottom:y.checked_add(h)?}})
}
fn target(settings:&Value)->Target{
    if let Some(list)=settings["monitors"].as_array(){if !list.is_empty(){return Target::List(list.iter().filter_map(wanted).collect())}}
    match settings["monitor"].as_str(){Some("all")=>return Target::All,Some("others")=>return Target::Others,_=>{}}
    Target::Index(usize::try_from(settings["monitor"].as_u64().unwrap_or(0)).unwrap_or(usize::MAX))
}
fn move_close(settings:&Value)->bool{settings["closeOnMove"].as_bool().unwrap_or(false)}
fn keep_focus(settings:&Value)->bool{settings["keepFocus"].as_bool().unwrap_or(false)}
#[derive(Debug,Clone,Copy,PartialEq,Eq)]enum Mode{On,Off,Toggle}
fn mode_checked(settings:&Value)->Result<Mode,String>{match settings["mode"].as_str(){
    None|Some("toggle")=>Ok(Mode::Toggle),Some("on")=>Ok(Mode::On),Some("off")=>Ok(Mode::Off),
    _=>Err("Invalid OLED mode".into())
}}
#[cfg(test)]fn mode(settings:&Value)->Mode{mode_checked(settings).unwrap_or(Mode::Toggle)}
/// What opened an overlay: lets OFF find it again even if names, rectangles or numbering changed.
#[derive(Debug,Clone,PartialEq,Eq)]enum Origin{All,Others,Index(usize),Key(String,Rect)}
/// Overlay currently open: screen name, origin of the request, rectangle.
#[allow(dead_code)]#[derive(Debug,Clone,PartialEq,Eq)]struct ActiveInfo{name:String,origin:Origin,rect:Rect}
#[derive(Debug,Clone,PartialEq,Eq)]struct Open{origin:Origin,screen:Screen}
#[derive(Debug,Default,PartialEq,Eq)]struct Plan{open:Vec<Open>,close:Vec<String>,alert:bool}
/// Pure: current screen index for each wanted entry. Passes in order: name+rect, rect, name, index; one screen per entry.
fn resolve(list:&[Wanted],screens:&[Screen])->Vec<Option<usize>>{
    let mut out=vec![None;list.len()];let mut used=vec![false;screens.len()];
    let rules:[fn(&Wanted,usize,&Screen)->bool;4]=[|w,_,s|w.name==s.name&&w.rect==s.rect,|w,_,s|w.rect==s.rect,|w,_,s|w.name==s.name,|w,i,_|w.index==i];
    for rule in rules{for (j,w) in list.iter().enumerate(){
        if out[j].is_some(){continue}
        if let Some(i)=screens.iter().enumerate().position(|(i,s)|!used[i]&&rule(w,i,s)){out[j]=Some(i);used[i]=true;}
    }}
    out
}
/// Current screens designated by the setting, with the origin to record on their overlay.
fn targets<'a>(target:&Target,screens:&'a [Screen])->Vec<(&'a Screen,Origin)>{match target{
    Target::All=>screens.iter().map(|s|(s,Origin::All)).collect(),
    Target::Others=>screens.iter().filter(|s|!s.primary).map(|s|(s,Origin::Others)).collect(),
    Target::Index(i)=>screens.get(*i).map(|s|(s,Origin::Index(*i))).into_iter().collect(),
    Target::List(list)=>resolve(list,screens).into_iter().zip(list).filter_map(|(r,w)|r.map(|i|(&screens[i],Origin::Key(w.name.clone(),w.rect)))).collect()
}}
/// Active overlays this key controls (see Origin): by resolved screen name or by the origin that opened them.
fn matching(target:&Target,screens:&[Screen],active:&[ActiveInfo])->Vec<String>{
    let tg=targets(target,screens);
    active.iter().filter(|a|match target{
        Target::All=>true,
        Target::Others=>a.origin==Origin::Others||screens.iter().any(|s|s.name==a.name&&!s.primary),
        Target::Index(i)=>a.origin==Origin::Index(*i)||tg.iter().any(|(s,_)|s.name==a.name),
        Target::List(list)=>tg.iter().any(|(s,_)|s.name==a.name)||list.iter().any(|w|a.origin==Origin::Key(w.name.clone(),w.rect))
    }).map(|a|a.name.clone()).collect()
}
fn plan(settings:&Value,screens:&[Screen],active:&[ActiveInfo])->Result<Plan,String>{
    let mode=mode_checked(settings)?;let target=target(settings);
    let found=matching(&target,screens,active);let tg=targets(&target,screens);
    let mut plan=Plan::default();
    if mode==Mode::Off||(mode==Mode::Toggle&&!found.is_empty()){plan.close=found;}
    else{
        for (screen,origin) in &tg{
            if !active.iter().any(|a|a.name==screen.name)&&!plan.open.iter().any(|o|o.screen.name==screen.name){
                plan.open.push(Open{origin:origin.clone(),screen:(*screen).clone()});
            }
        }
    }
    plan.alert=plan.open.is_empty()&&plan.close.is_empty()&&tg.is_empty();
    Ok(plan)
}
const KEEP_FOCUS:isize=8;
unsafe extern "system" fn window_proc(hwnd:HWND,msg:u32,w:usize,l:isize)->isize{
    match msg {
        WM_KEYDOWN|WM_SYSKEYDOWN|WM_LBUTTONDOWN|WM_RBUTTONDOWN|WM_MBUTTONDOWN=>{DestroyWindow(hwnd);0},
        WM_MOUSEACTIVATE=>if GetWindowLongPtrW(hwnd,GWLP_USERDATA)&KEEP_FOCUS!=0{3}else{DefWindowProcW(hwnd,msg,w,l)},
        WM_MOUSEMOVE=>{let state=GetWindowLongPtrW(hwnd,GWLP_USERDATA);
            if state&3==2 {DestroyWindow(hwnd);}else if state&3==1{SetWindowLongPtrW(hwnd,GWLP_USERDATA,(state&!3)|2);}0},
        WM_CLOSE=>{DestroyWindow(hwnd);0},
        WM_DESTROY=>{ShowCursor(1);PostQuitMessage(0);0},
        _=>DefWindowProcW(hwnd,msg,w,l)
    }
}
fn window(screen:Screen,close_on_move:bool,keep:bool,tx:mpsc::Sender<isize>){
    unsafe{
        let class:Vec<u16>="NovyDeckOledSaver\0".encode_utf16().collect();
        let cls=WNDCLASSW{lpfnWndProc:Some(window_proc),lpszClassName:class.as_ptr(),hbrBackground:GetStockObject(BLACK_BRUSH),..zeroed()};
        RegisterClassW(&cls);
        let r=screen.rect;
        let ex=WS_EX_TOPMOST|WS_EX_TOOLWINDOW|if keep{WS_EX_NOACTIVATE}else{0};
        let hwnd=CreateWindowExW(ex,class.as_ptr(),class.as_ptr(),WS_POPUP,
            r.left,r.top,r.right-r.left,r.bottom-r.top,null_mut(),null_mut(),null_mut(),null());
        if hwnd.is_null(){let _=tx.send(0);return}
        SetWindowLongPtrW(hwnd,GWLP_USERDATA,(if close_on_move{1}else{0})|(if keep{KEEP_FOCUS}else{0}));
        if keep{ShowWindow(hwnd,SW_SHOWNOACTIVATE);}
        SetWindowPos(hwnd,HWND_TOPMOST,r.left,r.top,r.right-r.left,r.bottom-r.top,SWP_SHOWWINDOW|if keep{SWP_NOACTIVATE}else{0});
        ShowCursor(0);if !keep{SetForegroundWindow(hwnd);}
        let _=tx.send(hwnd as isize);
        let mut message:MSG=zeroed();
        while GetMessageW(&mut message,null_mut(),0,0)>0 {TranslateMessage(&message);DispatchMessageW(&message);}
    }
}
struct Active{info:ActiveInfo,hwnd:isize,join:thread::JoinHandle<()>}
#[derive(Default)]struct Saver{contexts:HashMap<String,Value>,active:HashMap<String,Active>}
impl Saver{
    fn infos(&self)->Vec<ActiveInfo>{self.active.values().map(|a|a.info.clone()).collect()}
    fn close(&mut self,screen:&str){if let Some(active)=self.active.remove(screen){if !active.join.is_finished(){unsafe{PostMessageW(active.hwnd as HWND,WM_CLOSE,0,0);}}let _=active.join.join();}}
    fn display(&self,ctx:&str,sdk:&mut Client)->Result<(),String>{
        let covered=self.contexts.get(ctx).is_some_and(|s|!matching(&target(s),&monitors(),&self.infos()).is_empty());
        let label=if covered{"OLED ON"}else{"OLED OFF"};
        sdk.image(ctx,&novydeck_sdk::render(&[label],None)?)
    }
    fn refresh(&self,sdk:&mut Client)->Result<(),String>{for ctx in self.contexts.keys(){self.display(ctx,sdk)?;}Ok(())}
    fn list(&self,ctx:&str,sdk:&mut Client)->Result<(),String>{
        let screens=monitors().iter().enumerate().map(|(index,m)|json!({"index":index,"name":m.name,"x":m.rect.left,"y":m.rect.top,"primary":m.primary,"width":m.rect.right-m.rect.left,"height":m.rect.bottom-m.rect.top})).collect::<Vec<_>>();
        sdk.inspector(ctx,json!({"monitors":screens}))
    }
}
impl Drop for Saver{fn drop(&mut self){let screens=self.active.keys().cloned().collect::<Vec<_>>();for screen in screens{self.close(&screen)}}}
impl Plugin for Saver{
    fn event(&mut self,event:Event,sdk:&mut Client)->Result<(),String>{
        let ctx=event.context.clone();
        match event.name.as_str(){
            "willAppear"=>{self.contexts.insert(ctx.clone(),event.settings().clone());self.display(&ctx,sdk)?;},
            "didReceiveSettings"=>{self.contexts.insert(ctx.clone(),event.settings().clone());self.display(&ctx,sdk)?;},
            "propertyInspectorDidAppear"=>{self.list(&ctx,sdk)?;},
            "willDisappear"=>{
                self.contexts.remove(&ctx);
                // A macro may remove the opening key before a different key turns this screen off.
                // Keep the overlay alive until explicitly closed or dismissed by input.
            },
            "keyUp"=>{
                if let Some(settings)=self.contexts.get(&ctx).cloned(){
                    match plan(&settings,&monitors(),&self.infos()){
                        Ok(plan)=>{
                            let mut failed=plan.alert;
                            for name in &plan.close{self.close(name);}
                            for open in plan.open{
                                let (tx,rx)=mpsc::channel();let (movement,keep)=(move_close(&settings),keep_focus(&settings));
                                let info=ActiveInfo{name:open.screen.name.clone(),origin:open.origin,rect:open.screen.rect};
                                let screen=open.screen;
                                let join=thread::spawn(move||window(screen,movement,keep,tx));
                                match rx.recv(){Ok(hwnd) if hwnd!=0=>{self.active.insert(info.name.clone(),Active{info,hwnd,join});},
                                    _=>{let _=join.join();failed=true;}}
                            }
                            if failed{sdk.alert(&ctx)?;}
                            self.refresh(sdk)?;
                        },
                        Err(_)=>sdk.alert(&ctx)?
                    }
                }
            },_=>{}
        }
        Ok(())
    }
    fn tick(&mut self,sdk:&mut Client)->Result<(),String>{
        let closed=self.active.iter().filter(|(_,a)|a.join.is_finished()).map(|(k,_)|k.clone()).collect::<Vec<_>>();
        for screen in &closed{self.close(screen);}
        if !closed.is_empty(){self.refresh(sdk)?;}Ok(())
    }
}
fn main(){if let Err(e)=novydeck_sdk::run(Saver::default()){eprintln!("oledsaver: {e}");std::process::exit(1)}}
#[cfg(test)]mod tests{use super::*;
 fn at(n:&str,x:i32,primary:bool)->Screen{Screen{name:n.into(),rect:Rect{left:x,top:0,right:x+1920,bottom:1080},primary}}
 fn scr(n:&str)->Screen{at(n,0,false)}
 fn act(n:&str,i:Option<usize>)->ActiveInfo{ActiveInfo{name:n.into(),origin:i.map_or(Origin::All,Origin::Index),rect:scr(n).rect}}
 fn act_o(n:&str,x:i32,o:Origin)->ActiveInfo{ActiveInfo{name:n.into(),origin:o,rect:at(n,x,false).rect}}
 fn wt(n:&str,i:usize,x:i32)->Value{json!({"name":n,"index":i,"x":x,"y":0,"w":1920,"h":1080})}
 fn key(n:&str,x:i32)->Origin{Origin::Key(n.into(),at(n,x,false).rect)}
 fn two()->Vec<Screen>{vec![scr("D1"),scr("D2")]}
 fn three()->Vec<Screen>{vec![at("D1",0,true),at("D2",1920,false),at("D3",3840,false)]}
 fn opened(p:&Plan)->Vec<String>{p.open.iter().map(|o|o.screen.name.clone()).collect()}
 fn wl(v:Vec<Value>)->Vec<Wanted>{v.iter().filter_map(wanted).collect()}
 #[test]fn settings(){assert_eq!(target(&Value::Null),Target::Index(0));assert_eq!(target(&json!({"monitor":5})),Target::Index(5));assert_eq!(target(&json!({"monitor":"all"})),Target::All);assert_eq!(target(&json!({"monitor":"others"})),Target::Others);assert!(move_close(&json!({"closeOnMove":true})));}
 #[test]fn monitors_list_has_priority_only_when_non_empty(){
     assert_eq!(target(&json!({"monitor":"all","monitors":[]})),Target::All);
     assert_eq!(target(&json!({"monitor":2,"monitors":[]})),Target::Index(2));
     match target(&json!({"monitor":"all","monitors":[wt("D2",1,1920)]})){Target::List(l)=>{assert_eq!(l.len(),1);assert_eq!(l[0].index,1)},t=>panic!("{t:?}")}}
 #[test]fn keep_focus_defaults_to_false(){assert!(!keep_focus(&Value::Null));assert!(!keep_focus(&json!({"keepFocus":"yes"})));assert!(keep_focus(&json!({"keepFocus":true})));}
 #[test]fn image(){assert!(novydeck_sdk::render(&["OLED OFF"],None).unwrap().len()>100);}
 #[test]fn resolves_all_index_and_out_of_range(){let s=two();
     assert_eq!(targets(&Target::All,&s).len(),2);
     assert_eq!(targets(&Target::Index(1),&s)[0].0.name,"D2");
     assert!(targets(&Target::Index(2),&s).is_empty());}
 #[test]fn resolve_by_name_and_rect(){let s=three();
     assert_eq!(resolve(&wl(vec![wt("D3",2,3840),wt("D2",1,1920)]),&s),[Some(2),Some(1)]);
     // same rect wins over same name when both exist on different screens
     assert_eq!(resolve(&wl(vec![wt("D2",9,3840)]),&s),[Some(2)]);}
 #[test]fn resolve_by_rect_after_rename(){let s=vec![at("D1",0,true),at("D7",1920,false)];
     assert_eq!(resolve(&wl(vec![wt("D2",1,1920)]),&s),[Some(1)]);}
 #[test]fn resolve_by_name_after_resolution_change(){let s=vec![at("D1",0,true),at("D2",5000,false)];
     assert_eq!(resolve(&wl(vec![wt("D2",5,1920)]),&s),[Some(1)]);}
 #[test]fn resolve_by_index_last(){let s=vec![at("D1",0,true),at("DX",7000,false)];
     assert_eq!(resolve(&wl(vec![wt("D2",1,1920)]),&s),[Some(1)]);
     assert_eq!(resolve(&wl(vec![wt("D2",4,1920)]),&s),[None]);}
 #[test]fn two_entries_never_resolve_to_the_same_screen(){let s=three();
     assert_eq!(resolve(&wl(vec![wt("D2",1,1920),wt("D2",1,1920)]),&s),[Some(1),None]);}
 #[test]fn others_never_includes_the_primary(){
     for s in [three(),vec![at("D1",0,false),at("D2",1920,true),at("D3",3840,false)]]{
         let p=plan(&json!({"mode":"on","monitor":"others"}),&s,&[]).unwrap();
         assert_eq!(p.open.len(),2);assert!(p.open.iter().all(|o|!o.screen.primary&&o.origin==Origin::Others));
         let p=plan(&json!({"monitor":"others"}),&s,&[]).unwrap();assert!(p.open.iter().all(|o|!o.screen.primary));}}
 #[test]fn others_with_only_the_primary_alerts(){
     let p=plan(&json!({"mode":"on","monitor":"others"}),&[at("D1",0,true)],&[]).unwrap();assert!(p.alert&&p.open.is_empty());}
 #[test]fn off_others_closes_after_renumbering(){
     // D2 was a secondary screen when opened; now D2 became the primary and D3 appeared.
     let now=vec![at("D3",0,false),at("D2",1920,true)];
     let a=[act_o("D2",1920,Origin::Others),act_o("D4",3840,Origin::Others)];
     let p=plan(&json!({"mode":"off","monitor":"others"}),&now,&a).unwrap();
     assert_eq!(p.close,["D2","D4"]);assert!(p.open.is_empty()&&!p.alert);
     // a non-primary overlay opened by another key is closed too
     let p=plan(&json!({"mode":"off","monitor":"others"}),&now,&[act_o("D3",0,key("D3",0))]).unwrap();assert_eq!(p.close,["D3"]);}
 #[test]fn list_with_two_of_three_covers_only_two(){let s=three();
     let m=json!({"mode":"on","monitors":[wt("D1",0,0),wt("D3",2,3840)]});
     let p=plan(&m,&s,&[]).unwrap();assert_eq!(opened(&p),["D1","D3"]);
     let p=plan(&m,&s,&[act_o("D1",0,key("D1",0)),act_o("D3",3840,key("D3",3840))]).unwrap();assert_eq!(p,Plan::default());}
 #[test]fn off_list_closes_what_on_list_opened_even_if_names_changed(){
     let m_on=json!({"mode":"on","monitors":[wt("D2",1,1920)]});
     let p=plan(&m_on,&three(),&[]).unwrap();assert_eq!(opened(&p),["D2"]);
     let a=ActiveInfo{name:"D2".into(),origin:p.open[0].origin.clone(),rect:p.open[0].screen.rect};
     let now=vec![at("D1",0,true),at("D9",9000,false)];
     let p=plan(&json!({"mode":"off","monitors":[wt("D2",1,1920)]}),&now,&[a.clone()]).unwrap();
     assert_eq!(p.close,["D2"]);assert!(p.open.is_empty()&&!p.alert);
     let p=plan(&json!({"mode":"toggle","monitors":[wt("D2",1,1920)]}),&now,&[a]).unwrap();assert_eq!(p.close,["D2"]);}
 #[test]fn off_list_leaves_unrelated_overlays(){
     let p=plan(&json!({"mode":"off","monitors":[wt("D2",1,1920)]}),&three(),&[act_o("D3",3840,key("D3",3840))]).unwrap();assert_eq!(p,Plan::default());}
 #[test]fn list_without_resolved_target_alerts(){
     let p=plan(&json!({"mode":"on","monitors":[wt("Z",8,99999)]}),&two(),&[]).unwrap();assert!(p.alert);
     let p=plan(&json!({"mode":"on","monitors":[{"bad":true}]}),&two(),&[]).unwrap();assert!(p.alert&&p.open.is_empty());}
 #[test]fn legacy_settings_toggle_screen_zero(){let s=two();
     assert_eq!(mode(&Value::Null),Mode::Toggle);
     let p=plan(&json!({}),&s,&[]).unwrap();assert_eq!(opened(&p),["D1"]);assert!(p.close.is_empty()&&!p.alert);
     let p=plan(&json!({}),&s,&[act("D1",Some(0))]).unwrap();assert_eq!(p.close,["D1"]);assert!(p.open.is_empty());}
 #[test]fn on_is_idempotent_and_never_duplicates_a_screen(){let s=two();
     let p=plan(&json!({"mode":"on","monitor":1}),&s,&[act("D2",Some(1))]).unwrap();assert_eq!(p,Plan::default());
     let p=plan(&json!({"mode":"on","monitor":"all"}),&s,&[act("D1",Some(0))]).unwrap();assert_eq!(opened(&p),["D2"]);assert_eq!(p.open[0].origin,Origin::All);
     let p=plan(&json!({"mode":"on","monitor":"all"}),&s,&[act("D1",None),act("D2",None)]).unwrap();assert_eq!(p,Plan::default());}
 #[test]fn off_closes_by_index_after_screen_name_changed(){
     let now=vec![scr("D3"),scr("D2")];
     let p=plan(&json!({"mode":"off","monitor":0}),&now,&[act("D1",Some(0))]).unwrap();
     assert_eq!(p.close,["D1"]);assert!(p.open.is_empty()&&!p.alert);}
 #[test]fn off_with_missing_index_still_closes_without_alert(){
     let p=plan(&json!({"mode":"off","monitor":3}),&two(),&[act("D9",Some(3))]).unwrap();
     assert_eq!(p.close,["D9"]);assert!(!p.alert);}
 #[test]fn off_all_closes_everything_and_opens_nothing(){
     let p=plan(&json!({"mode":"off","monitor":"all"}),&two(),&[act("D1",Some(0)),act("OLD",Some(7))]).unwrap();
     assert_eq!(p.close.len(),2);assert!(p.open.is_empty());
     assert_eq!(plan(&json!({"mode":"off","monitor":"all"}),&two(),&[]).unwrap(),Plan::default());}
 #[test]fn off_does_not_touch_other_screens(){
     let p=plan(&json!({"mode":"off","monitor":0}),&two(),&[act("D2",Some(1))]).unwrap();assert_eq!(p,Plan::default());}
 #[test]fn toggle_closes_when_a_match_exists_otherwise_opens(){let s=two();
     let p=plan(&json!({"mode":"toggle","monitor":"all"}),&s,&[act("D2",Some(1))]).unwrap();assert_eq!(p.close,["D2"]);assert!(p.open.is_empty());
     let p=plan(&json!({"mode":"toggle","monitor":"all"}),&s,&[]).unwrap();assert_eq!(opened(&p),["D1","D2"]);
     let p=plan(&json!({"monitor":1}),&s,&[act("D1",Some(0))]).unwrap();assert_eq!(opened(&p),["D2"]);}
 #[test]fn invalid_index_without_overlay_alerts(){
     for m in ["on","off","toggle"]{let p=plan(&json!({"mode":m,"monitor":9}),&two(),&[]).unwrap();assert!(p.alert&&p.open.is_empty()&&p.close.is_empty());}}
 #[test]fn covered_label_follows_matching(){let s=two();
     assert!(matching(&Target::All,&s,&[]).is_empty());
     assert!(!matching(&Target::All,&s,&[act("D1",Some(0))]).is_empty());
     assert!(!matching(&Target::Index(1),&s,&[act("D2",None)]).is_empty());
     assert!(matching(&Target::Index(1),&s,&[act("D1",Some(0))]).is_empty());}
 #[test]fn invalid_mode_does_not_activate(){assert!(mode_checked(&json!({"mode":"invalid"})).is_err());assert!(plan(&json!({"mode":"invalid"}),&two(),&[]).is_err());}
}
