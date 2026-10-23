use std::{collections::HashMap, mem::{size_of, zeroed}, ptr::{null, null_mut}, time::{Duration, Instant}};
use novydeck_sdk::{Client, Event, Plugin};
use windows_sys::Win32::{Foundation::FILETIME, System::{Threading::GetSystemTimes, SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX}, Performance::*}};
use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIFactory1};

const CPU: &str = "com.novydeck.sysmonitor.cpu";
const RAM: &str = "com.novydeck.sysmonitor.ram";
const GPU: &str = "com.novydeck.sysmonitor.gpu";
const TEMP: &str = "com.novydeck.sysmonitor.temperature";
fn unavailable(fr: bool) -> &'static str { if fr { "capteur indisponible" } else { "sensor unavailable" } }
fn percent(v: Option<f64>) -> String { v.filter(|n| n.is_finite()).map(|n| format!("{}%", n.clamp(0.0,100.0).round() as u32)).unwrap_or_else(|| "N/D".into()) }
fn kelvin_to_celsius(kelvin: f64) -> Option<f64> {
    let c = kelvin - 273.15;
    (kelvin.is_finite() && (0.0..=125.0).contains(&c)).then_some(c)
}
fn temperature(v: Option<f64>) -> String { v.filter(|c| c.is_finite() && (0.0..=125.0).contains(c)).map(|c| format!("{c:.0}°C")).unwrap_or_else(|| "N/D".into()) }
fn temperature_lines(cpu: Option<f64>, gpu: Option<f64>, fr: bool) -> Vec<String> {
    let mut lines=vec![format!("CPU {}", temperature(cpu)), format!("GPU {}", temperature(gpu))];
    if cpu.is_none() || gpu.is_none() { lines.push(unavailable(fr).into()); }
    lines
}
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
// DXGI enumerates the adapter driving the primary desktop output first.
fn primary_luid() -> Option<(u32,u32)> {
    unsafe {
        let factory: IDXGIFactory1 = CreateDXGIFactory1().ok()?;
        let desc = factory.EnumAdapters1(0).ok()?.GetDesc1().ok()?;
        Some((desc.AdapterLuid.HighPart as u32, desc.AdapterLuid.LowPart))
    }
}
fn engine_key(path:&str)->Option<((u32,u32),String)> {
    let path=path.to_ascii_lowercase();
    let instance=path.split('(').nth(1)?.split(')').next()?;
    let luid=instance.split("luid_0x").nth(1)?;
    let (high,rest)=luid.split_once("_0x")?;
    let low=rest.split('_').next()?;
    let engine=instance.split("_engtype_").nth(1)?.split('_').next()?;
    if engine.is_empty(){return None}
    Some(((u32::from_str_radix(high,16).ok()?,u32::from_str_radix(low,16).ok()?),engine.into()))
}
fn engine_usage<'a>(samples:impl IntoIterator<Item=(&'a ((u32,u32),String),f64)>, primary:(u32,u32))->Option<f64>{
    let mut by_engine: HashMap<&str,f64>=HashMap::new();
    for (key,value) in samples {
        if key.0==primary && value.is_finite() && value>=0.0 {
            *by_engine.entry(&key.1).or_default()+=value;
        }
    }
    by_engine.values().copied().reduce(f64::max).map(|v|v.clamp(0.0,100.0))
}
struct GpuCounters { query: isize, counters: Vec<(isize,((u32,u32),String))>, primary:(u32,u32) }
impl GpuCounters {
    fn open() -> Option<Self> {
        let primary=primary_luid()?;
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
                let text=String::from_utf16_lossy(part);
                if let Some(key)=engine_key(&text) {
                    if key.0!=primary {continue}
                    let mut p=part.to_vec();p.push(0);let mut counter=0;
                    if PdhAddEnglishCounterW(query,p.as_ptr(),0,&mut counter)==0 { counters.push((counter,key)); }
                }
            }
            if counters.is_empty() {PdhCloseQuery(query);return None}
            PdhCollectQueryData(query);
            Some(Self {query,counters,primary})
        }
    }
    fn sample(&self)->Option<f64> {
        unsafe {if PdhCollectQueryData(self.query)!=0{return None}
            let mut samples=Vec::new();
            for (counter,key) in &self.counters {
                let mut value: PDH_FMT_COUNTERVALUE=zeroed();
                if PdhGetFormattedCounterValue(*counter,PDH_FMT_DOUBLE,null_mut(),&mut value)==0 &&
                   (value.CStatus==PDH_CSTATUS_VALID_DATA || value.CStatus==PDH_CSTATUS_NEW_DATA) {
                    samples.push((key,value.Anonymous.doubleValue));
                }
            }
            engine_usage(samples,self.primary)
        }
    }
}
impl Drop for GpuCounters { fn drop(&mut self) {unsafe {PdhCloseQuery(self.query);} } }
// PDH thermal zones are ACPI zones, not guaranteed to be the CPU die sensor.
struct ThermalCounters { query: isize, counters: Vec<isize> }
impl ThermalCounters {
    fn open() -> Option<Self> {
        unsafe {
            let path=wide("\\Thermal Zone Information(*)\\Temperature");let mut len=0;
            if PdhExpandWildCardPathW(null(),path.as_ptr(),null_mut(),&mut len,0)!=PDH_MORE_DATA || len==0 || len>65536{return None}
            let mut paths=vec![0u16;len as usize];
            if PdhExpandWildCardPathW(null(),path.as_ptr(),paths.as_mut_ptr(),&mut len,0)!=0{return None}
            let mut query=0;
            if PdhOpenQueryW(null(),0,&mut query)!=0{return None}
            let mut counters=Vec::new();
            for part in paths.split(|c|*c==0).filter(|p|!p.is_empty()) {
                let mut path=part.to_vec();path.push(0);let mut counter=0;
                if PdhAddEnglishCounterW(query,path.as_ptr(),0,&mut counter)==0 { counters.push(counter); }
            }
            if counters.is_empty(){PdhCloseQuery(query);return None}
            Some(Self {query,counters})
        }
    }
    fn sample(&self)->Option<f64> {
        unsafe {
            if PdhCollectQueryData(self.query)!=0{return None}
            self.counters.iter().find_map(|counter|{
                let mut value:PDH_FMT_COUNTERVALUE=zeroed();
                if PdhGetFormattedCounterValue(*counter,PDH_FMT_DOUBLE,null_mut(),&mut value)==0 &&
                    (value.CStatus==PDH_CSTATUS_VALID_DATA || value.CStatus==PDH_CSTATUS_NEW_DATA) {
                    kelvin_to_celsius(value.Anonymous.doubleValue)
                } else {None}
            })
        }
    }
}
impl Drop for ThermalCounters {fn drop(&mut self){unsafe{PdhCloseQuery(self.query);} }}
#[derive(Default)] struct Monitor { contexts:HashMap<String,String>, last:Option<Instant>, previous:Option<(u64,u64)>, gpu:Option<GpuCounters>, thermal:Option<ThermalCounters>, fr:bool }
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
        if self.thermal.is_none(){self.thermal=ThermalCounters::open();}
        let cpu_temperature=self.thermal.as_ref().and_then(ThermalCounters::sample);
        // Nvml::init loads nvml.dll dynamically; NVIDIA absent or inaccessible means unavailable.
        let gpu_temperature=nvml_wrapper::Nvml::init().ok().and_then(|nvml|nvml.device_by_index(0).ok()
            .and_then(|dev|dev.temperature(nvml_wrapper::enum_wrappers::device::TemperatureSensor::Gpu).ok())).map(|v|v as f64).filter(|v|(0.0..=125.0).contains(v));
        for (ctx,action) in &self.contexts {
            let (lines,gauge):(Vec<String>,Option<f32>)=match action.as_str(){
                CPU=>(vec!["CPU".into(),percent(cpu),if cpu.is_none(){unavailable(self.fr).into()}else{String::new()}],cpu.map(|v|(v/100.0) as f32)),
                RAM=>(vec!["RAM".into(),percent(ram.map(|x|x.0)),ram.map(|x|format!("{:.1} GiB",x.1)).unwrap_or_else(||unavailable(self.fr).into())],ram.map(|x|(x.0/100.0) as f32)),
                GPU=>(vec!["GPU".into(),percent(gpu),if gpu.is_none(){unavailable(self.fr).into()}else{String::new()}],gpu.map(|v|(v/100.0) as f32)),
                TEMP=>(temperature_lines(cpu_temperature,gpu_temperature,self.fr),None),
                _=>continue
            };
            sdk.image(ctx,&novydeck_sdk::render(&lines.iter().map(String::as_str).collect::<Vec<_>>(),gauge)?)?;
        }
        Ok(())
    }
}
fn info_is_french(args:impl IntoIterator<Item=String>)->bool {
    let mut args=args.into_iter();
    while let Some(key)=args.next() {
        if key=="-info" {
            return args.next().and_then(|raw|serde_json::from_str::<serde_json::Value>(&raw).ok())
                .and_then(|info|info["application"]["language"].as_str().map(|s|s.starts_with("fr"))).unwrap_or(false);
        }
        let _=args.next();
    }
    false
}
fn main(){let monitor=Monitor {fr:info_is_french(std::env::args().skip(1)),..Default::default()};
    if let Err(e)=novydeck_sdk::run(monitor){eprintln!("sysmonitor: {e}");std::process::exit(1)}}
