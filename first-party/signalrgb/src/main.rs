use std::{collections::HashMap,io::{Read,Write},net::{IpAddr,Ipv4Addr,SocketAddr,TcpStream},time::Duration};
use novydeck_sdk::{Client,Event,Plugin};
use serde_json::{json,Value};

const ENDPOINT:&str="127.0.0.1:16038";
const PATH:&str="/api/v1/lighting/enabled";
const TIMEOUT:Duration=Duration::from_secs(2);

fn enabled(settings:&Value)->Result<bool,String>{match settings["mode"].as_str(){
    Some("on")=>Ok(true),Some("off")=>Ok(false),_=>Err("Choose ON or OFF / Choisissez ON ou OFF".into())
}}
// Fixed loopback address; neither host nor path comes from button settings.
fn patch_at(address:SocketAddr,on:bool)->Result<(),String>{
    if address.ip()!=IpAddr::V4(Ipv4Addr::LOCALHOST){return Err("SignalRGB must use 127.0.0.1".into());}
    let mut stream=TcpStream::connect_timeout(&address,TIMEOUT).map_err(|e|format!("SignalRGB unavailable: {e}"))?;
    stream.set_read_timeout(Some(TIMEOUT)).map_err(|e|e.to_string())?;
    stream.set_write_timeout(Some(TIMEOUT)).map_err(|e|e.to_string())?;
    let body=if on {r#"{"enabled":true}"#} else {r#"{"enabled":false}"#};
    let request=format!("PATCH {PATH} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len());
    stream.write_all(request.as_bytes()).map_err(|e|format!("SignalRGB write failed: {e}"))?;
    let mut response=Vec::new();let mut chunk=[0u8;512];
    while !response.windows(4).any(|w|w==b"\r\n\r\n") && response.len()<8192 {
        let n=stream.read(&mut chunk).map_err(|e|format!("SignalRGB response failed: {e}"))?;
        if n==0 {break;}response.extend_from_slice(&chunk[..n]);
    }
    if response.len()>8192 || !response.windows(4).any(|w|w==b"\r\n\r\n") {return Err("Invalid SignalRGB HTTP response".into());}
    let line=response.split(|b|*b==b'\n').next().unwrap_or_default();
    let line=std::str::from_utf8(line).map_err(|_|"Invalid SignalRGB HTTP status")?;
    let mut fields=line.trim_end_matches('\r').split_whitespace();
    if fields.next()!=Some("HTTP/1.1") && !line.starts_with("HTTP/1.0 "){return Err("Invalid SignalRGB HTTP status".into());}
    let status=fields.next().and_then(|s|s.parse::<u16>().ok()).ok_or("Invalid SignalRGB HTTP status")?;
    if (200..300).contains(&status){Ok(())}else{Err(format!("SignalRGB HTTP {status}"))}
}
fn patch(on:bool)->Result<(),String>{patch_at(ENDPOINT.parse().expect("fixed loopback address"),on)}

#[derive(Default)]struct SignalRgb{settings:HashMap<String,Value>}
impl Plugin for SignalRgb{
    fn event(&mut self,e:Event,sdk:&mut Client)->Result<(),String>{
        let ctx=e.context.clone();
        match e.name.as_str(){
            "willAppear"|"didReceiveSettings"=>{self.settings.insert(ctx,e.settings().clone());},
            "willDisappear"=>{self.settings.remove(&ctx);},
            "keyDown" if e.action=="com.novydeck.signalrgb.lighting"=>{
                let result=self.settings.get(&ctx).ok_or("Missing SignalRGB settings".into()).and_then(enabled).and_then(patch);
                match result{Ok(())=>sdk.ok(&ctx)?,Err(message)=>{
                    sdk.inspector(&ctx,json!({"type":"message","message":message}))?;
                    sdk.alert(&ctx)?;
                }}
            },_=>()
        }Ok(())
    }
    fn tick(&mut self,_:&mut Client)->Result<(),String>{Ok(())}
}
fn main(){if let Err(e)=novydeck_sdk::run(SignalRgb::default()){eprintln!("signalrgb: {e}");std::process::exit(1)}}

#[cfg(test)]mod tests{use super::*;use std::{net::TcpListener,thread};
    #[test]fn explicit_mode_only(){assert_eq!(enabled(&json!({"mode":"off"})),Ok(false));assert_eq!(enabled(&json!({"mode":"on"})),Ok(true));assert!(enabled(&Value::Null).is_err());assert!(enabled(&json!({"mode":"toggle"})).is_err());}
    fn serve(status:u16)->(SocketAddr,thread::JoinHandle<String>){
        let listener=TcpListener::bind("127.0.0.1:0").unwrap();let address=listener.local_addr().unwrap();
        let handle=thread::spawn(move||{
            let (mut stream,_)=listener.accept().unwrap();stream.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
            let mut bytes=Vec::new();let mut buf=[0;512];
            loop{
                let n=stream.read(&mut buf).unwrap();assert!(n>0);bytes.extend_from_slice(&buf[..n]);
                if let Some(split)=bytes.windows(4).position(|w|w==b"\r\n\r\n"){
                    let header=String::from_utf8_lossy(&bytes[..split]);
                    let length=header.lines().find_map(|line|line.strip_prefix("Content-Length: ")).unwrap().parse::<usize>().unwrap();
                    if bytes.len()>=split+4+length {break;}
                }
            }
            let reply=format!("HTTP/1.1 {status} Test\r\nContent-Length: 0\r\n\r\n");stream.write_all(reply.as_bytes()).unwrap();
            String::from_utf8(bytes).unwrap()
        });(address,handle)
    }
    #[test]fn local_server_receives_exact_patch_and_false_body(){
        let (address,server)=serve(204);assert!(patch_at(address,false).is_ok());
        let request=server.join().unwrap();assert!(request.starts_with("PATCH /api/v1/lighting/enabled HTTP/1.1\r\n"));
        assert!(request.contains("Host: 127.0.0.1\r\n"));assert!(request.ends_with("\r\n\r\n{\"enabled\":false}"));
    }
    #[test]fn true_body_and_error_status(){let (address,server)=serve(500);assert!(patch_at(address,true).unwrap_err().contains("500"));assert!(server.join().unwrap().ends_with("\r\n\r\n{\"enabled\":true}"));}
    #[test]fn refuses_non_loopback(){assert!(patch_at("192.0.2.1:80".parse().unwrap(),false).is_err());}
}
