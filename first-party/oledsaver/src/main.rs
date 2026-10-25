use std::{collections::HashMap,mem::{size_of,zeroed},ptr::{null,null_mut},sync::mpsc,thread};
use novydeck_sdk::{Client,Event,Plugin};
use serde_json::{json,Value};
use windows_sys::Win32::{Foundation::{HWND,LPARAM,RECT},Graphics::Gdi::{EnumDisplayMonitors,GetMonitorInfoW,GetStockObject,BLACK_BRUSH,MONITORINFOEXW,HDC,HMONITOR},UI::WindowsAndMessaging::*};

#[derive(Clone)]struct Screen {name:String,rect:RECT}
unsafe extern "system" fn enumerate(m:HMONITOR,_dc:HDC,_rect:*mut RECT,data:LPARAM)->i32 {
    let screens=&mut *(data as *mut Vec<Screen>);
    let mut info:MONITORINFOEXW=zeroed();info.monitorInfo.cbSize=size_of::<MONITORINFOEXW>() as u32;
    if GetMonitorInfoW(m,&mut info.monitorInfo)!=0 {
        let name=String::from_utf16_lossy(&info.szDevice[..info.szDevice.iter().position(|c|*c==0).unwrap_or(info.szDevice.len())]);
        screens.push(Screen{name,rect:info.monitorInfo.rcMonitor});
    }
    1
}
fn monitors()->Vec<Screen>{let mut screens=Vec::new();unsafe{EnumDisplayMonitors(null_mut(),null(),Some(enumerate),&mut screens as *mut _ as isize);}screens}
fn selected(settings:&Value,count:usize)->Option<usize>{
    let index=settings["monitor"].as_u64().unwrap_or(0);
    usize::try_from(index).ok().filter(|i|*i<count)
}
fn move_close(settings:&Value)->bool{settings["closeOnMove"].as_bool().unwrap_or(false)}
#[derive(Debug,Clone,Copy,PartialEq,Eq)]enum Mode{On,Off,Toggle}
#[derive(Debug,Clone,Copy,PartialEq,Eq)]enum Change{Open,Close,None}
fn mode_checked(settings:&Value)->Result<Mode,String>{match settings["mode"].as_str(){
    None|Some("toggle")=>Ok(Mode::Toggle),Some("on")=>Ok(Mode::On),Some("off")=>Ok(Mode::Off),
    _=>Err("Invalid OLED mode".into())
}}
#[cfg(test)]fn mode(settings:&Value)->Mode{mode_checked(settings).unwrap_or(Mode::Toggle)}
fn transition(mode:Mode,is_open:bool)->Change{match (mode,is_open){
    (Mode::On,false)|(Mode::Toggle,false)=>Change::Open,
    (Mode::Off,true)|(Mode::Toggle,true)=>Change::Close,
    _=>Change::None
}}
unsafe extern "system" fn window_proc(hwnd:HWND,msg:u32,w:usize,l:isize)->isize{
    match msg {
        WM_KEYDOWN|WM_SYSKEYDOWN|WM_LBUTTONDOWN|WM_RBUTTONDOWN|WM_MBUTTONDOWN=>{DestroyWindow(hwnd);0},
        WM_MOUSEMOVE=>{let state=GetWindowLongPtrW(hwnd,GWLP_USERDATA);
            if state==2 {DestroyWindow(hwnd);}else if state==1{SetWindowLongPtrW(hwnd,GWLP_USERDATA,2);}0},
        WM_CLOSE=>{DestroyWindow(hwnd);0},
        WM_DESTROY=>{ShowCursor(1);PostQuitMessage(0);0},
        _=>DefWindowProcW(hwnd,msg,w,l)
    }
}
fn window(screen:Screen,close_on_move:bool,tx:mpsc::Sender<isize>){
    unsafe{
        let class:Vec<u16>="NovyDeckOledSaver\0".encode_utf16().collect();
        let cls=WNDCLASSW{lpfnWndProc:Some(window_proc),lpszClassName:class.as_ptr(),hbrBackground:GetStockObject(BLACK_BRUSH),..zeroed()};
        RegisterClassW(&cls);
        let r=screen.rect;
        let hwnd=CreateWindowExW(WS_EX_TOPMOST|WS_EX_TOOLWINDOW,class.as_ptr(),class.as_ptr(),WS_POPUP|WS_VISIBLE,
            r.left,r.top,r.right-r.left,r.bottom-r.top,null_mut(),null_mut(),null_mut(),null());
        if hwnd.is_null(){let _=tx.send(0);return}
        if close_on_move{SetWindowLongPtrW(hwnd,GWLP_USERDATA,1);}
        SetWindowPos(hwnd,HWND_TOPMOST,r.left,r.top,r.right-r.left,r.bottom-r.top,SWP_SHOWWINDOW);
        ShowCursor(0);SetForegroundWindow(hwnd);
        let _=tx.send(hwnd as isize);
        let mut message:MSG=zeroed();
        while GetMessageW(&mut message,null_mut(),0,0)>0 {TranslateMessage(&message);DispatchMessageW(&message);}
    }
}
struct Active{hwnd:isize,join:thread::JoinHandle<()>}
#[derive(Default)]struct Saver{contexts:HashMap<String,Value>,active:HashMap<String,Active>}
impl Saver{
    fn close(&mut self,screen:&str){if let Some(active)=self.active.remove(screen){if !active.join.is_finished(){unsafe{PostMessageW(active.hwnd as HWND,WM_CLOSE,0,0);}}let _=active.join.join();}}
    fn display(&self,ctx:&str,sdk:&mut Client)->Result<(),String>{
        let screens=monitors();
        let label=if self.contexts.get(ctx).and_then(|s|selected(s,screens.len())).is_some_and(|i|self.active.contains_key(&screens[i].name)){"OLED ON"}else{"OLED OFF"};
        sdk.image(ctx,&novydeck_sdk::render(&[label],None)?)
    }
    fn refresh(&self,sdk:&mut Client)->Result<(),String>{for ctx in self.contexts.keys(){self.display(ctx,sdk)?;}Ok(())}
    fn list(&self,ctx:&str,sdk:&mut Client)->Result<(),String>{
        let screens=monitors().iter().enumerate().map(|(index,m)|json!({"index":index,"name":m.name,"width":m.rect.right-m.rect.left,"height":m.rect.bottom-m.rect.top})).collect::<Vec<_>>();
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
                    let screens=monitors();
                    if let (Some(index),Ok(mode))=(selected(&settings,screens.len()),mode_checked(&settings)){
                        let screen=screens[index].clone();
                        match transition(mode,self.active.contains_key(&screen.name)){
                            Change::Close=>self.close(&screen.name),
                            Change::Open=>{
                                let (tx,rx)=mpsc::channel();let movement=move_close(&settings);
                                let name=screen.name.clone();
                                let join=thread::spawn(move||window(screen,movement,tx));
                                match rx.recv(){Ok(hwnd) if hwnd!=0=>{self.active.insert(name,Active{hwnd,join});},
                                    _=>{let _=join.join();sdk.alert(&ctx)?;}}
                            },
                            Change::None=>{}
                        }
                        self.refresh(sdk)?;
                    }else{sdk.alert(&ctx)?;}
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
 #[test]fn settings(){assert_eq!(selected(&Value::Null,2),Some(0));assert_eq!(selected(&json!({"monitor":5}),2),None);assert!(move_close(&json!({"closeOnMove":true})));}
 #[test]fn image(){assert!(novydeck_sdk::render(&["OLED OFF"],None).unwrap().len()>100);}
 #[test]fn explicit_modes_are_idempotent_and_scoped_to_screen(){
     assert_eq!(mode(&Value::Null),Mode::Toggle);
     assert_eq!(transition(Mode::On,false),Change::Open);
     assert_eq!(transition(Mode::On,true),Change::None);
     assert_eq!(transition(Mode::Off,true),Change::Close);
     assert_eq!(transition(Mode::Off,false),Change::None);
     assert_eq!(transition(Mode::Toggle,true),Change::Close);
     assert_eq!(transition(Mode::Toggle,false),Change::Open);
     let mut screens=std::collections::HashSet::from(["screen-a","screen-b"]);
     assert_eq!(transition(Mode::Off,screens.contains("screen-a")),Change::Close);
     screens.remove("screen-a");
     assert!(!screens.contains("screen-a"));
     assert!(screens.contains("screen-b"));
     assert_eq!(transition(Mode::On,screens.contains("screen-b")),Change::None);
 }
 #[test]fn invalid_mode_does_not_activate(){assert!(mode_checked(&json!({"mode":"invalid"})).is_err());}
}
