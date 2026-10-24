fn main() {
    let log = std::env::var("WALLPAPER_FIXTURE_LOG").expect("fixture log");
    let args: Vec<_> = std::env::args().skip(1).collect();
    std::fs::write(log, args.join("\n")).expect("write arguments");
    std::process::exit(std::env::var("WALLPAPER_FIXTURE_EXIT").unwrap_or_default().parse().unwrap_or(0));
}
