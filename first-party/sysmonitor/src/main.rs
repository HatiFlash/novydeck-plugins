use std::{collections::HashMap, mem::{size_of, zeroed}, ptr::{null, null_mut}, time::{Duration, Instant}};
use novydeck_sdk::{Client, Event, Plugin};
use windows_sys::Win32::{Foundation::FILETIME, System::{Threading::GetSystemTimes, SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX}, Performance::*}};

const CPU: &str = "com.novydeck.sysmonitor.cpu";
const RAM: &str = "com.novydeck.sysmonitor.ram";
const GPU: &str = "com.novydeck.sysmonitor.gpu";
const TEMP: &str = "com.novydeck.sysmonitor.temperature";
fn percent(v: Option<f64>) -> String { v.map(|n| format!("{}%", n.clamp(0.0,100.0).round() as u32)).unwrap_or_else(|| "—".into()) }
fn cpu_temp() -> &'static str { "n/d" } // Windows does not provide reliable CPU sensor data without a third-party driver.
fn ticks(f: FILETIME) -> u64 { ((f.dwHighDateTime as u64) << 32) | f.dwLowDateTime as u64 }
fn cpu_times() -> Option<(u64,u64)> {
    unsafe { let (mut idle,mut kernel,mut user)=(zeroed(),zeroed(),zeroed());
        if GetSystemTimes(&mut idle,&mut kernel,&mut user)==0 { None } else { Some((ticks(idle),ticks(kernel)+ticks(user))) } }
}
fn cpu_load(before: (u64,u64), after: (u64,u64)) -> Option<f64> {
    let total=after.1.checked_sub(before.1)?; let idle=after.0.checked_sub(before.0)?;
    (total>0).then(|| 100.0*(1.0-idle as f64/total as f64))
}
fn memory() -> Option<(f64,f64)> {
    unsafe { let mut m: MEMORYSTATUSEX=zeroed(); m.dwLength=size_of::<MEMORYSTATUSEX>() as u32;
        if GlobalMemoryStatusEx(&mut m)==0 || m.ullTotalPhys==0 {None} else {
            Some((100.0*(m.ullTotalPhys-m.ullAvailPhys) as f64/m.ullTotalPhys as f64, (m.ullTotalPhys-m.ullAvailPhys) as f64/1073741824.0))
        } }
}
fn wide(s:&str)->Vec<u16> { s.encode_utf16().chain(Some(0)).collect() }
struct GpuCounters { query: isize, counters: Vec<isize> }
impl GpuCounters {
    fn open() -> Option<Self> {
        unsafe {
            let path=wide("\\GPU Engine(*)\\Utilization Percentage"); let mut len=0;
            let status=PdhExpandWildCardPathW(null(),path.as_ptr(),null_mut(),&mut len,0);
            if status!=PDH_MORE_DATA || len==0 || len>65536 {return None}
            let mut paths=vec![0u16;len as usize];
            if PdhExpandWildCardPathW(null(),path.as_ptr(),paths.as_mut_ptr(),&mut len,0)!=0 {return None}
            let mut query=0;
            if PdhOpenQueryW(null(),0,&mut query)!=0 {return None}
            let mut counters=Vec::new();
            for part in paths.split(|c| *c==0).filter(|p| !p.is_empty()) {
                let mut p=part.to_vec();p.push(0);let mut counter=0;
                if PdhAddEnglishCounterW(query,p.as_ptr(),0,&mut counter)==0 { counters.push(counter); }
            }
            if counters.is_empty() {PdhCloseQuery(query);return None}
            PdhCollectQueryData(query);
            Some(Self {query,counters})
        }
    }
    fn sample(&self)->Option<f64> {
        unsafe {if PdhCollectQueryData(self.query)!=0{return None}
            let mut by_engine: HashMap<String,f64>=HashMap::new();
            // Each PDH instance represents one process on one engine; sum process loads per engine,
            // then take the busiest engine rather than summing unrelated engine percentages.
            for (i,c) in self.counters.iter().enumerate() {
                let mut value: PDH_FMT_COUNTERVALUE=zeroed();
                if PdhGetFormattedCounterValue(*c,PDH_FMT_DOUBLE,null_mut(),&mut value)==0 &&
                   (value.CStatus==PDH_CSTATUS_VALID_DATA || value.CStatus==PDH_CSTATUS_NEW_DATA) {
                    let v=value.Anonymous.doubleValue;
                    if v.is_finite() && v>=0.0 { *by_engine.entry(format!("{i}")).or_default()+=v; }
                }
            }
            (!by_engine.is_empty()).then(|| by_engine.values().copied().fold(0.0,f64::max).min(100.0))
        }
    }
}
impl Drop for GpuCounters { fn drop(&mut self) {unsafe {PdhCloseQuery(self.query);} } }
#[derive(Default)] struct Monitor { contexts:HashMap<String,String>, last:Option<Instant>, previous:Option<(u64,u64)>, gpu:Option<GpuCounters> }
impl Plugin for Monitor {
    fn event(&mut self,event:Event,sdk:&mut Client)->Result<(),String> {
        match event.name.as_str() {
            "willAppear"=>{self.contexts.insert(event.context.clone(),event.action);self.last=None;},
            "willDisappear"=>{self.contexts.remove(&event.context);},
            "keyUp"=>{if self.contexts.contains_key(&event.context) { if let Err(e)=std::process::Command::new("taskmgr.exe").spawn() {sdk.log(&format!("Task Manager: {e}"))?;sdk.alert(&event.context)?;} }},
            _=>{}
        }
        Ok(())
    }
    fn tick(&mut self,sdk:&mut Client)->Result<(),String> {
        if self.contexts.is_empty() || self.last.is_some_and(|t|t.elapsed()<Duration::from_secs(2)){return Ok(())}
        self.last=Some(Instant::now());
        let now=cpu_times();let cpu=self.previous.zip(now).and_then(|(a,b)|cpu_load(a,b));self.previous=now;
        let ram=memory();
        if self.gpu.is_none(){self.gpu=GpuCounters::open();}
        let gpu=self.gpu.as_ref().and_then(GpuCounters::sample);
        // Nvml::init loads nvml.dll dynamically; NVIDIA absent or inaccessible means unavailable.
        let temperature=nvml_wrapper::Nvml::init().ok().and_then(|nvml|nvml.device_by_index(0).ok()
            .and_then(|dev|dev.temperature(nvml_wrapper::enum_wrappers::device::TemperatureSensor::Gpu).ok()));
        for (ctx,action) in &self.contexts {
            let (lines,gauge):(Vec<String>,Option<f32>)=match action.as_str(){
                CPU=>(vec!["CPU".into(),percent(cpu)],cpu.map(|v|(v/100.0) as f32)),
                RAM=>(vec!["RAM".into(),percent(ram.map(|x|x.0)),ram.map(|x|format!("{:.1} GiB",x.1)).unwrap_or_else(||"—".into())],ram.map(|x|(x.0/100.0) as f32)),
                GPU=>(vec!["GPU".into(),percent(gpu)],gpu.map(|v|(v/100.0) as f32)),
                TEMP=>(vec!["CPU n/d".into(),format!("GPU {}",temperature.map(|x|format!("{x}°C")).unwrap_or_else(||"—".into()))],None),
                _=>continue
            };
            sdk.image(ctx,&novydeck_sdk::render(&lines.iter().map(String::as_str).collect::<Vec<_>>(),gauge)?)?;
        }
        Ok(())
    }
}
fn main(){if let Err(e)=novydeck_sdk::run(Monitor::default()){eprintln!("sysmonitor: {e}");std::process::exit(1)}}
#[cfg(test)] mod tests {use super::*;
 #[test]fn formatting(){assert_eq!(percent(None),"—");assert_eq!(percent(Some(105.0)),"100%");assert_eq!(cpu_temp(),"n/d");assert_eq!(cpu_load((2,10),(3,20)),Some(90.0));}
 #[test]fn png(){let p=novydeck_sdk::render(&["GPU","—"],None).unwrap();assert!(p.len()>100);}
}
