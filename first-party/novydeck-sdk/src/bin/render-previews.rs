use std::{fs, path::PathBuf};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../previews");
    fs::create_dir_all(&root)?;
    // Les chiffres des aperçus proviennent de la machine de compilation, jamais d'exemples inventés.
    let cpu = std::env::var("NOVYDECK_PREVIEW_CPU").ok().and_then(|v| v.parse::<u32>().ok()).filter(|v| *v <= 100);
    let ram = std::env::var("NOVYDECK_PREVIEW_RAM").ok().and_then(|v| v.parse::<u32>().ok()).filter(|v| *v <= 100);
    for (name, lines, gauge) in [
        ("clock", vec!["12:34:56".into(), "22/10/2026".into()], None),
        ("timer", vec!["Timer".into(), "03:25".into()], Some(0.68)),
        ("stopwatch", vec!["Stopwatch".into(), "01:42".into()], None),
        ("counter", vec!["Count".into(), "42".into()], None),
        ("note", vec!["Note".into(), "Hello!".into()], None),
        ("clipboard", vec!["Clipboard".into(), "Saved text".into()], None),
        ("cpu", vec!["CPU".into(), cpu.map(|v| format!("{v}%")).unwrap_or_else(|| "—".into())], cpu.map(|v| v as f32 / 100.0)),
        ("ram", vec!["RAM".into(), ram.map(|v| format!("{v}%")).unwrap_or_else(|| "—".into())], ram.map(|v| v as f32 / 100.0)),
        ("gpu", vec!["GPU".into(), "—".into()], None),
        ("temperature", vec!["CPU n/d".into(), "GPU —".into()], None),
        ("play", vec!["Play/Pause".into(), "Example".into(), "Artist".into()], None),
        ("next", vec!["Next".into(), "Example".into()], None),
        ("previous", vec!["Previous".into(), "Example".into()], None),
        ("saver", vec!["OLED OFF".into()], None),
        ("toggle", vec!["Maison".into(), "on".into()], Some(1.0)),
        ("service", vec!["Service".into(), "light.turn_on".into()], None),
        ("state", vec!["État".into(), "19.5 °C".into()], None),
        ("googleassistant-command", vec!["Google".into(), "Commande".into()], None),
        ("googlehome-control", vec!["Home".into(), "Appareil".into()], None),
        ("wallpaperengine-wallpaper", vec!["Wallpaper".into(), "Fond".into()], None),
        ("wallpaperengine-playlist", vec!["Playlist".into()], None),
        ("wallpaperengine-next", vec!["Suivant".into()], None),
        ("wallpaperengine-profile", vec!["Profil".into()], None),
        ("wallpaperengine-pause", vec!["Pause".into()], None),
        ("wallpaperengine-play", vec!["Lecture".into()], None),
        ("wallpaperengine-stop", vec!["Stop".into()], None),
        ("wallpaperengine-mute", vec!["Muet".into()], None),
        ("wallpaperengine-unmute", vec!["Son".into()], None),
    ] { fs::write(root.join(format!("{name}.png")), novydeck_sdk::render(&lines.iter().map(String::as_str).collect::<Vec<_>>(), gauge)?)?; }
    Ok(())
}
