fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("usage: compare_session <session.json> <recorded.wav> [output.wav]");
        std::process::exit(1);
    }

    let session_path = &args[1];
    let recorded_path = &args[2];
    let output_path = args.get(3).map(|arg| arg.as_str());

    // The .bms does not record the limiter, so a wrong guess here diverges from
    // the reference without saying why. Print what was assumed.
    let stored = app_lib::settings::limiter_enabled();
    let limiter_enabled = stored.unwrap_or(true);
    println!(
        "limiter          : {} ({})",
        if limiter_enabled { "on" } else { "off" },
        match stored {
            Some(_) => "from settings.json",
            None => "default, no stored setting",
        }
    );

    match app_lib::offline_render::render_and_compare(
        session_path,
        recorded_path,
        output_path,
        if limiter_enabled {
            app_lib::offline_render::MasterLimiter::On
        } else {
            app_lib::offline_render::MasterLimiter::Off
        },
    ) {
        Err(error) => {
            eprintln!("error: {error}");
            std::process::exit(1);
        }
        Ok(report) => {
            println!("frames compared : {}", report.compared_frames);
            println!("max |diff|       : {:.6}", report.max_abs_diff);
            println!("RMS diff         : {:.1} dBFS", report.rms_diff_db);

            match report.first_divergence_frame {
                None => {
                    println!("first divergence : none");
                    println!("\nresult: PASS");
                }
                Some(div_frame) => {
                    let div_ms = div_frame as f64 * 1000.0 / report.sample_rate as f64;
                    println!("first divergence : frame {} (~{:.1} ms)", div_frame, div_ms);
                    println!("\nresult: FAIL. Reconstruction diverges from reference");

                    println!("\n--- events near divergence ({:.0} ms ± 3s) ---", div_ms);
                    dump_events_near(session_path, div_ms);

                    std::process::exit(2);
                }
            }
        }
    }
}

fn dump_events_near(session_path: &str, center_ms: f64) {
    let json = match std::fs::read_to_string(session_path) {
        Ok(contents) => contents,
        Err(error) => {
            eprintln!("  (could not read session: {error})");
            return;
        }
    };
    let parsed: serde_json::Value = match serde_json::from_str(&json) {
        Ok(parsed) => parsed,
        Err(error) => {
            eprintln!("  (could not parse session: {error})");
            return;
        }
    };
    let events = match parsed["events"].as_array() {
        Some(array) => array,
        None => {
            eprintln!("  (no events array)");
            return;
        }
    };

    let window_start_ms = center_ms - 3000.0;
    let window_end_ms = center_ms + 3000.0;
    let mut found = 0;
    for event in events {
        let ms = event["elapsed_ms"].as_f64().unwrap_or(0.0);
        if ms >= window_start_ms && ms <= window_end_ms {
            let marker = if (ms - center_ms).abs() < 100.0 {
                " <-- near divergence"
            } else {
                ""
            };
            println!("  {:>10.1} ms  {}{}", ms, event, marker);
            found += 1;
        }
    }
    if found == 0 {
        println!("  (no events in this window)");
    }
}
