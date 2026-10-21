mod desktop;

fn main() -> bevy::app::AppExit {
    if std::env::args().any(|arg| arg == "--licenses") {
        println!(
            "{}\n{}\n{}",
            include_str!("../LICENSE"),
            include_str!("../NOTICE.md"),
            include_str!("../THIRD_PARTY_LICENSES.txt")
        );
        return bevy::app::AppExit::Success;
    }
    desktop::run()
}
