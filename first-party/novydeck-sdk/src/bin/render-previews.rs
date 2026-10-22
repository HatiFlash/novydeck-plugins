use std::{fs, path::PathBuf};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../previews");
    fs::create_dir_all(&root)?;
    for (name, lines, gauge) in [
        ("clock", vec!["12:34:56", "22/10/2026"], None),
        ("timer", vec!["Timer", "03:25"], Some(0.68)),
        ("stopwatch", vec!["Stopwatch", "01:42"], None),
        ("counter", vec!["Count", "42"], None),
        ("note", vec!["Note", "Hello!"], None),
        ("clipboard", vec!["Clipboard", "Saved text"], None),
    ] { fs::write(root.join(format!("{name}.png")), novydeck_sdk::render(&lines, gauge)?)?; }
    Ok(())
}
