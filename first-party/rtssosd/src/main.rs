//! RivaTuner OSD: show/hide the RTSS overlay through the documented-by-export `RTSSHooks64.dll`
//! (`GetFlags` / `SetFlags`), without simulated keystrokes. No RTSS code is bundled or redistributed.
use std::{collections::HashMap, path::{Path, PathBuf}};
use novydeck_sdk::{Client, Event, Plugin, StateSource};
use serde_json::{json, Value};

const DLL_NAME: &str = "RTSSHooks64.dll";
/// RTSSHOOKSFLAG_OSD_VISIBLE
const OSD_VISIBLE: u32 = 1;
const ACTION: &str = "com.novydeck.rtssosd.osd";
/// tick() runs every ~200 ms; poll the real state about once per second.
const POLL_EVERY: u32 = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode { On, Off, Toggle }
fn mode_checked(settings: &Value) -> Result<Mode, String> {
    match settings["mode"].as_str() {
        None | Some("toggle") => Ok(Mode::Toggle),
        Some("on") => Ok(Mode::On),
        Some("off") => Ok(Mode::Off),
        _ => Err("Invalid RivaTuner OSD mode".into()),
    }
}
/// (andMask, xorMask) for `SetFlags`: new = (old & and) ^ xor.
fn masks(mode: Mode) -> (u32, u32) {
    match mode {
        Mode::On => (!OSD_VISIBLE, OSD_VISIBLE),
        Mode::Off => (!OSD_VISIBLE, 0),
        Mode::Toggle => (0xFFFF_FFFF, OSD_VISIBLE),
    }
}
fn osd_visible(flags: u32) -> bool { flags & OSD_VISIBLE != 0 }
/// Expected visibility after applying `mode` when it was `before`.
fn expected(mode: Mode, before: bool) -> bool {
    match mode { Mode::On => true, Mode::Off => false, Mode::Toggle => !before }
}
/// Install directories to try, in order: registry values first, then the default locations.
fn dll_candidates(registry_dirs: &[String]) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = registry_dirs.iter().filter(|d| !d.trim().is_empty()).map(|d| PathBuf::from(d.trim())).collect();
    dirs.push(PathBuf::from(r"C:\Program Files (x86)\RivaTuner Statistics Server"));
    dirs.push(PathBuf::from(r"C:\Program Files\RivaTuner Statistics Server"));
    dirs.into_iter().map(|d| d.join(DLL_NAME)).collect()
}
/// Pure choice: first candidate that exists.
fn pick_dll(candidates: &[PathBuf], exists: impl Fn(&Path) -> bool) -> Option<PathBuf> {
    candidates.iter().find(|p| exists(p)).cloned()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Failure { NotFound, NotRunning, Invalid }
impl Failure {
    fn code(self) -> &'static str { match self { Failure::NotFound => "notFound", Failure::NotRunning => "notRunning", Failure::Invalid => "invalidMode" } }
}

#[cfg(windows)]
mod rtss {
    use std::{os::windows::ffi::OsStrExt, path::PathBuf, ptr::{null, null_mut}};
    use windows_sys::Win32::{Foundation::HMODULE, System::{LibraryLoader::{GetProcAddress, LoadLibraryW}, Registry::*}};

    pub struct Hooks { _module: HMODULE, get: unsafe extern "C" fn() -> u32, set: unsafe extern "C" fn(u32, u32) -> u32 }
    impl Hooks {
        pub fn get_flags(&self) -> u32 { unsafe { (self.get)() } }
        pub fn set_flags(&self, and: u32, xor: u32) -> u32 { unsafe { (self.set)(and, xor) } }
    }
    fn wide(s: &std::ffi::OsStr) -> Vec<u16> { s.encode_wide().chain(Some(0)).collect() }
    fn install_dir(subkey: &str) -> Option<String> {
        unsafe {
            let key: Vec<u16> = subkey.encode_utf16().chain(Some(0)).collect();
            let mut h: HKEY = null_mut();
            if RegOpenKeyExW(HKEY_LOCAL_MACHINE, key.as_ptr(), 0, KEY_READ, &mut h) != 0 { return None; }
            let name: Vec<u16> = "InstallDir".encode_utf16().chain(Some(0)).collect();
            let (mut kind, mut len) = (0u32, 0u32);
            let mut out = None;
            if RegQueryValueExW(h, name.as_ptr(), null(), &mut kind, null_mut(), &mut len) == 0 && kind == REG_SZ && len >= 2 && len < 65536 {
                let mut buf = vec![0u16; (len as usize).div_ceil(2) + 1];
                let mut size = len;
                if RegQueryValueExW(h, name.as_ptr(), null(), &mut kind, buf.as_mut_ptr() as *mut u8, &mut size) == 0 {
                    let n = buf.iter().position(|c| *c == 0).unwrap_or(buf.len());
                    out = Some(String::from_utf16_lossy(&buf[..n]));
                }
            }
            RegCloseKey(h);
            out
        }
    }
    pub fn registry_dirs() -> Vec<String> {
        ["SOFTWARE\\WOW6432Node\\Unwinder\\RTSS", "SOFTWARE\\Unwinder\\RTSS"].iter().filter_map(|k| install_dir(k)).collect()
    }
    /// Loads by FULL path only, never by bare name.
    pub fn load(path: &PathBuf) -> Option<Hooks> {
        if !path.is_absolute() { return None; }
        unsafe {
            let module = LoadLibraryW(wide(path.as_os_str()).as_ptr());
            if module.is_null() { return None; }
            let get = GetProcAddress(module, b"GetFlags\0".as_ptr())?;
            let set = GetProcAddress(module, b"SetFlags\0".as_ptr())?;
            Some(Hooks { _module: module, get: std::mem::transmute(get), set: std::mem::transmute(set) })
        }
    }
}
#[cfg(not(windows))]
mod rtss {
    use std::path::PathBuf;
    pub struct Hooks;
    impl Hooks { pub fn get_flags(&self) -> u32 { 0 } pub fn set_flags(&self, _: u32, _: u32) -> u32 { 0 } }
    pub fn registry_dirs() -> Vec<String> { Vec::new() }
    pub fn load(_: &PathBuf) -> Option<Hooks> { None }
}

#[derive(Default)]
struct Osd { contexts: HashMap<String, Value>, hooks: Option<rtss::Hooks>, shown: HashMap<String, Option<bool>>, ticks: u32 }
impl Osd {
    /// Loaded on demand, handle kept.
    fn hooks(&mut self) -> Option<&rtss::Hooks> {
        if self.hooks.is_none() {
            let path = pick_dll(&dll_candidates(&rtss::registry_dirs()), |p| p.is_file())?;
            self.hooks = rtss::load(&path);
        }
        self.hooks.as_ref()
    }
    fn visible(&mut self) -> Option<bool> { self.hooks().map(|h| osd_visible(h.get_flags())) }
    fn apply(&mut self, mode: Mode) -> Result<bool, Failure> {
        let hooks = self.hooks().ok_or(Failure::NotFound)?;
        let before = osd_visible(hooks.get_flags());
        let (and, xor) = masks(mode);
        hooks.set_flags(and, xor);
        let after = osd_visible(hooks.get_flags());
        if after == expected(mode, before) { Ok(after) } else { Err(Failure::NotRunning) }
    }
    /// Draws the key and reports the measured state, only when it differs from what is displayed.
    fn display(&mut self, ctx: &str, sdk: &mut Client, force: bool) -> Result<(), String> {
        let now = self.visible();
        if !force && self.shown.get(ctx) == Some(&now) { return Ok(()); }
        let label = match now { Some(true) => "OSD ON", Some(false) => "OSD OFF", None => "OSD ?" };
        sdk.image(ctx, &novydeck_sdk::render(&[label], None)?)?;
        match now { Some(on) => sdk.state(ctx, on, StateSource::Confirmed)?, None => sdk.state_unknown(ctx)? }
        self.shown.insert(ctx.to_string(), now);
        Ok(())
    }
}
impl Plugin for Osd {
    fn event(&mut self, event: Event, sdk: &mut Client) -> Result<(), String> {
        if !event.action.is_empty() && event.action != ACTION { return Ok(()); }
        let ctx = event.context.clone();
        match event.name.as_str() {
            "willAppear" | "didReceiveSettings" => { self.contexts.insert(ctx.clone(), event.settings().clone()); self.display(&ctx, sdk, true)?; }
            "willDisappear" => { self.contexts.remove(&ctx); self.shown.remove(&ctx); }
            "propertyInspectorDidAppear" => { sdk.inspector(&ctx, json!({"status": "ready"}))?; }
            // NovyDeck sends keyDown then keyUp to virtual blocks: act on keyDown only.
            "keyDown" => {
                let settings = self.contexts.get(&ctx).cloned().unwrap_or(Value::Null);
                let result = mode_checked(&settings).map_err(|_| Failure::Invalid).and_then(|m| self.apply(m));
                match result {
                    Ok(_) => { sdk.ok(&ctx)?; sdk.inspector(&ctx, json!({"status": "ok"}))?; }
                    Err(f) => { sdk.alert(&ctx)?; sdk.inspector(&ctx, json!({"status": f.code()}))?; }
                }
                self.display(&ctx, sdk, true)?;
            }
            _ => {}
        }
        Ok(())
    }
    fn tick(&mut self, sdk: &mut Client) -> Result<(), String> {
        self.ticks += 1;
        if self.ticks % POLL_EVERY != 0 || self.contexts.is_empty() { return Ok(()); }
        let ctxs: Vec<String> = self.contexts.keys().cloned().collect();
        for ctx in ctxs { self.display(&ctx, sdk, false)?; }
        Ok(())
    }
}
fn main() { if let Err(e) = novydeck_sdk::run(Osd::default()) { eprintln!("rtssosd: {e}"); std::process::exit(1) } }

#[cfg(test)]
mod tests {
    use super::*;
    fn apply(m: Mode, old: u32) -> u32 { let (a, x) = masks(m); (old & a) ^ x }
    #[test] fn on_forces_bit_and_keeps_other_flags() { assert_eq!(apply(Mode::On, 0), 1); assert_eq!(apply(Mode::On, 1), 1); assert_eq!(apply(Mode::On, 0b110), 0b111); }
    #[test] fn off_clears_bit_and_keeps_other_flags() { assert_eq!(apply(Mode::Off, 1), 0); assert_eq!(apply(Mode::Off, 0), 0); assert_eq!(apply(Mode::Off, 0b111), 0b110); }
    #[test] fn toggle_inverts_only_the_bit() { assert_eq!(apply(Mode::Toggle, 0), 1); assert_eq!(apply(Mode::Toggle, 1), 0); assert_eq!(apply(Mode::Toggle, 0b110), 0b111); }
    #[test] fn exact_masks() { assert_eq!(masks(Mode::On), (0xFFFF_FFFE, 1)); assert_eq!(masks(Mode::Off), (0xFFFF_FFFE, 0)); assert_eq!(masks(Mode::Toggle), (0xFFFF_FFFF, 1)); }
    #[test] fn modes_parse() {
        assert_eq!(mode_checked(&json!({"mode": "on"})), Ok(Mode::On));
        assert_eq!(mode_checked(&json!({"mode": "off"})), Ok(Mode::Off));
        assert_eq!(mode_checked(&json!({"mode": "toggle"})), Ok(Mode::Toggle));
    }
    #[test] fn missing_mode_is_toggle() { assert_eq!(mode_checked(&Value::Null), Ok(Mode::Toggle)); assert_eq!(mode_checked(&json!({})), Ok(Mode::Toggle)); }
    #[test] fn invalid_mode_is_an_error() { assert!(mode_checked(&json!({"mode": "bogus"})).is_err()); assert!(mode_checked(&json!({"mode": 3})).is_err()); }
    #[test] fn bit_interpretation() { assert!(osd_visible(1)); assert!(osd_visible(0b11)); assert!(!osd_visible(0)); assert!(!osd_visible(0b10)); }
    #[test] fn expected_visibility() {
        assert!(expected(Mode::On, false) && expected(Mode::On, true));
        assert!(!expected(Mode::Off, false) && !expected(Mode::Off, true));
        assert!(expected(Mode::Toggle, false) && !expected(Mode::Toggle, true));
    }
    #[test] fn candidates_order_registry_then_defaults() {
        let c = dll_candidates(&[r"D:\RTSS".to_string(), "  ".to_string()]);
        assert_eq!(c.len(), 3);
        assert_eq!(c[0], PathBuf::from(r"D:\RTSS").join(DLL_NAME));
        assert_eq!(c[1], PathBuf::from(r"C:\Program Files (x86)\RivaTuner Statistics Server").join(DLL_NAME));
        assert_eq!(c[2], PathBuf::from(r"C:\Program Files\RivaTuner Statistics Server").join(DLL_NAME));
        assert!(c.iter().all(|p| p.file_name().unwrap() == DLL_NAME));
    }
    #[test] fn pick_first_existing_candidate() {
        let c = dll_candidates(&[r"D:\RTSS".to_string()]);
        assert_eq!(pick_dll(&c, |p| p == c[0]), Some(c[0].clone()));
        assert_eq!(pick_dll(&c, |p| p == c[2] || p == c[1]), Some(c[1].clone()));
        assert_eq!(pick_dll(&c, |p| p == c[2]), Some(c[2].clone()));
        assert_eq!(pick_dll(&c, |_| false), None);
    }
    #[test] fn failure_codes_are_distinct() { assert_ne!(Failure::NotFound.code(), Failure::NotRunning.code()); assert_ne!(Failure::Invalid.code(), Failure::NotRunning.code()); }
    #[test] fn images_render() { for l in ["OSD ON", "OSD OFF", "OSD ?"] { assert!(novydeck_sdk::render(&[l], None).unwrap().len() > 100); } }
}
