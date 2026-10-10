//! Checks ac-core against a real install: decrypts every car's data.acd,
//! verifies a byte-exact re-pack, and scans content.
//!
//! cargo run -p ac-core --example verify [AC_ROOT]

use ac_core::{acd, content, paths};

fn main() -> anyhow::Result<()> {
    let root = std::env::args()
        .nth(1)
        .map(Into::into)
        .or_else(paths::find_ac_root)
        .expect("AC not found; pass the path");
    println!("AC root: {}", root.display());
    println!("race.ini: {:?}", paths::race_ini_path(&root));

    let cars = content::scan_cars(&root);
    let (mut ok, mut bad, mut no_acd, mut repack_mismatch) = (0, 0, 0, 0);
    for car in &cars {
        let path = car.dir.join("data.acd");
        if !path.is_file() {
            no_acd += 1;
            continue;
        }
        match acd::read(&path) {
            Ok(entries) => {
                let car_ini = entries.iter().find(|e| e.name.eq_ignore_ascii_case("car.ini"));
                let readable = car_ini.is_some_and(|e| String::from_utf8_lossy(&e.data).contains("[HEADER]"));
                if readable {
                    ok += 1;
                } else {
                    bad += 1;
                    println!("  undecodable: {}", car.id);
                }
                // Re-encode and decode again: must give identical entries.
                let bytes = acd::write_bytes(&entries, &acd::key_source_for(&path))?;
                if acd::read_bytes(&bytes, &car.id)? != entries {
                    repack_mismatch += 1;
                }
            }
            Err(e) => {
                bad += 1;
                println!("  error {}: {e:#}", car.id);
            }
        }
    }
    println!("cars: {} | data.acd decoded: {ok}, failed: {bad}, unpacked-only: {no_acd}, repack mismatches: {repack_mismatch}", cars.len());
    println!("cars with ui_car.json problems: {}", cars.iter().filter(|c| c.ui_error.is_some()).count());
    for c in cars.iter().filter(|c| c.ui_error.is_some()).take(5) {
        println!("  {}: {}", c.id, c.ui_error.as_deref().unwrap());
    }

    let tracks = content::scan_tracks(&root);
    let entries = content::track_entries(&tracks);
    println!("tracks: {} ({} layouts)", tracks.len(), entries.len());
    for e in entries.iter().take(8) {
        println!("  {:<40} {}", e.key(), e.layout.name);
    }
    let weather = content::scan_weather(&root);
    println!("weather: {:?}", weather.iter().map(|w| &w.name).collect::<Vec<_>>());

    // race.ini for a 3-car quick race, built on Kunos' template (printed, not saved).
    if let (Some(car), Some(track)) = (cars.iter().find(|c| c.id == "abarth500"), entries.iter().find(|e| e.key() == "ks_silverstone/gp")) {
        let skins: Vec<String> = car.skins.iter().map(|s| s.id.clone()).collect();
        let mut q = ac_core::race::QuickDrive {
            car_id: car.id.clone(),
            skin_id: skins[0].clone(),
            track_id: track.track_id.clone(),
            layout_id: track.layout.layout_id.clone(),
            driver_name: "Player".into(),
            nationality: "ITA".into(),
            mode: ac_core::race::Mode::Race { laps: 3, opponents: 2, ai_level: 95, starting_position: 3 },
            penalties: true,
            ..Default::default()
        };
        q.opponents = ac_core::race::pick_opponents(&car.id, &skins[0], &skins, 2);
        let mut ini = ac_core::ini::IniFile::load(&root.join("cfg").join("race.ini"))?;
        ac_core::race::apply(&mut ini, &q);
        println!("--- generated race.ini ---\n{}", ini.to_string());
    }

    if let Some(car) = cars.iter().find(|c| c.id == "abarth500") {
        let entries = acd::read(&car.dir.join("data.acd"))?;
        let ini = entries.iter().find(|e| e.name == "car.ini").unwrap();
        println!("--- abarth500 car.ini (first lines) ---");
        for line in String::from_utf8_lossy(&ini.data).lines().take(6) {
            println!("{line}");
        }
    }
    Ok(())
}
