fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("usage: compare_session <session.json> <recorded.wav> [output.wav]");
        std::process::exit(1);
    }

    let session_path = &args[1];
    let recorded_path = &args[2];
    let output_path = args.get(3).map(|s| s.as_str());

    match app_lib::offline_render::render_and_compare(session_path, recorded_path, output_path) {
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
        Ok(r) => {
            println!("frames compared : {}", r.compared_frames);
            println!("max |diff|       : {:.6}", r.max_abs_diff);
            println!("RMS diff         : {:.1} dBFS", r.rms_diff_db);

            match r.first_divergence_frame {
                None => {
                    println!("first divergence : none (signals match within threshold)");
                    println!("\nresult: PASS — reconstruction matches reference (< -60 dBFS RMS error)");
                }
                Some(div_frame) => {
                    let div_ms = div_frame as f64 * 1000.0 / r.sample_rate as f64;
                    println!("first divergence : frame {} (~{:.1} ms)", div_frame, div_ms);
                    println!("\nresult: FAIL — reconstruction diverges from reference");

                    // Dump events in a ±3s window around the divergence.
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
        Ok(s) => s,
        Err(e) => { eprintln!("  (could not read session: {e})"); return; }
    };
    let v: serde_json::Value = match serde_json::from_str(&json) {
        Ok(v) => v,
        Err(e) => { eprintln!("  (could not parse session: {e})"); return; }
    };
    let events = match v["events"].as_array() {
        Some(a) => a,
        None => { eprintln!("  (no events array)"); return; }
    };

    let lo = center_ms - 3000.0;
    let hi = center_ms + 3000.0;
    let mut found = 0;
    for ev in events {
        let t = ev["elapsed_ms"].as_f64().unwrap_or(0.0);
        if t >= lo && t <= hi {
            let marker = if (t - center_ms).abs() < 100.0 { " <-- near divergence" } else { "" };
            println!("  {:>10.1} ms  {}{}", t, ev, marker);
            found += 1;
        }
    }
    if found == 0 {
        println!("  (no events in this window)");
    }
}
