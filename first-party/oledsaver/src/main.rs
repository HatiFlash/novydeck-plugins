use std::{collections::HashMap,mem::{size_of,zeroed},ptr::{null,null_mut},sync::mpsc,thread};
use novydeck_sdk::{Client,Event,Plugin};
use serde_json::{json,Value};
use windows_sys::Win32::{Foundation::{HWND,LPARAM,RECT},Graphics::Gdi::{EnumDisplayMonitors,GetMonitorInfoW,GetStockObject,BLACK_BRUSH,MONITORINFOEXW},UI::WindowsAndMessaging::*};

#[derive(Clone)]struct Screen {name:String,rect:RECT}
unsafe extern "system" fn enumerate(m:isize,_dc:isize,_rect:*mut RECT,data:LPARAM)->i32 {
    let screens=&mut *(data as *mut Vec<Screen>);
    let mut info:MONITORINFOEXW=zeroed();info.monitorInfo.cbSize=size_of::<MONITORINFOEXW>() as u32;
    if GetMonitorInfoW(m,&mut info.monitorInfo)!=0 {
        let name=String::from_utf16_lossy(&info.szDevice[..info.szDevice.iter().position(|c|*c==0).unwrap_or(info.szDevice.len())]);
        screens.push(Screen{name,rect:info.monitorInfo.rcMonitor});
    }
    1
}
fn monitors()->Vec<Screen>{let mut screens=Vec::new();unsafe{EnumDisplayMonitors(0,null(),Some(enumerate),&mut screens as *mut _ as isize);}screens}
fn selected(settings:&Value,count:usize)->Option<usize>{
    let index=settings["monitor"].as_u64().unwrap_or(0);
    usize::try_from(index).ok().filter(|i|*i<count)
}
fn move_close(settings:&Value)->bool{settings["closeOnMove"].as_bool().unwrap_or(false)}
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
        let cls=WNDCLASSW{lpfnWndProc:Some(window_proc),lpszClassName:class.as_ptr(),hbrBackground:GetStockObject(BLACK_BRUSH) as isize,..zeroed()};
        RegisterClassW(&cls);
        let r=screen.rect;
        let hwnd=CreateWindowExW(WS_EX_TOPMOST|WS_EX_TOOLWINDOW,class.as_ptr(),class.as_ptr(),WS_POPUP|WS_VISIBLE,
            r.left,r.top,r.right-r.left,r.bottom-r.top,0,0,0,null());
        if hwnd==0{let _=tx.send(0);return}
        if close_on_move{SetWindowLongPtrW(hwnd,GWLP_USERDATA,1);}
        SetWindowPos(hwnd,HWND_TOPMOST,r.left,r.top,r.right-r.left,r.bottom-r.top,SWP_SHOWWINDOW);
        ShowCursor(0);SetForegroundWindow(hwnd);SetFocus(hwnd);
        let _=tx.send(hwnd);
        let mut message:MSG=zeroed();
        while GetMessageW(&mut message,0,0,0)>0 {TranslateMessage(&message);DispatchMessageW(&message);}
    }
}
struct Active{hwnd:isize,join:thread::JoinHandle<()>}
#[derive(Default)]struct Saver{contexts:HashMap<String,Value>,active:HashMap<String,Active>}
impl Saver{
    fn close(&mut self,ctx:&str){if let Some(active)=self.active.remove(ctx){unsafe{PostMessageW(active.hwnd,WM_CLOSE,0,0);}let _=active.join.join();}}
    fn display(&self,ctx:&str,sdk:&mut Client)->Result<(),String>{
        let label=if self.active.contains_key(ctx){"OLED ON"}else{"OLED OFF"};
        sdk.image(ctx,&novydeck_sdk::render(&[label],None)?)
    }
    fn list(&self,ctx:&str,sdk:&mut Client)->Result<(),String>{
        let screens=monitors().iter().enumerate().map(|(index,m)|json!({"index":index,"name":m.name,"width":m.rect.right-m.rect.left,"height":m.rect.bottom-m.rect.top})).collect::<Vec<_>>();
        sdk.inspector(ctx,json!({"monitors":screens}))
    }
}
impl Drop for Saver{fn drop(&mut self){let contexts=self.active.keys().cloned().collect::<Vec<_>>();for ctx in contexts{self.close(&ctx)}}}
impl Plugin for Saver{
    fn event(&mut self,event:Event,sdk:&mut Client)->Result<(),String>{
        let ctx=event.context;
        match event.name.as_str(){
            "willAppear"=>{self.contexts.insert(ctx.clone(),event.settings().clone());self.display(&ctx,sdk)?;},
            "didReceiveSettings"=>{self.contexts.insert(ctx.clone(),event.settings().clone());},
            "propertyInspectorDidAppear"=>{self.list(&ctx,sdk)?;},
            "willDisappear"=>{self.close(&ctx);self.contexts.remove(&ctx);},
            "keyUp"=>{
                if self.active.contains_key(&ctx){self.close(&ctx);self.display(&ctx,sdk)?;}
                else if let Some(settings)=self.contexts.get(&ctx){
                    let screens=monitors();
                    if let Some(index)=selected(settings,screens.len()){
                        let (tx,rx)=mpsc::channel();let screen=screens[index].clone();let movement=move_close(settings);
                        let join=thread::spawn(move||window(screen,movement,tx));
                        match rx.recv(){Ok(hwnd) if hwnd!=0=>{self.active.insert(ctx.clone(),Active{hwnd,join});self.display(&ctx,sdk)?;},
                            _=>{let _=join.join();sdk.alert(&ctx)?;}}
                    }else{sdk.alert(&ctx)?;}
                }
            },_=>{}
        }
        Ok(())
    }
    fn tick(&mut self,sdk:&mut Client)->Result<(),String>{
        let closed=self.active.iter().filter(|(_,a)|a.join.is_finished()).map(|(k,_)|k.clone()).collect::<Vec<_>>();
        for ctx in closed{self.close(&ctx);self.display(&ctx,sdk)?;}Ok(())
    }
}
fn main(){if let Err(e)=novydeck_sdk::run(Saver::default()){eprintln!("oledsaver: {e}");std::process::exit(1)}}
#[cfg(test)]mod tests{use super::*;
 #[test]fn settings(){assert_eq!(selected(&Value::Null,2),Some(0));assert_eq!(selected(&json!({"monitor":5}),2),None);assert!(move_close(&json!({"closeOnMove":true})));}
 #[test]fn image(){assert!(novydeck_sdk::render(&["OLED OFF"],None).unwrap().len()>100);}
}
