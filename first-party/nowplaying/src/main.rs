use std::{collections::HashMap, io::Cursor, time::{Duration,Instant}};
use novydeck_sdk::{Client,Event,Plugin};
use windows::{Media::Control::{GlobalSystemMediaTransportControlsSessionManager as Manager, GlobalSystemMediaTransportControlsSession}, Storage::Streams::{DataReader,IInputStream}};
use image::{DynamicImage,ImageFormat};

const PLAY:&str="com.novydeck.nowplaying.play";
const NEXT:&str="com.novydeck.nowplaying.next";
const PREVIOUS:&str="com.novydeck.nowplaying.previous";
fn short(s:&str)->String{s.chars().take(22).collect()}
fn cover_png(bytes:&[u8])->Option<Vec<u8>>{
    let image=image::load_from_memory(bytes).ok()?.resize_to_fill(144,144,image::imageops::FilterType::Triangle);
    let mut out=Cursor::new(Vec::new());
    image.write_to(&mut out,ImageFormat::Png).ok()?;
    Some(out.into_inner())
}
fn thumbnail(session:&GlobalSystemMediaTransportControlsSession)->Option<Vec<u8>>{
    let media=session.TryGetMediaPropertiesAsync().ok()?.get().ok()?;
    let stream=media.Thumbnail().ok()?.OpenReadAsync().ok()?.get().ok()?;
    let size=stream.Size().ok()?;
    if size==0 || size>5_000_000 {return None}
    let input: IInputStream=stream.cast().ok()?;
    let reader=DataReader::CreateDataReader(&input).ok()?;
    reader.LoadAsync(size as u32).ok()?.get().ok()?;
    let mut bytes=vec![0;size as usize];
    reader.ReadBytes(&mut bytes).ok()?;
    cover_png(&bytes)
}
fn active()->Option<GlobalSystemMediaTransportControlsSession>{
    Manager::RequestAsync().ok()?.get().ok()?.GetCurrentSession().ok()
}
#[derive(Default)]struct NowPlaying {contexts:HashMap<String,String>,last:Option<Instant>}
impl Plugin for NowPlaying {
    fn event(&mut self,event:Event,sdk:&mut Client)->Result<(),String>{
        match event.name.as_str(){
            "willAppear"=>{self.contexts.insert(event.context.clone(),event.action);self.last=None;},
            "willDisappear"=>{self.contexts.remove(&event.context);},
            "keyUp"=>{
                if let Some(action)=self.contexts.get(&event.context){
                    let result=active().and_then(|s|match action.as_str(){
                        PLAY=>s.TryTogglePlayPauseAsync().ok()?.get().ok(),
                        NEXT=>s.TrySkipNextAsync().ok()?.get().ok(),
                        PREVIOUS=>s.TrySkipPreviousAsync().ok()?.get().ok(),
                        _=>None
                    });
                    if result!=Some(true){sdk.alert(&event.context)?;}
                    self.last=None;
                }
            },_=>{}
        }
        Ok(())
    }
    fn tick(&mut self,sdk:&mut Client)->Result<(),String>{
        if self.contexts.is_empty() || self.last.is_some_and(|t|t.elapsed()<Duration::from_secs(2)){return Ok(())}
        self.last=Some(Instant::now());
        let session=active();
        let media=session.as_ref().and_then(|s|s.TryGetMediaPropertiesAsync().ok()?.get().ok());
        let title=media.as_ref().and_then(|m|m.Title().ok()).map(|s|short(&s.to_string())).unwrap_or_default();
        let artist=media.as_ref().and_then(|m|m.Artist().ok()).map(|s|short(&s.to_string())).unwrap_or_default();
        let cover=session.as_ref().and_then(thumbnail);
        for (ctx,action) in &self.contexts {
            let label=match action.as_str(){PLAY=>"Play/Pause",NEXT=>"Next",PREVIOUS=>"Previous",_=>continue};
            let fallback=if session.is_none(){vec![label,"No player"]}else{vec![label,if title.is_empty(){"—"}else{&title},&artist]};
            let png=if action==PLAY {cover.clone()}else{None};
            sdk.image(ctx,&png.unwrap_or(novydeck_sdk::render(&fallback,None)?))?;
        }
        Ok(())
    }
}
fn main(){if let Err(e)=novydeck_sdk::run(NowPlaying::default()){eprintln!("nowplaying: {e}");std::process::exit(1)}}
#[cfg(test)]mod tests{use super::*;
 #[test]fn no_cover(){assert!(cover_png(&[]).is_none());assert_eq!(short("éé"),"éé");}
 #[test]fn cover_render(){let png=novydeck_sdk::render(&["No player"],None).unwrap();let result=cover_png(&png).unwrap();let decoded=image::load_from_memory(&result).unwrap();assert_eq!((decoded.width(),decoded.height()),(144,144));}
}