#[cfg(test)] mod tests {use super::*;
 #[test]fn formatting(){assert_eq!(percent(None),"N/D");assert_eq!(percent(Some(f64::NAN)),"N/D");assert_eq!(percent(Some(105.0)),"100%");assert_eq!(cpu_load((2,10),(3,20)),Some(90.0));
    assert_eq!(kelvin_to_celsius(273.15),Some(0.0));assert_eq!(kelvin_to_celsius(398.15),Some(125.0));
    assert_eq!(kelvin_to_celsius(0.0),None);assert_eq!(kelvin_to_celsius(f64::NAN),None);
    assert_eq!(temperature(None),"N/D");assert_eq!(temperature(Some(50.4)),"50°C");
    assert_eq!(temperature_lines(None,None,true)[2],"capteur indisponible");
    assert_eq!(temperature_lines(None,None,false)[2],"sensor unavailable");}
 #[test]fn png(){let p=novydeck_sdk::render(&["GPU","N/D","sensor unavailable"],None).unwrap();assert!(p.len()>100);}
 #[test]fn gpu_engines(){
     let parse=|s|engine_key(s).unwrap();
     let a=parse("\\GPU Engine(pid_1_luid_0x00000000_0x00000001_phys_0_eng_0_engtype_3D)\\Utilization Percentage");
     let b=parse("\\GPU Engine(pid_2_luid_0x00000000_0x00000001_phys_0_eng_0_engtype_3D)\\Utilization Percentage");
     let copy=parse("\\GPU Engine(pid_1_luid_0x00000000_0x00000001_phys_0_eng_1_engtype_Copy)\\Utilization Percentage");
     let other=parse("\\GPU Engine(pid_1_luid_0x00000000_0x00000002_phys_0_eng_0_engtype_3D)\\Utilization Percentage");
     assert_eq!(engine_usage([(&a,35.0),(&b,40.0),(&copy,20.0),(&other,90.0)],(0,1)),Some(75.0));
     assert_eq!(engine_usage([(&a,80.0),(&b,80.0)],(0,1)),Some(100.0));
     assert_eq!(engine_usage([(&other,90.0)],(0,1)),None);
     assert_eq!(engine_usage([(&a,f64::NAN)],(0,1)),None);
 }
}
